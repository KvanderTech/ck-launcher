//! Import policy: validate the entire plan before performing any network or filesystem mutations.
use super::{input_error, FileHashes, MrpackIndex};
use crate::{downloads::DownloadCancellationToken, error::LauncherError, paths::AppPaths};
use futures_util::StreamExt;
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::{
    collections::HashSet,
    fs::{self},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
pub(super) const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_INDEX: u64 = 4 * 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "github-releases.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    "maven.minecraftforge.net",
];
pub(super) fn validate_url(value: &str) -> Result<url::Url, LauncherError> {
    let url = url::Url::parse(value).map_err(|_| denied_url())?;
    if url.scheme() != "https"
        || url.port().is_some_and(|p| p != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !DOWNLOAD_HOSTS.contains(&url.host_str().unwrap_or(""))
    {
        return Err(denied_url());
    }
    Ok(url)
}
fn denied_url() -> LauncherError {
    input_error("download_url_denied", "Сборка содержит ссылку на неразрешённый источник. Разрешены проверяемые HTTPS-источники модов.")
}
pub(super) fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let url = attempt.url();
        let metadata = [
            "api.modrinth.com",
            "meta.fabricmc.net",
            "meta.quiltmc.org",
            "files.minecraftforge.net",
        ]
        .contains(&url.host_str().unwrap_or(""));
        if attempt.previous().len() >= 5
            || !(validate_url(url.as_str()).is_ok()
                || (metadata
                    && url.scheme() == "https"
                    && url.port().is_none()
                    && url.username().is_empty()
                    && url.password().is_none()))
        {
            attempt.error("Redirect destination is not permitted")
        } else {
            attempt.follow()
        }
    })
}
pub(super) fn validate_hashes(
    sha512: Option<&str>,
    sha1: Option<&str>,
) -> Result<(), LauncherError> {
    if sha512.is_none() && sha1.is_none() {
        return Err(input_error(
            "download_hash_required",
            "У файла нет контрольной суммы. Установка остановлена.",
        ));
    }
    for (hash, length) in [(sha512, 128), (sha1, 40)] {
        if hash.is_some_and(|h| h.len() != length || !h.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(input_error(
                "download_hash_invalid",
                "Контрольная сумма имеет неверный формат.",
            ));
        }
    }
    Ok(())
}
pub(super) fn relative_path(value: &str) -> Result<PathBuf, LauncherError> {
    if value.is_empty() || value.len() > 2048 || value.split(['/', '\\']).count() > 32 {
        return Err(LauncherError::invalid_path());
    }
    let mut path = PathBuf::new();
    for part in value.split(['/', '\\']) {
        let upper = part.split('.').next().unwrap_or("").to_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"]
            .contains(&upper.as_str())
            || ((upper.starts_with("COM") || upper.starts_with("LPT"))
                && upper.chars().count() == 4
                && "123456789¹²³".contains(upper.chars().last().unwrap()));
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.encode_utf16().count() > 255
            || part
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || device
            || part.to_ascii_lowercase().ends_with(".part")
            || part.starts_with(".ck-")
        {
            return Err(LauncherError::invalid_path());
        }
        path.push(part);
    }
    Ok(path)
}
pub(super) fn safe_destination(root: &Path, relative: &Path) -> Result<PathBuf, LauncherError> {
    let relative = relative_path(relative.to_str().ok_or_else(LauncherError::invalid_path)?)?;
    let paths = AppPaths::new(root.to_owned());
    paths.validate_absolute_directory(root)?;
    Ok(crate::paths::strip_verbatim_prefix(
        paths.safe_join(root, &relative)?,
    ))
}
pub(super) fn prepare_parent(root: &Path, relative: &Path) -> Result<PathBuf, LauncherError> {
    let target = safe_destination(root, relative)?;
    if let Some(parent) = relative.parent() {
        let mut current = PathBuf::new();
        for component in parent.components() {
            current.push(component);
            let directory = safe_destination(root, &current)?;
            if !directory.exists() {
                fs::create_dir(&directory).map_err(|_| LauncherError::storage_unavailable())?;
            }
            safe_destination(root, &current)?;
        }
    }
    safe_destination(root, relative)?;
    Ok(target)
}
pub(super) fn inspect_archive<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<MrpackIndex, LauncherError> {
    if archive.len() > MAX_ENTRIES {
        return Err(limit_error());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|_| invalid_archive())?;
        let name = entry.name().replace('\\', "/");
        relative_path(name.trim_end_matches('/'))?;
        if !names.insert(name.to_lowercase())
            || entry.unix_mode().is_some_and(|m| {
                let kind = m & 0o170000;
                kind != 0 && kind != 0o100000 && kind != 0o040000
            })
        {
            return Err(invalid_archive());
        }
        total = total.checked_add(entry.size()).ok_or_else(limit_error)?;
        if total > MAX_EXPANDED
            || entry.size() > MAX_FILE
            || (entry.size() > 1024 * 1024 && entry.size() / entry.compressed_size().max(1) > 200)
        {
            return Err(limit_error());
        }
    }
    let entry = archive
        .by_name("modrinth.index.json")
        .map_err(|_| invalid_archive())?;
    if entry.size() > MAX_INDEX {
        return Err(limit_error());
    }
    let mut bytes = Vec::new();
    entry
        .take(MAX_INDEX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_archive())?;
    if bytes.len() as u64 > MAX_INDEX {
        return Err(limit_error());
    }
    let index: MrpackIndex = serde_json::from_slice(&bytes).map_err(|_| invalid_archive())?;
    if index.format_version != 1
        || index.game != "minecraft"
        || index.name.trim().is_empty()
        || index.name.chars().count() > 100
        || index.files.len() > MAX_ENTRIES
    {
        return Err(invalid_archive());
    }
    if index
        .dependencies
        .keys()
        .any(|k| !["minecraft", "fabric-loader", "quilt-loader", "forge"].contains(&k.as_str()))
        || ["fabric-loader", "quilt-loader", "forge"]
            .iter()
            .filter(|key| index.dependencies.contains_key(**key))
            .count()
            > 1
    {
        return Err(input_error("loader_not_supported", "Эта сборка требует неподдерживаемый загрузчик или несколько загрузчиков одновременно. Доступны Vanilla, Fabric, Quilt и Forge; подмена на Vanilla запрещена."));
    }
    if !index.dependencies.contains_key("minecraft") {
        return Err(invalid_archive());
    }
    for value in index.dependencies.values() {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        {
            return Err(invalid_archive());
        }
    }
    let mut files = HashSet::new();
    let mut download_total = 0u64;
    for file in &index.files {
        let path = relative_path(&file.path)?;
        let key = path.to_string_lossy().replace('\\', "/").to_lowercase();
        if !files.insert(key.clone())
            || names.contains(&format!("overrides/{key}"))
            || names.contains(&format!("client-overrides/{key}"))
        {
            return Err(invalid_archive());
        }
        validate_hashes(file.hashes.sha512.as_deref(), file.hashes.sha1.as_deref())?;
        if file.downloads.is_empty() || file.downloads.len() > 8 {
            return Err(denied_url());
        }
        for url in &file.downloads {
            validate_url(url)?;
        }
        if file.file_size == 0 || file.file_size > MAX_FILE {
            return Err(limit_error());
        }
        download_total = download_total
            .checked_add(file.file_size)
            .ok_or_else(limit_error)?;
        if download_total > 8 * 1024 * 1024 * 1024 {
            return Err(limit_error());
        }
        if file
            .env
            .as_ref()
            .and_then(|e| e.client.as_deref())
            .is_some_and(|e| !["required", "optional", "unsupported"].contains(&e))
        {
            return Err(invalid_archive());
        }
    }
    Ok(index)
}
pub(super) fn invalid_archive() -> LauncherError {
    input_error(
        "mrpack_invalid",
        "Архив сборки повреждён или содержит неоднозначные пути.",
    )
}
pub(super) fn limit_error() -> LauncherError {
    input_error(
        "content_limit_exceeded",
        "Превышен безопасный предел размера или числа файлов сборки.",
    )
}
pub(super) fn cancelled() -> LauncherError {
    input_error("operation_cancelled", "Операция отменена.")
}
pub(super) async fn download(
    client: &reqwest::Client,
    root: &Path,
    url: &str,
    hashes: &FileHashes,
    size: Option<u64>,
    token: &DownloadCancellationToken,
) -> Result<tempfile::NamedTempFile, LauncherError> {
    validate_url(url)?;
    validate_hashes(hashes.sha512.as_deref(), hashes.sha1.as_deref())?;
    if size.is_some_and(|s| s > MAX_FILE) {
        return Err(limit_error());
    }
    let response = tokio::select! { _ = token.cancelled() => return Err(cancelled()), r = client.get(url).send() => r.map_err(|_| super::network_error())? }.error_for_status().map_err(|_| super::network_error())?;
    validate_url(response.url().as_str())?;
    if response
        .content_length()
        .is_some_and(|s| s > size.unwrap_or(MAX_FILE))
    {
        return Err(limit_error());
    }
    let mut file = tempfile::Builder::new()
        .prefix(".ck-download-")
        .tempfile_in(root)
        .map_err(|_| LauncherError::storage_unavailable())?;
    let mut stream = response.bytes_stream();
    let mut count = 0u64;
    let mut sha512 = Sha512::new();
    let mut sha1 = Sha1::new();
    loop {
        let chunk = tokio::select! { _ = token.cancelled() => return Err(cancelled()), c = stream.next() => c };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|_| super::network_error())?;
        count = count
            .checked_add(chunk.len() as u64)
            .ok_or_else(limit_error)?;
        if count > size.unwrap_or(MAX_FILE) {
            return Err(limit_error());
        }
        sha512.update(&chunk);
        sha1.update(&chunk);
        file.write_all(&chunk)
            .map_err(|_| LauncherError::storage_unavailable())?;
    }
    if size.is_some_and(|s| s != count)
        || hashes
            .sha512
            .as_ref()
            .is_some_and(|h| !h.eq_ignore_ascii_case(&format!("{:x}", sha512.finalize())))
        || hashes
            .sha1
            .as_ref()
            .is_some_and(|h| !h.eq_ignore_ascii_case(&format!("{:x}", sha1.finalize())))
    {
        return Err(input_error(
            "download_hash_mismatch",
            "Размер или контрольная сумма файла не совпадает.",
        ));
    }
    file.flush()
        .and_then(|_| file.as_file().sync_all())
        .and_then(|_| file.seek(SeekFrom::Start(0)))
        .map_err(|_| LauncherError::storage_unavailable())?;
    Ok(file)
}
/// Roll back every replaced file if validation, cancellation, or database commit fails.
pub(super) struct FileTransaction {
    root: PathBuf,
    backup: tempfile::TempDir,
    changes: Vec<(PathBuf, Option<PathBuf>)>,
    committed: bool,
}
impl FileTransaction {
    pub fn new(root: &Path) -> Result<Self, LauncherError> {
        AppPaths::new(root.to_owned()).validate_absolute_directory(root)?;
        Ok(Self {
            root: root.to_owned(),
            backup: tempfile::Builder::new()
                .prefix(".ck-rollback-")
                .tempdir_in(root)
                .map_err(|_| LauncherError::storage_unavailable())?,
            changes: Vec::new(),
            committed: false,
        })
    }
    pub fn replace(&mut self, relative: &Path, staged: &Path) -> Result<(), LauncherError> {
        let target = prepare_parent(&self.root, relative)?;
        let old = if target.exists() {
            if !target.is_file() {
                return Err(LauncherError::invalid_path());
            }
            let backup = self.backup.path().join(self.changes.len().to_string());
            fs::rename(&target, &backup).map_err(|_| LauncherError::storage_unavailable())?;
            Some(backup)
        } else {
            None
        };
        self.changes.push((target.clone(), old));
        fs::rename(staged, target).map_err(|_| LauncherError::storage_unavailable())
    }
    pub fn remove(&mut self, relative: &Path) -> Result<(), LauncherError> {
        let target = safe_destination(&self.root, relative)?;
        if target.exists() {
            if !target.is_file() {
                return Err(LauncherError::invalid_path());
            }
            let backup = self.backup.path().join(self.changes.len().to_string());
            fs::rename(&target, &backup).map_err(|_| LauncherError::storage_unavailable())?;
            self.changes.push((target, Some(backup)));
        }
        Ok(())
    }
    pub fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for FileTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let mut failed = false;
            for (target, old) in self.changes.iter().rev() {
                if target.exists() && fs::remove_file(target).is_err() {
                    failed = true;
                    continue;
                }
                if let Some(old) = old {
                    if fs::rename(old, target).is_err() {
                        failed = true;
                    }
                }
            }
            // Never delete the only remaining copy when a rollback cannot finish.
            if failed {
                self.backup.disable_cleanup(true);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn archive(
        index: serde_json::Value,
        extra: Option<(&str, &[u8])>,
    ) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
        use zip::write::SimpleFileOptions;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("modrinth.index.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(serde_json::to_string(&index).unwrap().as_bytes())
            .unwrap();
        if let Some((name, bytes)) = extra {
            zip.start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip::ZipArchive::new(std::io::Cursor::new(zip.finish().unwrap().into_inner())).unwrap()
    }
    fn index() -> serde_json::Value {
        serde_json::json!({"formatVersion":1,"game":"minecraft","versionId":"test","name":"Harmless test","dependencies":{"minecraft":"1.20.1"},"files":[]})
    }
    #[test]
    fn rejects_an_unsupported_loader_before_any_install() {
        let mut data = index();
        data["dependencies"]["neoforge"] = serde_json::json!("21.0.0");
        assert_eq!(
            inspect_archive(&mut archive(data, None))
                .unwrap_err()
                .code(),
            "loader_not_supported"
        );
    }
    #[test]
    fn accepts_forge_but_never_combines_or_substitutes_loaders() {
        let mut data = index();
        data["dependencies"]["forge"] = serde_json::json!("47.4.10");
        let parsed = inspect_archive(&mut archive(data.clone(), None)).unwrap();
        assert_eq!(parsed.dependencies["forge"], "47.4.10");
        for loader in ["fabric-loader", "quilt-loader"] {
            let mut mixed = data.clone();
            mixed["dependencies"][loader] = serde_json::json!("0.18.0");
            assert_eq!(
                inspect_archive(&mut archive(mixed, None))
                    .unwrap_err()
                    .code(),
                "loader_not_supported"
            );
        }
    }
    #[test]
    fn rejects_compression_bombs_and_override_traversal() {
        let bytes = vec![0; 2 * 1024 * 1024];
        assert_eq!(
            inspect_archive(&mut archive(index(), Some(("overrides/a", &bytes))))
                .unwrap_err()
                .code(),
            "content_limit_exceeded"
        );
        assert_eq!(
            inspect_archive(&mut archive(
                index(),
                Some(("overrides/../../outside", b"safe"))
            ))
            .unwrap_err()
            .code(),
            "invalid_path"
        );
    }
    #[test]
    fn rejects_an_invalid_late_file_during_preflight() {
        let mut data = index();
        let file = serde_json::json!({"path":"mods/a.jar","hashes":{"sha512":"ab".repeat(64)},"downloads":["https://cdn.modrinth.com/a.jar"],"fileSize":1});
        let mut bad = file.clone();
        bad["path"] = serde_json::json!("mods/b.jar");
        bad["hashes"] = serde_json::json!({});
        data["files"] = serde_json::json!([file, bad]);
        assert_eq!(
            inspect_archive(&mut archive(data, None))
                .unwrap_err()
                .code(),
            "download_hash_required"
        );
    }
    #[test]
    fn failed_database_transaction_can_restore_replaced_bytes() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.jar"), b"old").unwrap();
        let mut staged = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        staged.write_all(b"new").unwrap();
        {
            let mut tx = FileTransaction::new(root.path()).unwrap();
            tx.replace(Path::new("a.jar"), staged.path()).unwrap();
            assert_eq!(fs::read(root.path().join("a.jar")).unwrap(), b"new");
        }
        assert_eq!(fs::read(root.path().join("a.jar")).unwrap(), b"old");
    }
    #[test]
    fn a_file_transaction_cannot_replace_or_remove_a_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("world")).unwrap();
        fs::write(root.path().join("world/level.dat"), b"keep").unwrap();
        let staged = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        let mut transaction = FileTransaction::new(root.path()).unwrap();
        assert!(transaction
            .replace(Path::new("world"), staged.path())
            .is_err());
        assert!(transaction.remove(Path::new("world")).is_err());
        assert_eq!(
            fs::read(root.path().join("world/level.dat")).unwrap(),
            b"keep"
        );
    }
    #[test]
    fn rejects_missing_and_malformed_hashes() {
        assert!(validate_hashes(None, None).is_err());
        assert!(validate_hashes(Some(""), Some(&"0".repeat(40))).is_err());
        assert!(validate_hashes(Some(&"ab".repeat(64)), None).is_ok());
    }
    #[test]
    fn denies_drive_http_ip_and_host_suffix_tricks() {
        for url in [
            "https://drive.google.com/x",
            "http://cdn.modrinth.com/x",
            "https://cdn.modrinth.com.evil.test/x",
            "https://127.0.0.1/x",
            "https://user@cdn.modrinth.com/x",
            "https://cdn.modrinth.com:8443/x",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn windows_paths_are_checked_even_on_other_platforms() {
        for p in [
            "../x",
            "mods/../../x",
            "C:/x",
            "/x",
            "mods/a.jar:stream",
            "mods/NUL.jar",
            "mods/LPT¹",
            "mods/x.",
            "mods//x",
            "mods/x.part",
            "mods/./x",
        ] {
            assert!(relative_path(p).is_err(), "{p}");
        }
        assert!(relative_path("mods/Мой мод.jar").is_ok());
    }
    #[test]
    fn replacement_and_removal_roll_back() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.jar.disabled"), b"old").unwrap();
        {
            let mut tx = FileTransaction::new(root.path()).unwrap();
            tx.remove(Path::new("a.jar.disabled")).unwrap();
        }
        assert_eq!(
            fs::read(root.path().join("a.jar.disabled")).unwrap(),
            b"old"
        );
    }
}
