//! Public UI images use the same Rustls transport on modern and legacy Windows.
//! No Qt/OpenSSL DLL, credentials, filesystem paths or arbitrary destinations are accepted.
use crate::error::LauncherError;
use base64::Engine;
use futures_util::StreamExt;
use std::{sync::OnceLock, time::Duration};
use url::Url;
const MAX_IMAGE: usize = 2 * 1024 * 1024;
const HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "textures.minecraft.net",
    "mc-heads.net",
    "minecraft.net",
    "www.minecraft.net",
];
fn denied() -> LauncherError {
    LauncherError::new(
        "image_source_denied",
        "Источник изображения не разрешён.",
        None,
        false,
    )
}
fn unavailable() -> LauncherError {
    LauncherError::new(
        "image_unavailable",
        "Не удалось загрузить изображение.",
        None,
        true,
    )
}
fn allowed(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none_or(|p| p == 443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && HOSTS.contains(&url.host_str().unwrap_or(""))
}
fn image_url(value: &str) -> Result<Url, LauncherError> {
    if value.len() > 2048 {
        return Err(denied());
    }
    let mut url = Url::parse(value).map_err(|_| denied())?;
    if url.scheme() == "http" && url.port().is_none_or(|p| p == 80) {
        url.set_scheme("https").map_err(|_| denied())?;
        url.set_port(None).map_err(|_| denied())?;
    }
    if !allowed(&url) {
        return Err(denied());
    }
    Ok(url)
}
fn encode_image(bytes: &[u8]) -> Result<String, LauncherError> {
    let raster = bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"));
    if bytes.len() > MAX_IMAGE || !raster {
        return Err(unavailable());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
pub async fn load_public_image(value: String) -> Result<String, LauncherError> {
    static CLIENT: OnceLock<Result<reqwest::Client, reqwest::Error>> = OnceLock::new();
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(6);
    let url = image_url(&value)?;
    let _slot = SLOTS.acquire().await.map_err(|_| unavailable())?;
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12))
                .pool_max_idle_per_host(2)
                .user_agent(concat!("CKLauncher/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 4 || !allowed(attempt.url()) {
                        attempt.error("Image redirect denied")
                    } else {
                        attempt.follow()
                    }
                }))
                .build()
        })
        .as_ref()
        .map_err(|_| unavailable())?;
    let response = client.get(url).send().await.map_err(|_| unavailable())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|n| n > MAX_IMAGE as u64)
    {
        return Err(unavailable());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| unavailable())?;
        if bytes.len().saturating_add(chunk.len()) > MAX_IMAGE {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    encode_image(&bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minecraft_http_urls_are_upgraded_before_any_request() {
        assert_eq!(
            image_url("http://textures.minecraft.net/texture/abc")
                .unwrap()
                .as_str(),
            "https://textures.minecraft.net/texture/abc"
        );
        assert!(image_url("https://cdn.modrinth.com/data/icon.png").is_ok());
    }
    #[test]
    fn image_sources_cannot_read_local_or_unrelated_resources() {
        for value in [
            "file:///C:/secret.png",
            "http://127.0.0.1/icon.png",
            "https://cdn.modrinth.com.attacker.test/a",
            "https://user:password@cdn.modrinth.com/a",
            "https://textures.minecraft.net:8080/a",
            "https://mc-heads.net/a#secret",
        ] {
            assert!(image_url(value).is_err(), "{value}");
        }
    }
    #[test]
    fn redirects_cannot_downgrade_tls_or_escape_the_host_policy() {
        assert!(!allowed(
            &Url::parse("http://textures.minecraft.net/texture/abc").unwrap()
        ));
        assert!(!allowed(
            &Url::parse("https://example.test/image.png").unwrap()
        ));
        assert!(allowed(
            &Url::parse("https://cdn.modrinth.com/image.png").unwrap()
        ));
    }
    #[test]
    fn only_bounded_raster_payloads_cross_the_ui_transport() {
        assert!(encode_image(b"<svg onload='alert(1)'></svg>").is_err());
        assert!(encode_image(b"<html>Error</html>").is_err());
        let mut oversized = b"\x89PNG\r\n\x1a\n".to_vec();
        oversized.resize(MAX_IMAGE + 1, 0);
        assert!(encode_image(&oversized).is_err());
        assert_eq!(encode_image(b"GIF89a").unwrap(), "R0lGODlh");
    }
}
