use crate::error::LauncherError;
use base64::Engine;
use futures_util::StreamExt;
use minisign_verify::{PublicKey, Signature};
use rand::Rng;
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

const RELEASES: &str = "https://api.github.com/repos/KvanderTech/ck-launcher/releases?per_page=30";
const PUBLIC_KEY: &str = "RWSPZXwqz2znhHAzYOMAoh7y49b4McVu4EgiO66/r1tQvuJgNG6w63M+";
const MAX_ARCHIVE: usize = 256 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    available: bool,
    version: String,
    notes: String,
    url: String,
    asset_url: String,
    signature_url: String,
}

fn client() -> Result<reqwest::Client, LauncherError> {
    reqwest::Client::builder()
        .user_agent("CKLauncher/native-updater")
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| failed())
}

pub async fn check_update() -> Result<UpdateInfo, LauncherError> {
    let response = client()?
        .get(RELEASES)
        .send()
        .await
        .map_err(|_| failed())?
        .error_for_status()
        .map_err(|_| failed())?;
    let body = bounded(response, 2 * 1024 * 1024).await?;
    let releases: serde_json::Value = serde_json::from_slice(&body).map_err(|_| failed())?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| failed())?;
    let channel = if cfg!(target_vendor = "win7") {
        "legacy"
    } else {
        "modern"
    };
    let archive_name = format!("ck-launcher-qt-{channel}-windows-x64.zip");
    let signature_name = format!("{archive_name}.sig");
    let mut selected = None;
    for release in releases.as_array().ok_or_else(failed)? {
        if release["draft"].as_bool() == Some(true) {
            continue;
        }
        let Some(tag) = release["tag_name"].as_str() else {
            continue;
        };
        let Ok(version) = semver::Version::parse(tag.trim_start_matches('v')) else {
            continue;
        };
        if version <= current || (current.pre.is_empty() && !version.pre.is_empty()) {
            continue;
        }
        let mut archive = None;
        let mut signature = None;
        for asset in release["assets"].as_array().into_iter().flatten() {
            let name = asset["name"].as_str().unwrap_or_default();
            let value = asset["browser_download_url"].as_str().unwrap_or_default();
            if name == archive_name {
                archive = Some(value.to_owned());
            }
            if name == signature_name {
                signature = Some(value.to_owned());
            }
        }
        let (Some(asset_url), Some(signature_url), Some(url)) =
            (archive, signature, release["html_url"].as_str())
        else {
            continue;
        };
        validate_release_url(url)?;
        validate_asset_url(&asset_url, tag, &archive_name)?;
        validate_asset_url(&signature_url, tag, &signature_name)?;
        if selected
            .as_ref()
            .is_none_or(|(v, _): &(semver::Version, UpdateInfo)| version > *v)
        {
            selected = Some((
                version.clone(),
                UpdateInfo {
                    available: true,
                    version: version.to_string(),
                    notes: release["body"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(8000)
                        .collect(),
                    url: url.to_owned(),
                    asset_url,
                    signature_url,
                },
            ));
        }
    }
    Ok(selected.map(|(_, value)| value).unwrap_or(UpdateInfo {
        available: false,
        version: current.to_string(),
        notes: String::new(),
        url: String::new(),
        asset_url: String::new(),
        signature_url: String::new(),
    }))
}

pub async fn install_update(
    asset_url: String,
    signature_url: String,
    install_dir: String,
    launcher_pid: u32,
) -> Result<bool, LauncherError> {
    let asset = url::Url::parse(&asset_url).map_err(|_| failed())?;
    let segments: Vec<_> = asset.path_segments().ok_or_else(failed)?.collect();
    if segments.len() != 6
        || segments[0..4] != ["KvanderTech", "ck-launcher", "releases", "download"]
    {
        return Err(failed());
    }
    let tag = segments[4];
    let name = segments[5..].join("/");
    validate_asset_url(&asset_url, tag, &name)?;
    validate_asset_url(&signature_url, tag, &format!("{name}.sig"))?;
    let http = client()?;
    let archive = bounded(
        http.get(&asset_url)
            .send()
            .await
            .map_err(|_| failed())?
            .error_for_status()
            .map_err(|_| failed())?,
        MAX_ARCHIVE,
    )
    .await?;
    let signature = bounded(
        http.get(&signature_url)
            .send()
            .await
            .map_err(|_| failed())?
            .error_for_status()
            .map_err(|_| failed())?,
        32 * 1024,
    )
    .await?;
    verify(&archive, &signature)?;
    let target = PathBuf::from(install_dir)
        .canonicalize()
        .map_err(|_| failed())?;
    let current = std::env::current_exe()
        .map_err(|_| failed())?
        .canonicalize()
        .map_err(|_| failed())?;
    if current.parent() != Some(target.as_path()) {
        return Err(failed());
    }
    let root = std::env::temp_dir().join(format!(
        "CKLauncherUpdate-{:016x}",
        rand::rng().random::<u64>()
    ));
    let extracted = root.join("package");
    fs::create_dir_all(&extracted).map_err(|_| failed())?;
    extract(&archive, &extracted)?;
    let updater_source = target.join("ck-launcher-updater.exe");
    if !updater_source.is_file() {
        return Err(failed());
    }
    let updater = root.join("ck-launcher-updater.exe");
    fs::copy(updater_source, &updater).map_err(|_| failed())?;
    Command::new(updater)
        .args([
            launcher_pid.to_string(),
            extracted.to_string_lossy().into_owned(),
            target.to_string_lossy().into_owned(),
        ])
        .spawn()
        .map_err(|_| failed())?;
    Ok(true)
}

async fn bounded(response: reqwest::Response, max: usize) -> Result<Vec<u8>, LauncherError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(failed());
    }
    let mut result = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| failed())?;
        if result.len() + chunk.len() > max {
            return Err(failed());
        }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}

fn verify(archive: &[u8], encoded: &[u8]) -> Result<(), LauncherError> {
    let text = std::str::from_utf8(encoded).map_err(|_| failed())?.trim();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(text)
        .ok()
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or_else(|| text.to_owned());
    let key = PublicKey::from_base64(PUBLIC_KEY).map_err(|_| failed())?;
    let signature = Signature::decode(&decoded).map_err(|_| failed())?;
    key.verify(archive, &signature, false).map_err(|_| failed())
}

fn extract(bytes: &[u8], destination: &Path) -> Result<(), LauncherError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| failed())?;
    if archive.len() > 4000 {
        return Err(failed());
    }
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| failed())?;
        total = total.checked_add(entry.size()).ok_or_else(failed)?;
        if total > 768 * 1024 * 1024 {
            return Err(failed());
        }
        let relative = entry.enclosed_name().ok_or_else(failed)?;
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|_| failed())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|_| failed())?;
        }
        let mut file = fs::File::create(output).map_err(|_| failed())?;
        std::io::copy(&mut entry, &mut file).map_err(|_| failed())?;
        file.flush().map_err(|_| failed())?;
    }
    if !destination.join("ck-launcher-qt.exe").is_file()
        || !destination.join("ck-launcher-service.exe").is_file()
    {
        return Err(failed());
    }
    Ok(())
}

fn failed() -> LauncherError {
    LauncherError::new(
        "update_failed",
        "Не удалось безопасно установить обновление.",
        None,
        true,
    )
}

pub fn validate_release_url(text: &str) -> Result<(), LauncherError> {
    let url = url::Url::parse(text).map_err(|_| failed())?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(failed());
    }
    let tag = url
        .path()
        .strip_prefix("/KvanderTech/ck-launcher/releases/tag/")
        .ok_or_else(failed)?;
    semver::Version::parse(tag.trim_start_matches('v')).map_err(|_| failed())?;
    Ok(())
}
fn validate_asset_url(text: &str, tag: &str, name: &str) -> Result<(), LauncherError> {
    let url = url::Url::parse(text).map_err(|_| failed())?;
    let expected = format!("/KvanderTech/ck-launcher/releases/download/{tag}/{name}");
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.path() != expected
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(failed());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confines_links() {
        assert!(validate_release_url(
            "https://github.com/KvanderTech/ck-launcher/releases/tag/v0.2.1-beta.1"
        )
        .is_ok());
        assert!(validate_asset_url(
            "https://github.com/KvanderTech/ck-launcher/releases/download/v0.2.1-beta.1/ck.zip",
            "v0.2.1-beta.1",
            "ck.zip"
        )
        .is_ok());
        assert!(validate_asset_url("https://evil.test/x", "v0.2.1-beta.1", "ck.zip").is_err());
    }

    #[test]
    fn accepts_only_archives_signed_by_the_native_release_key() {
        const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTUFpYd3F6MnpuaFBVN0g2OGhvekM4ZmdXRHJFQys1QXpCUy8vVC8zYlAwTzFYTFMxSk1wZVRPeFdTbURTVmFpb0NlOVN3WTlTTVZIZ3RwZzJSK3FSdDViS2ZIY3NRWXdzPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg4ODU0MDQ3CWZpbGU6bmF0aXZlLXVwZGF0ZS10ZXN0LnR4dAppZDdITFc0MVFlUkVyME5tMDBjSytxeWVvRHd6Y3hLUVNVSnpFMmZwT3RGcFlJK2FSS3VPc3BYZXV4UThHcFo2NkJHSTlPemhBWnI3dEdtVDc1eVlDdz09Cg==";
        let content = b"CK Launcher native updater signature test\n";
        assert!(verify(content, SIGNATURE.as_bytes()).is_ok());
        assert!(verify(b"tampered", SIGNATURE.as_bytes()).is_err());
    }
}
