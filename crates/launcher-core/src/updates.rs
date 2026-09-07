//! Release discovery is separated from installation. The native UI opens an exact
//! repository release page; it never downloads or executes an unsigned installer.
use crate::error::LauncherError;
use futures_util::StreamExt;
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    available: bool,
    version: String,
    notes: String,
    url: String,
}
pub async fn check_update() -> Result<UpdateInfo, LauncherError> {
    let client = reqwest::Client::builder()
        .user_agent("CKLauncher/native-update-check")
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| failed())?;
    let response = client
        .get("https://api.github.com/repos/KvanderTech/ck-launcher/releases?per_page=30")
        .send()
        .await
        .map_err(|_| failed())?
        .error_for_status()
        .map_err(|_| failed())?;
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| failed())?;
        if body.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(failed());
        }
        body.extend_from_slice(&chunk);
    }
    let releases: serde_json::Value = serde_json::from_slice(&body).map_err(|_| failed())?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| failed())?;
    let channel = if cfg!(target_vendor = "win7") {
        "legacy"
    } else {
        "modern"
    };
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
        if version <= current
            || (!current.pre.is_empty() && version.pre.is_empty() && version < current)
            || (current.pre.is_empty() && !version.pre.is_empty())
        {
            continue;
        }
        let matching = release["assets"].as_array().is_some_and(|a| {
            a.iter().any(|a| {
                a["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("ck-launcher-qt-") && n.contains(channel))
            })
        });
        if !matching {
            continue;
        }
        let Some(url) = release["html_url"].as_str() else {
            continue;
        };
        if validate_release_url(url).is_err() {
            continue;
        }
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
                },
            ));
        }
    }
    Ok(selected.map(|(_, v)| v).unwrap_or(UpdateInfo {
        available: false,
        version: current.to_string(),
        notes: String::new(),
        url: String::new(),
    }))
}
fn failed() -> LauncherError {
    LauncherError::new(
        "update_check_failed",
        "Не удалось проверить обновления. Попробуйте позже.",
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
    let prefix = "/KvanderTech/ck-launcher/releases/tag/";
    let tag = url.path().strip_prefix(prefix).ok_or_else(failed)?;
    if semver::Version::parse(tag.trim_start_matches('v')).is_err() {
        return Err(failed());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confines_release_links_to_this_repo() {
        assert!(validate_release_url(
            "https://github.com/KvanderTech/ck-launcher/releases/tag/v0.2.0-beta.1"
        )
        .is_ok());
        for url in [
            "http://github.com/KvanderTech/ck-launcher/releases/tag/v1.0.0",
            "https://github.com/evil/repo/releases/tag/v1.0.0",
            "https://github.com/KvanderTech/ck-launcher/releases/tag/v1.0.0?redirect=x",
        ] {
            assert!(validate_release_url(url).is_err());
        }
    }
}
