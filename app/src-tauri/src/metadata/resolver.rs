use super::models::{GameVersionSummary, ResolvedVersion, VersionJson, VersionManifest};
use crate::{downloads::DownloadCancellationToken, error::LauncherError};
use async_trait::async_trait;
use reqwest::{header, Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha1::{Digest, Sha1};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(test)]
use std::sync::Mutex;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MAX_INHERITANCE_DEPTH: usize = 16;
static NEXT_TEMPORARY_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub enum HttpResponse {
    Ok { body: String, etag: Option<String> },
    NotModified,
}

#[async_trait]
pub trait MetadataHttp: Send + Sync {
    async fn get(&self, url: &str, etag: Option<&str>) -> Result<HttpResponse, LauncherError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub body: String,
    pub etag: Option<String>,
    pub verified: bool,
}

pub trait MetadataCache: Send + Sync {
    fn load(&self, key: &str) -> Result<Option<CacheEntry>, LauncherError>;
    fn save(&self, key: &str, entry: &CacheEntry) -> Result<(), LauncherError>;
}

pub struct HttpMetadataClient {
    client: Client,
}
impl HttpMetadataClient {
    pub fn new() -> Result<Self, LauncherError> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| LauncherError::metadata_unavailable())?,
        })
    }
}
#[async_trait]
impl MetadataHttp for HttpMetadataClient {
    async fn get(&self, url: &str, etag: Option<&str>) -> Result<HttpResponse, LauncherError> {
        let mut request = self.client.get(url);
        if let Some(etag) = etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let response = request
            .send()
            .await
            .map_err(|_| LauncherError::metadata_unavailable())?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(HttpResponse::NotModified);
        }
        if !response.status().is_success() {
            return Err(LauncherError::metadata_unavailable());
        }
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        Ok(HttpResponse::Ok {
            body: response
                .text()
                .await
                .map_err(|_| LauncherError::metadata_unavailable())?,
            etag,
        })
    }
}

pub struct FileMetadataCache {
    root: PathBuf,
}
impl FileMetadataCache {
    pub fn new(root: PathBuf) -> Result<Self, LauncherError> {
        fs::create_dir_all(&root).map_err(|_| LauncherError::storage_unavailable())?;
        Ok(Self { root })
    }
    fn path(&self, key: &str) -> PathBuf {
        self.root.join(format!(
            "{}.json",
            key.chars()
                .map(
                    |c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                )
                .collect::<String>()
        ))
    }

    fn temporary_path(&self, target: &std::path::Path) -> PathBuf {
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("metadata");
        self.root
            .join(format!(".{name}.{}-{sequence}.tmp", std::process::id()))
    }

    fn write_temporary(
        &self,
        target: &std::path::Path,
        body: &[u8],
    ) -> Result<PathBuf, LauncherError> {
        for _ in 0..16 {
            let temporary = self.temporary_path(target);
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(LauncherError::storage_unavailable()),
            };
            if file.write_all(body).and_then(|_| file.sync_all()).is_err() {
                drop(file);
                let _ = fs::remove_file(&temporary);
                return Err(LauncherError::storage_unavailable());
            }
            return Ok(temporary);
        }
        Err(LauncherError::storage_unavailable())
    }
}
impl MetadataCache for FileMetadataCache {
    fn load(&self, key: &str) -> Result<Option<CacheEntry>, LauncherError> {
        match fs::read_to_string(self.path(key)) {
            Ok(body) => Ok(serde_json::from_str(&body).ok()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(LauncherError::storage_unavailable()),
        }
    }
    fn save(&self, key: &str, entry: &CacheEntry) -> Result<(), LauncherError> {
        let target = self.path(key);
        let body = serde_json::to_vec(entry).map_err(|_| LauncherError::storage_unavailable())?;
        let temporary = self.write_temporary(&target, &body)?;
        if replace_existing(&temporary, &target).is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(LauncherError::storage_unavailable());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn replace_existing(temporary: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } != 0
    {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn replace_existing(temporary: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    fs::rename(temporary, target)
}

pub struct MetadataService {
    http: Arc<dyn MetadataHttp>,
    cache: Arc<dyn MetadataCache>,
}
impl MetadataService {
    pub fn new(http: Arc<dyn MetadataHttp>, cache: Arc<dyn MetadataCache>) -> Self {
        Self { http, cache }
    }
    pub fn production(cache_root: PathBuf) -> Result<Self, LauncherError> {
        Ok(Self::new(
            Arc::new(HttpMetadataClient::new()?),
            Arc::new(FileMetadataCache::new(cache_root)?),
        ))
    }
    #[cfg(test)]
    pub fn from_manifest_fixture(body: &str) -> Self {
        Self::new(
            Arc::new(FixtureHttp {
                body: body.to_owned(),
            }),
            Arc::new(MemoryCache::default()),
        )
    }

    pub async fn stable_releases(&self) -> Result<Vec<GameVersionSummary>, LauncherError> {
        let mut releases: Vec<_> = self
            .manifest(DownloadCancellationToken::new())
            .await?
            .versions
            .into_iter()
            .filter(|version| version.version_type == "release")
            .map(|version| GameVersionSummary {
                id: version.id,
                version_type: version.version_type,
                release_date: version.release_date,
            })
            .collect();
        releases.sort_by(|left, right| right.release_date.cmp(&left.release_date));
        Ok(releases)
    }
    pub async fn resolved_version(&self, id: &str) -> Result<ResolvedVersion, LauncherError> {
        self.resolved_version_cancellable(id, DownloadCancellationToken::new())
            .await
    }

    pub fn register_custom_version(&self, version: &VersionJson) -> Result<(), LauncherError> {
        if version.id.trim().is_empty() {
            return Err(LauncherError::metadata_invalid());
        }
        let body = serde_json::to_string(version).map_err(|_| LauncherError::metadata_invalid())?;
        self.cache.save(
            &format!("custom-version-{}", version.id),
            &CacheEntry {
                body,
                etag: None,
                verified: true,
            },
        )
    }
    pub(crate) async fn resolved_version_cancellable(
        &self,
        id: &str,
        cancel: DownloadCancellationToken,
    ) -> Result<ResolvedVersion, LauncherError> {
        self.resolve(id, 0, &mut HashSet::new(), cancel).await
    }
    async fn manifest(
        &self,
        cancel: DownloadCancellationToken,
    ) -> Result<VersionManifest, LauncherError> {
        self.document("manifest", MANIFEST_URL, None, cancel).await
    }
    async fn resolve(
        &self,
        id: &str,
        depth: usize,
        visiting: &mut HashSet<String>,
        cancel: DownloadCancellationToken,
    ) -> Result<ResolvedVersion, LauncherError> {
        ensure_not_cancelled(&cancel)?;
        if depth >= MAX_INHERITANCE_DEPTH || !visiting.insert(id.to_owned()) {
            return Err(LauncherError::metadata_invalid());
        }
        if let Some(entry) = self.cache.load(&format!("custom-version-{id}"))? {
            let child: VersionJson =
                serde_json::from_str(&entry.body).map_err(|_| LauncherError::metadata_invalid())?;
            let result = if let Some(parent_id) = child.inherits_from.clone() {
                let parent =
                    Box::pin(self.resolve(&parent_id, depth + 1, visiting, cancel)).await?;
                merge(parent, child)
            } else {
                into_resolved(child)
            };
            visiting.remove(id);
            return Ok(result);
        }
        let manifest = self.manifest(cancel.clone()).await?;
        let entry = manifest
            .versions
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(LauncherError::metadata_invalid)?;
        let child: VersionJson = self
            .document(
                &format!("version-{id}"),
                &entry.url,
                entry.sha1.as_deref(),
                cancel.clone(),
            )
            .await?;
        let result = if let Some(parent_id) = child.inherits_from.clone() {
            let parent = Box::pin(self.resolve(&parent_id, depth + 1, visiting, cancel)).await?;
            merge(parent, child)
        } else {
            into_resolved(child)
        };
        visiting.remove(id);
        Ok(result)
    }
    async fn document<T: DeserializeOwned>(
        &self,
        key: &str,
        url: &str,
        sha1: Option<&str>,
        cancel: DownloadCancellationToken,
    ) -> Result<T, LauncherError> {
        ensure_not_cancelled(&cancel)?;
        let cached = self.cache.load(key)?;
        let valid_cached = || {
            cached
                .as_ref()
                .filter(|entry| entry.verified && !entry.body.is_empty())
        };
        let request = self
            .http
            .get(url, valid_cached().and_then(|entry| entry.etag.as_deref()));
        let cancelled = cancel.cancelled();
        futures_util::pin_mut!(request, cancelled);
        let response = match futures_util::future::select(cancelled, request).await {
            futures_util::future::Either::Left(_) => return Err(cancelled_error()),
            futures_util::future::Either::Right((response, _)) => response,
        };
        match response {
            Ok(HttpResponse::NotModified) => valid_cached()
                .and_then(|entry| serde_json::from_str(&entry.body).ok())
                .ok_or_else(LauncherError::metadata_unavailable),
            Ok(HttpResponse::Ok { body, etag }) => {
                if let Some(expected) = sha1 {
                    if hex_sha1(&body) != expected.to_ascii_lowercase() {
                        return Err(LauncherError::metadata_invalid());
                    }
                }
                let decoded =
                    serde_json::from_str(&body).map_err(|_| LauncherError::metadata_invalid())?;
                self.cache.save(
                    key,
                    &CacheEntry {
                        body,
                        etag,
                        verified: true,
                    },
                )?;
                Ok(decoded)
            }
            Err(error) if error.code() == "download_cancelled" => Err(error),
            Err(_) => valid_cached()
                .and_then(|entry| serde_json::from_str(&entry.body).ok())
                .ok_or_else(LauncherError::metadata_unavailable),
        }
    }
}

fn ensure_not_cancelled(cancel: &DownloadCancellationToken) -> Result<(), LauncherError> {
    if cancel.is_cancelled() {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

fn cancelled_error() -> LauncherError {
    LauncherError::new(
        "download_cancelled",
        "The operation was cancelled.",
        None,
        true,
    )
}

fn hex_sha1(body: &str) -> String {
    format!("{:x}", Sha1::digest(body.as_bytes()))
}
fn into_resolved(version: VersionJson) -> ResolvedVersion {
    version.into()
}
fn merge(parent: ResolvedVersion, child: VersionJson) -> ResolvedVersion {
    let mut libraries: BTreeMap<String, _> = parent
        .libraries
        .into_iter()
        .map(|library| (library.name.clone(), library))
        .collect();
    for library in child.libraries {
        libraries.insert(library.name.clone(), library);
    }
    let mut arguments = parent.arguments;
    arguments.game.extend(child.arguments.game);
    arguments.jvm.extend(child.arguments.jvm);
    ResolvedVersion {
        id: child.id,
        main_class: child.main_class.or(parent.main_class),
        assets: child.assets.or(parent.assets),
        asset_index: child.asset_index.or(parent.asset_index),
        downloads: if child.downloads == Default::default() {
            parent.downloads
        } else {
            child.downloads
        },
        libraries: libraries.into_values().collect(),
        logging: child.logging.or(parent.logging),
        java_version: child.java_version.or(parent.java_version),
        arguments,
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
    }
}

#[cfg(test)]
struct FixtureHttp {
    body: String,
}
#[cfg(test)]
#[async_trait]
impl MetadataHttp for FixtureHttp {
    async fn get(&self, _url: &str, _etag: Option<&str>) -> Result<HttpResponse, LauncherError> {
        Ok(HttpResponse::Ok {
            body: self.body.clone(),
            etag: None,
        })
    }
}
#[cfg(test)]
#[derive(Default)]
struct MemoryCache(Mutex<BTreeMap<String, CacheEntry>>);
#[cfg(test)]
impl MetadataCache for MemoryCache {
    fn load(&self, key: &str) -> Result<Option<CacheEntry>, LauncherError> {
        Ok(self.0.lock().expect("cache lock").get(key).cloned())
    }
    fn save(&self, key: &str, entry: &CacheEntry) -> Result<(), LauncherError> {
        self.0
            .lock()
            .expect("cache lock")
            .insert(key.to_owned(), entry.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::models::Argument;

    struct MapHttp {
        responses: BTreeMap<String, Result<HttpResponse, LauncherError>>,
    }
    #[async_trait]
    impl MetadataHttp for MapHttp {
        async fn get(&self, url: &str, _etag: Option<&str>) -> Result<HttpResponse, LauncherError> {
            self.responses
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err(LauncherError::metadata_unavailable()))
        }
    }
    struct QueueHttp {
        responses: Mutex<Vec<Result<HttpResponse, LauncherError>>>,
    }
    #[async_trait]
    impl MetadataHttp for QueueHttp {
        async fn get(
            &self,
            _url: &str,
            _etag: Option<&str>,
        ) -> Result<HttpResponse, LauncherError> {
            self.responses.lock().expect("http lock").remove(0)
        }
    }

    fn manifest(entries: &[(&str, &str, Option<&str>)]) -> String {
        serde_json::json!({"versions": entries.iter().map(|(id, url, sha1)| serde_json::json!({"id": id, "type": "release", "url": url, "sha1": sha1, "releaseTime": "2026-01-01T00:00:00+00:00"})).collect::<Vec<_>>()}).to_string()
    }
    fn mapped_service(
        entries: &[(&str, &str, Option<&str>)],
        documents: &[(&str, &str)],
    ) -> MetadataService {
        let mut responses = BTreeMap::new();
        responses.insert(
            MANIFEST_URL.to_owned(),
            Ok(HttpResponse::Ok {
                body: manifest(entries),
                etag: None,
            }),
        );
        for (url, body) in documents {
            responses.insert(
                (*url).to_owned(),
                Ok(HttpResponse::Ok {
                    body: (*body).to_owned(),
                    etag: None,
                }),
            );
        }
        MetadataService::new(
            Arc::new(MapHttp { responses }),
            Arc::new(MemoryCache::default()),
        )
    }

    #[test]
    fn inheritance_uses_child_scalars_parent_first_arguments_and_replaced_libraries() {
        let parent = include_str!("../tests/fixtures/version_inherited.json");
        let child = include_str!("../tests/fixtures/version_child.json");
        let service = mapped_service(
            &[
                ("parent", "https://example.test/parent", None),
                ("child", "https://example.test/child", None),
            ],
            &[
                ("https://example.test/parent", parent),
                ("https://example.test/child", child),
            ],
        );
        let version = tauri::async_runtime::block_on(service.resolved_version("child"))
            .expect("child resolves");
        assert_eq!(version.main_class.as_deref(), Some("example.Child"));
        assert_eq!(
            version.minecraft_arguments.as_deref(),
            Some("--legacy-child")
        );
        assert_eq!(
            version.arguments.game,
            vec![
                Argument::Literal("--parent".to_owned()),
                Argument::Literal("--child".to_owned())
            ]
        );
        assert_eq!(
            version.arguments.jvm,
            vec![
                Argument::Literal("-Dparent=true".to_owned()),
                Argument::Literal("-Dchild=true".to_owned())
            ]
        );
        assert_eq!(
            version
                .libraries
                .iter()
                .filter(|library| library.name == "com.example:replace:1.0")
                .count(),
            1
        );
        assert!(version
            .libraries
            .iter()
            .find(|library| library.name == "com.example:replace:1.0")
            .expect("replacement")
            .natives
            .is_some());
    }

    #[test]
    fn cyclic_or_excessive_parent_chains_return_the_stable_metadata_error() {
        let cyclic = mapped_service(
            &[
                ("one", "https://example.test/one", None),
                ("two", "https://example.test/two", None),
            ],
            &[
                (
                    "https://example.test/one",
                    r#"{"id":"one","inheritsFrom":"two"}"#,
                ),
                (
                    "https://example.test/two",
                    r#"{"id":"two","inheritsFrom":"one"}"#,
                ),
            ],
        );
        let cyclic_error = tauri::async_runtime::block_on(cyclic.resolved_version("one"))
            .expect_err("cycle is rejected");
        assert_eq!(cyclic_error.code(), "metadata_invalid");
        let mut entries = Vec::new();
        let mut documents = Vec::new();
        for index in 0..=16 {
            let id = format!("v{index}");
            let url = format!("https://example.test/{id}");
            entries.push((id, url));
        }
        for (index, (_, url)) in entries.iter().enumerate() {
            let parent = if index == 16 {
                String::new()
            } else {
                format!(",\"inheritsFrom\":\"v{}\"", index + 1)
            };
            let body = format!(r#"{{"id":"v{index}"{parent}}}"#);
            documents.push((url.as_str(), body));
        }
        let tuples: Vec<_> = entries
            .iter()
            .map(|(id, url)| (id.as_str(), url.as_str(), None))
            .collect();
        let documents: Vec<_> = documents
            .iter()
            .map(|(url, body)| (*url, body.as_str()))
            .collect();
        let deep = mapped_service(&tuples, &documents);
        let deep_error = tauri::async_runtime::block_on(deep.resolved_version("v0"))
            .expect_err("deep chain is rejected");
        assert_eq!(deep_error.code(), "metadata_invalid");
    }

    #[test]
    fn etag_and_verified_last_known_good_control_cache_fallback() {
        let body = include_str!("../tests/fixtures/version_manifest_v2.json").to_owned();
        let service = MetadataService::new(
            Arc::new(QueueHttp {
                responses: Mutex::new(vec![
                    Ok(HttpResponse::Ok {
                        body: body.clone(),
                        etag: Some("manifest-v1".to_owned()),
                    }),
                    Ok(HttpResponse::NotModified),
                ]),
            }),
            Arc::new(MemoryCache::default()),
        );
        let first =
            tauri::async_runtime::block_on(service.stable_releases()).expect("network manifest");
        let second =
            tauri::async_runtime::block_on(service.stable_releases()).expect("304 cache manifest");
        assert_eq!(first, second);
        let cache = Arc::new(MemoryCache::default());
        cache
            .save(
                "manifest",
                &CacheEntry {
                    body,
                    etag: None,
                    verified: true,
                },
            )
            .expect("cache saves");
        let offline = MetadataService::new(
            Arc::new(QueueHttp {
                responses: Mutex::new(vec![Err(LauncherError::metadata_unavailable())]),
            }),
            cache,
        );
        assert!(!tauri::async_runtime::block_on(offline.stable_releases())
            .expect("verified cache fallback")
            .is_empty());
        let unverified = Arc::new(MemoryCache::default());
        unverified
            .save(
                "manifest",
                &CacheEntry {
                    body: "{}".to_owned(),
                    etag: None,
                    verified: false,
                },
            )
            .expect("cache saves");
        let rejected = MetadataService::new(
            Arc::new(QueueHttp {
                responses: Mutex::new(vec![Err(LauncherError::metadata_unavailable())]),
            }),
            unverified,
        );
        let error = tauri::async_runtime::block_on(rejected.stable_releases())
            .expect_err("unverified cache is rejected");
        assert_eq!(error.code(), "metadata_unavailable");
        let corrupt = Arc::new(MemoryCache::default());
        corrupt
            .save(
                "manifest",
                &CacheEntry {
                    body: "not json".to_owned(),
                    etag: None,
                    verified: true,
                },
            )
            .expect("cache saves");
        let corrupt_service = MetadataService::new(
            Arc::new(QueueHttp {
                responses: Mutex::new(vec![Err(LauncherError::metadata_unavailable())]),
            }),
            corrupt,
        );
        let error = tauri::async_runtime::block_on(corrupt_service.stable_releases())
            .expect_err("corrupt cache is rejected");
        assert_eq!(error.code(), "metadata_unavailable");
    }

    #[test]
    fn version_json_hash_mismatch_is_rejected_before_caching() {
        let body = r#"{"id":"one"}"#;
        let service = mapped_service(
            &[(
                "one",
                "https://example.test/one",
                Some("0000000000000000000000000000000000000000"),
            )],
            &[("https://example.test/one", body)],
        );
        let error = tauri::async_runtime::block_on(service.resolved_version("one"))
            .expect_err("wrong hash is rejected");
        assert_eq!(error.code(), "metadata_invalid");
    }

    #[test]
    fn file_cache_replaces_an_existing_verified_entry_after_a_second_refresh() {
        let unique = format!(
            "ck-launcher-metadata-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let cache = FileMetadataCache::new(root.clone()).expect("cache directory is created");
        cache
            .save(
                "manifest",
                &CacheEntry {
                    body: "first verified manifest".to_owned(),
                    etag: Some("etag-one".to_owned()),
                    verified: true,
                },
            )
            .expect("first cache entry saves");
        std::fs::create_dir(root.join("manifest.tmp"))
            .expect("the legacy fixed temporary name is unavailable");

        cache
            .save(
                "manifest",
                &CacheEntry {
                    body: "second verified manifest".to_owned(),
                    etag: Some("etag-two".to_owned()),
                    verified: true,
                },
            )
            .expect("second cache refresh replaces the first entry");

        let entry = cache
            .load("manifest")
            .expect("cache reads")
            .expect("cache entry remains");
        assert_eq!(entry.body, "second verified manifest");
        assert_eq!(entry.etag.as_deref(), Some("etag-two"));
        assert!(entry.verified);
        std::fs::remove_dir_all(root).expect("cache test directory is removed");
    }
}
