//! Forge is installed by its checksum-verified official client installer, in a
//! disposable directory. Only completed library/version files enter an instance.
use super::{input_error, security, ContentService, FileHashes, FileTransaction};
use crate::{
    error::LauncherError,
    metadata::models::VersionJson,
    paths::AppPaths,
    runtime::{requirement_for_version, JavaRuntimeState},
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

const MAX_FILES: usize = 20_000;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Inventory {
    version: String,
    files: Vec<InventoryFile>,
}
#[derive(Serialize, Deserialize)]
struct InventoryFile {
    path: String,
    size: u64,
    sha1: String,
}

fn valid_number(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn validate_versions(minecraft: &str, loader: &str) -> Result<(), LauncherError> {
    if !valid_number(minecraft) || !valid_number(loader) {
        return Err(input_error(
            "forge_version_invalid",
            "Неверная версия Minecraft или Forge.",
        ));
    }
    let pieces: Vec<_> = minecraft.split('.').collect();
    if pieces.first() != Some(&"1")
        || pieces
            .get(1)
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0)
            < 13
    {
        return Err(input_error(
            "forge_version_unsupported",
            "Поддерживается официальный установщик Forge для Minecraft 1.13 и новее.",
        ));
    }
    Ok(())
}
fn invalid_install() -> LauncherError {
    input_error("forge_installation_invalid", "Файлы Forge отсутствуют или повреждены. Откройте настройки сборки и нажмите «Переустановить и восстановить».")
}
fn checksum_network_error() -> LauncherError {
    input_error("forge_download_unavailable", "Не удалось получить контрольную сумму с официального сервера Forge. Проверьте соединение и повторите установку.")
}

async fn installer_checksum(
    client: &reqwest::Client,
    url: &str,
    token: &crate::downloads::DownloadCancellationToken,
) -> Result<String, LauncherError> {
    for attempt in 0..3 {
        let response = tokio::select! {
            _ = token.cancelled() => return Err(security::cancelled()),
            response = client.get(url).send() => response,
        };
        let retry = match &response {
            Ok(response) => {
                response.status().is_server_error()
                    || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            }
            Err(error) => error.is_timeout() || error.is_connect(),
        };
        if retry && attempt < 2 {
            tokio::select! {
                _ = token.cancelled() => return Err(security::cancelled()),
                _ = tokio::time::sleep(Duration::from_millis(300 * (attempt + 1))) => {},
            }
            continue;
        }
        let mut response = response
            .map_err(|_| checksum_network_error())?
            .error_for_status()
            .map_err(|_| checksum_network_error())?;
        security::validate_url(response.url().as_str())?;
        let mut checksum = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = token.cancelled() => return Err(security::cancelled()),
                chunk = response.chunk() => chunk.map_err(|_| checksum_network_error())?,
            };
            let Some(chunk) = chunk else { break };
            checksum.extend_from_slice(&chunk);
            if checksum.len() > 128 {
                return Err(security::limit_error());
            }
        }
        let sha1 = String::from_utf8(checksum)
            .map_err(|_| checksum_network_error())?
            .trim()
            .to_owned();
        security::validate_hashes(None, Some(&sha1))?;
        return Ok(sha1);
    }
    Err(checksum_network_error())
}
fn inventory_path(version: &str) -> PathBuf {
    Path::new("versions")
        .join(version)
        .join("forge-installation.json")
}
fn file_hash(path: &Path) -> Result<(u64, String), LauncherError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_install())?;
    if !metadata.is_file() || crate::paths::is_reparse_point(&metadata) || metadata.len() > MAX_FILE
    {
        return Err(invalid_install());
    }
    let mut reader = fs::File::open(path)
        .map_err(|_| invalid_install())?
        .take(MAX_FILE + 1);
    let mut digest = Sha1::new();
    let mut buffer = [0u8; 65536];
    let mut length = 0;
    loop {
        let count = reader.read(&mut buffer).map_err(|_| invalid_install())?;
        if count == 0 {
            break;
        }
        length += count as u64;
        digest.update(&buffer[..count]);
    }
    if length != metadata.len() {
        return Err(invalid_install());
    }
    Ok((length, format!("{:x}", digest.finalize())))
}
fn collect_files(root: &Path) -> Result<Vec<InventoryFile>, LauncherError> {
    let safety = AppPaths::new(root.to_owned());
    let mut pending = vec![PathBuf::from("libraries"), PathBuf::from("versions")];
    let mut files = Vec::new();
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut entries = 0usize;
    while let Some(relative) = pending.pop() {
        let directory = safety.safe_join(root, &relative)?;
        for entry in fs::read_dir(directory).map_err(|_| invalid_install())? {
            entries += 1;
            if entries > MAX_FILES {
                return Err(invalid_install());
            }
            let entry = entry.map_err(|_| invalid_install())?;
            let relative = relative.join(entry.file_name());
            let file = safety.safe_join(root, &relative)?;
            if file.is_dir() {
                pending.push(relative);
                continue;
            }
            let path = relative
                .to_str()
                .ok_or_else(invalid_install)?
                .replace('\\', "/");
            if !names.insert(path.to_lowercase()) {
                return Err(invalid_install());
            }
            let (size, sha1) = file_hash(&file)?;
            total = total.checked_add(size).ok_or_else(invalid_install)?;
            if total > MAX_TOTAL {
                return Err(invalid_install());
            }
            files.push(InventoryFile { path, size, sha1 });
        }
    }
    if files.is_empty() {
        return Err(invalid_install());
    }
    Ok(files)
}

pub(crate) fn verify_installation(root: &Path, version: &str) -> Result<(), LauncherError> {
    if !version.starts_with("forge-loader-") {
        return Ok(());
    }
    crate::installer::validate_version_id(version)?;
    let safety = AppPaths::new(root.to_owned());
    let path = safety.safe_join(root, &inventory_path(version))?;
    let file = fs::File::open(path).map_err(|_| invalid_install())?;
    if file.metadata().map_err(|_| invalid_install())?.len() > 4 * 1024 * 1024 {
        return Err(invalid_install());
    }
    let inventory: Inventory = serde_json::from_reader(file).map_err(|_| invalid_install())?;
    if inventory.version != version
        || inventory.files.is_empty()
        || inventory.files.len() > MAX_FILES
    {
        return Err(invalid_install());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for entry in inventory.files {
        let relative = security::relative_path(&entry.path)?;
        if !matches!(
            relative
                .components()
                .next()
                .and_then(|v| v.as_os_str().to_str()),
            Some("libraries" | "versions")
        ) || !names.insert(entry.path.to_lowercase())
            || entry.size > MAX_FILE
            || entry.sha1.len() != 40
            || !entry.sha1.bytes().all(|v| v.is_ascii_hexdigit())
        {
            return Err(invalid_install());
        }
        total = total.checked_add(entry.size).ok_or_else(invalid_install)?;
        if total > MAX_TOTAL {
            return Err(invalid_install());
        }
        let actual = file_hash(&safety.safe_join(root, &relative)?)?;
        if actual.0 != entry.size || !actual.1.eq_ignore_ascii_case(&entry.sha1) {
            return Err(invalid_install());
        }
    }
    Ok(())
}

fn read_profile(
    installer: &Path,
    minecraft: &str,
    loader: &str,
) -> Result<VersionJson, LauncherError> {
    let mut zip = zip::ZipArchive::new(fs::File::open(installer).map_err(|_| invalid_install())?)
        .map_err(|_| invalid_install())?;
    let mut read = |name| -> Result<serde_json::Value, LauncherError> {
        let entry = zip.by_name(name).map_err(|_| invalid_install())?;
        if entry.size() > 4 * 1024 * 1024 {
            return Err(invalid_install());
        }
        serde_json::from_reader(entry.take(4 * 1024 * 1024 + 1)).map_err(|_| invalid_install())
    };
    let install = read("install_profile.json")?;
    if install["spec"] != 1
        || install["minecraft"] != minecraft
        || install["json"] != "/version.json"
    {
        return Err(input_error(
            "forge_installer_unsupported",
            "Этот формат установщика Forge пока не поддерживается. Выберите другую версию Forge.",
        ));
    }
    let profile: VersionJson =
        serde_json::from_value(read("version.json")?).map_err(|_| invalid_install())?;
    if profile.id != format!("{minecraft}-forge-{loader}")
        || profile.inherits_from.as_deref() != Some(minecraft)
        || install["version"] != profile.id
    {
        return Err(invalid_install());
    }
    Ok(profile)
}

impl ContentService {
    pub(super) async fn install_forge_profile(
        &self,
        minecraft: &str,
        requested: Option<&str>,
        game_root: &Path,
    ) -> Result<String, LauncherError> {
        let token = self.token();
        let loader = match requested {
            Some(version) => version.to_owned(),
            None => {
                if !valid_number(minecraft) {
                    return Err(invalid_install());
                }
                let promotions: serde_json::Value = self.json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json").await?;
                promotions["promos"][format!("{minecraft}-recommended")]
                    .as_str()
                    .or_else(|| promotions["promos"][format!("{minecraft}-latest")].as_str())
                    .ok_or_else(|| {
                        input_error(
                            "forge_unavailable",
                            "Для этой версии Minecraft не найден Forge.",
                        )
                    })?
                    .to_owned()
            }
        };
        validate_versions(minecraft, &loader)?;
        AppPaths::new(game_root.to_owned()).validate_absolute_directory(game_root)?;
        if token.is_cancelled() {
            return Err(security::cancelled());
        }
        let version_id = format!("forge-loader-{loader}-{minecraft}");
        if verify_installation(game_root, &version_id).is_ok() {
            let original = format!("{minecraft}-forge-{loader}");
            let path = game_root
                .join("versions")
                .join(&original)
                .join(format!("{original}.json"));
            let file = fs::File::open(path).map_err(|_| invalid_install())?;
            if file.metadata().map_err(|_| invalid_install())?.len() > 4 * 1024 * 1024 {
                return Err(invalid_install());
            }
            let mut cached: VersionJson = serde_json::from_reader(file.take(4 * 1024 * 1024 + 1))
                .map_err(|_| invalid_install())?;
            if cached.id != original || cached.inherits_from.as_deref() != Some(minecraft) {
                return Err(invalid_install());
            }
            cached.id = version_id;
            if cached
                .logging
                .as_ref()
                .is_some_and(|v| v.as_object().is_some_and(|o| o.is_empty()))
            {
                cached.logging = None;
            }
            self.metadata.register_custom_version(&cached)?;
            return Ok(loader);
        }
        let url = format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft}-{loader}/forge-{minecraft}-{loader}-installer.jar");
        let sha1 = installer_checksum(&self.client, &format!("{url}.sha1"), &token).await?;
        let staging = tempfile::Builder::new()
            .prefix(".ck-forge-")
            .tempdir_in(game_root)
            .map_err(|_| LauncherError::storage_unavailable())?;
        let archive = security::download(
            &self.client,
            staging.path(),
            &url,
            &FileHashes {
                sha1: Some(sha1),
                sha512: None,
            },
            None,
            &token,
        )
        .await?;
        let installer = staging.path().join("installer.jar");
        fs::copy(archive.path(), &installer).map_err(|_| LauncherError::storage_unavailable())?;
        let mut profile = read_profile(&installer, minecraft, &loader)?;
        let base = self
            .metadata
            .resolved_version_cancellable(minecraft, token.clone())
            .await?;
        let requirement = requirement_for_version(&base)?;
        let mut runtime = self
            .runtimes
            .resolve_cancellable(requirement, None, token.clone())
            .await?;
        if runtime.state != JavaRuntimeState::Valid {
            runtime = self
                .runtimes
                .install_cancellable(requirement, token.clone())
                .await?;
        }
        let java = runtime.path.filter(|_| runtime.state == JavaRuntimeState::Valid).ok_or_else(|| input_error("forge_java_unavailable", "Для установки Forge нужна подходящая Java. Проверьте раздел Java в настройках."))?;
        fs::write(
            staging.path().join("launcher_profiles.json"),
            br#"{"profiles":{}}"#,
        )
        .map_err(|_| LauncherError::storage_unavailable())?;
        let log_path = staging.path().join("forge-install.log");
        let log = fs::File::create(&log_path).map_err(|_| LauncherError::storage_unavailable())?;
        let mut command = tokio::process::Command::new(java);
        command
            .args(["-Djava.awt.headless=true", "-jar"])
            .arg(&installer)
            .arg("--installClient")
            .arg(staging.path())
            .current_dir(staging.path())
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                log.try_clone()
                    .map_err(|_| LauncherError::storage_unavailable())?,
            ))
            .stderr(Stdio::from(log))
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .env_remove("CLASSPATH")
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|_| invalid_install())?;
        let status = tokio::select! {
            result = child.wait() => result.map_err(|_| invalid_install())?,
            _ = token.cancelled() => { let _ = child.kill().await; return Err(security::cancelled()); },
            _ = tokio::time::sleep(Duration::from_secs(15 * 60)) => { let _ = child.kill().await; return Err(input_error("forge_install_timeout", "Установка Forge заняла слишком много времени. Проверьте подключение и повторите.")); }
        };
        if !status.success() {
            // The installer has no account data; bound diagnostic output, never pass
            // launcher credentials or JVM options from a pack to this process.
            let tail = fs::File::open(log_path)
                .ok()
                .and_then(|mut file| {
                    let length = file.metadata().ok()?.len();
                    file.seek(SeekFrom::Start(length.saturating_sub(4000)))
                        .ok()?;
                    let mut bytes = Vec::new();
                    file.take(4000).read_to_end(&mut bytes).ok()?;
                    Some(String::from_utf8_lossy(&bytes).into_owned())
                })
                .unwrap_or_default();
            return Err(LauncherError::new("forge_install_failed", "Официальный установщик Forge не смог подготовить игру. Проверьте подключение и повторите установку.", Some(tail), true));
        }
        if token.is_cancelled() {
            return Err(security::cancelled());
        }
        let version = format!("forge-loader-{loader}-{minecraft}");
        let inventory = Inventory {
            version: version.clone(),
            files: collect_files(staging.path())?,
        };
        let mut transaction = FileTransaction::new(game_root)?;
        for entry in &inventory.files {
            if token.is_cancelled() {
                return Err(security::cancelled());
            }
            let relative = security::relative_path(&entry.path)?;
            transaction.replace(&relative, &staging.path().join(&relative))?;
        }
        let marker = staging.path().join("inventory.json");
        fs::write(
            &marker,
            serde_json::to_vec(&inventory).map_err(|_| invalid_install())?,
        )
        .map_err(|_| LauncherError::storage_unavailable())?;
        transaction.replace(&inventory_path(&version), &marker)?;
        profile.id = version;
        // Preserve Mojang's logging configuration when Forge supplies an empty object.
        if profile
            .logging
            .as_ref()
            .is_some_and(|v| v.as_object().is_some_and(|o| o.is_empty()))
        {
            profile.logging = None;
        }
        self.metadata.register_custom_version(&profile)?;
        transaction.commit();
        Ok(loader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_retries_transient_server_failures_with_a_bounded_count() {
        crate::tasks::block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let mut count = 0;
                for _ in 0..3 {
                    let (mut connection, _) =
                        tokio::time::timeout(Duration::from_secs(4), listener.accept())
                            .await
                            .unwrap()
                            .unwrap();
                    let mut request = [0; 2048];
                    connection.read(&mut request).await.unwrap();
                    connection.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                    count += 1;
                }
                count
            });
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            let token = crate::downloads::DownloadCancellationToken::new();
            let error =
                installer_checksum(&client, &format!("http://{address}/fixture.sha1"), &token)
                    .await
                    .unwrap_err();
            assert_eq!(error.code(), "forge_download_unavailable");
            assert_eq!(server.await.unwrap(), 3);
        });
    }

    #[test]
    fn checksum_cancellation_does_not_turn_into_a_corruption_error() {
        crate::tasks::block_on(async {
            let token = crate::downloads::DownloadCancellationToken::new();
            token.cancel();
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let error = installer_checksum(
                &client,
                "https://maven.minecraftforge.net/fixture.sha1",
                &token,
            )
            .await
            .unwrap_err();
            assert_eq!(error.code(), "operation_cancelled");
        });
    }

    #[test]
    fn forge_coordinates_are_numbers_not_paths_or_arguments() {
        assert!(validate_versions("1.20.1", "47.4.10").is_ok());
        for value in ["../47.4.10", "47/4", "47--server", "47.4.10?x", "47..4"] {
            assert!(validate_versions("1.20.1", value).is_err());
        }
        assert!(validate_versions("1.12.2", "14.23.5.2860").is_err());
    }
    #[test]
    fn inventory_detects_missing_and_modified_generated_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("libraries")).unwrap();
        fs::create_dir_all(root.path().join("versions/forge-loader-47.4.10-1.20.1")).unwrap();
        fs::write(root.path().join("libraries/generated.jar"), b"generated").unwrap();
        let version = "forge-loader-47.4.10-1.20.1";
        let inventory = Inventory {
            version: version.into(),
            files: collect_files(root.path()).unwrap(),
        };
        fs::write(
            root.path().join(inventory_path(version)),
            serde_json::to_vec(&inventory).unwrap(),
        )
        .unwrap();
        verify_installation(root.path(), version).unwrap();
        fs::write(root.path().join("libraries/generated.jar"), b"modified!").unwrap();
        assert!(verify_installation(root.path(), version).is_err());
        fs::remove_file(root.path().join("libraries/generated.jar")).unwrap();
        assert!(verify_installation(root.path(), version).is_err());
    }
}
