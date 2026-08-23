#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::{
        verify_archive_checksum, RuntimeArchiveEntry, RuntimeArchiveFetcher,
        RuntimeArchiveManifest, RuntimeInstaller,
    };
    use crate::{
        error::LauncherError,
        runtime::{JavaRequirement, ProcessOutput, ProcessRunner},
    };
    use async_trait::async_trait;
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        io::{Cursor, Write},
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };
    use zip::{write::SimpleFileOptions, ZipWriter};

    struct FakeFetcher(Vec<u8>);
    #[async_trait]
    impl RuntimeArchiveFetcher for FakeFetcher {
        async fn fetch(&self, _url: &str, _max_bytes: usize) -> Result<Vec<u8>, LauncherError> {
            Ok(self.0.clone())
        }
    }
    struct FixedRunner;
    #[async_trait]
    impl ProcessRunner for FixedRunner {
        async fn run(
            &self,
            _executable: &Path,
            args: &[&str],
            timeout: Duration,
        ) -> Result<ProcessOutput, LauncherError> {
            assert_eq!(args, ["-version"]);
            assert_eq!(timeout, Duration::from_secs(5));
            Ok(ProcessOutput {
                success: true,
                stdout: String::new(),
                stderr: "openjdk version \"17.0.20\"".to_owned(),
            })
        }
    }
    struct InvalidRunner;
    #[async_trait]
    impl ProcessRunner for InvalidRunner {
        async fn run(
            &self,
            _executable: &Path,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<ProcessOutput, LauncherError> {
            Ok(ProcessOutput {
                success: false,
                stdout: String::new(),
                stderr: "openjdk version \"17\"".to_owned(),
            })
        }
    }

    fn runtime_zip() -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file("jdk-17/bin/java.exe", SimpleFileOptions::default())
            .expect("entry");
        archive.write_all(b"fake java").expect("contents");
        archive.finish().expect("zip").into_inner()
    }

    fn manifest(sha256: String) -> RuntimeArchiveManifest {
        RuntimeArchiveManifest {
            schema_version: 1,
            windows_x64: BTreeMap::from([(
                17,
                RuntimeArchiveEntry {
                    url: "https://example.test/java.zip".to_owned(),
                    sha256,
                },
            )]),
        }
    }

    fn temporary_root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ck-runtime-install-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).expect("temporary root");
        root
    }

    #[test]
    fn versioned_manifest_requires_windows_x64_url_and_sha_for_every_supported_major() {
        let manifest = RuntimeArchiveManifest::bundled().expect("bundled manifest is valid");
        assert_eq!(manifest.schema_version(), 1);
        for major in [8, 17, 21, 25] {
            let entry = manifest.entry(major).expect("supported Java entry");
            assert!(entry.url.starts_with("https://"));
            assert_eq!(entry.sha256.len(), 64);
        }
    }

    #[test]
    fn checksum_mismatch_is_rejected_before_archive_extraction() {
        let error = verify_archive_checksum(b"downloaded bytes", &"0".repeat(64))
            .expect_err("mismatched checksum is rejected");
        assert_eq!(error.code(), "runtime_checksum_mismatch");
    }

    #[test]
    fn checksum_failure_preserves_the_previous_valid_runtime() {
        tauri::async_runtime::block_on(async {
            let root = temporary_root();
            let previous = root.join("java-17");
            std::fs::create_dir_all(previous.join("bin")).expect("previous runtime");
            std::fs::write(previous.join("marker"), b"previous").expect("previous marker");
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(FixedRunner),
                Arc::new(FakeFetcher(runtime_zip())),
                manifest("0".repeat(64)),
            );
            let error = installer
                .install(JavaRequirement::new(17).unwrap())
                .await
                .expect_err("checksum mismatch");
            assert_eq!(error.code(), "runtime_checksum_mismatch");
            assert_eq!(
                std::fs::read(previous.join("marker")).expect("previous retained"),
                b"previous"
            );
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[test]
    fn verified_archive_is_probed_before_replacing_the_previous_runtime() {
        tauri::async_runtime::block_on(async {
            let root = temporary_root();
            let previous = root.join("java-17");
            std::fs::create_dir_all(previous.join("bin")).expect("previous runtime");
            std::fs::write(previous.join("marker"), b"previous").expect("previous marker");
            let bytes = runtime_zip();
            let sha = format!("{:x}", Sha256::digest(&bytes));
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(FixedRunner),
                Arc::new(FakeFetcher(bytes)),
                manifest(sha),
            );
            let status = installer
                .install(JavaRequirement::new(17).unwrap())
                .await
                .expect("runtime installs");
            assert_eq!(
                status.path.as_deref(),
                Some(root.join("java-17/bin/java.exe").as_path())
            );
            assert!(!previous.join("marker").exists());
            assert_eq!(
                std::fs::read(previous.join("bin/java.exe")).expect("new java"),
                b"fake java"
            );
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[test]
    fn failed_staged_java_probe_preserves_the_previous_valid_runtime() {
        tauri::async_runtime::block_on(async {
            let root = temporary_root();
            let previous = root.join("java-17");
            std::fs::create_dir_all(previous.join("bin")).expect("previous runtime");
            std::fs::write(previous.join("marker"), b"previous").expect("previous marker");
            let bytes = runtime_zip();
            let sha = format!("{:x}", Sha256::digest(&bytes));
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(InvalidRunner),
                Arc::new(FakeFetcher(bytes)),
                manifest(sha),
            );
            assert_eq!(
                installer
                    .install(JavaRequirement::new(17).unwrap())
                    .await
                    .unwrap_err()
                    .code(),
                "java_runtime_invalid"
            );
            assert_eq!(
                std::fs::read(previous.join("marker")).expect("previous retained"),
                b"previous"
            );
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }
}
use super::{
    archive::extract_zip_archive, detect::probe_java, java_executable, JavaRequirement,
    JavaRuntimeSource, JavaRuntimeState, JavaRuntimeStatus, ProcessRunner,
};
use crate::{error::LauncherError, paths::AppPaths};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const MAX_RUNTIME_ARCHIVE_BYTES: usize = 536_870_912;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeArchiveManifest {
    schema_version: u32,
    windows_x64: BTreeMap<u16, RuntimeArchiveEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeArchiveEntry {
    pub url: String,
    pub sha256: String,
}

impl RuntimeArchiveManifest {
    pub fn bundled() -> Result<Self, LauncherError> {
        let manifest: Self = serde_json::from_str(include_str!("runtime-manifest-v1.json"))
            .map_err(|_| invalid_manifest())?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn entry(&self, major: u16) -> Option<&RuntimeArchiveEntry> {
        self.windows_x64.get(&major)
    }

    fn validate(&self) -> Result<(), LauncherError> {
        if self.schema_version != 1 {
            return Err(invalid_manifest());
        }
        for major in super::SUPPORTED_JAVA_MAJORS {
            let entry = self.entry(major).ok_or_else(invalid_manifest)?;
            if !entry.url.starts_with("https://")
                || entry.sha256.len() != 64
                || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(invalid_manifest());
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait RuntimeArchiveFetcher: Send + Sync {
    async fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, LauncherError>;
}

pub struct BoundedReqwestRuntimeArchiveFetcher {
    client: reqwest::Client,
}

impl BoundedReqwestRuntimeArchiveFetcher {
    pub fn new() -> Result<Self, LauncherError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|_| fetch_error())?;
        Ok(Self { client })
    }
}

#[async_trait]
impl RuntimeArchiveFetcher for BoundedReqwestRuntimeArchiveFetcher {
    async fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, LauncherError> {
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| fetch_error())?
            .error_for_status()
            .map_err(|_| fetch_error())?;
        if response
            .content_length()
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err(fetch_error());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| fetch_error())? {
            let next = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or_else(fetch_error)?;
            if next > max_bytes {
                return Err(fetch_error());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

pub struct RuntimeInstaller {
    runtime_root: PathBuf,
    runner: Arc<dyn ProcessRunner>,
    fetcher: Arc<dyn RuntimeArchiveFetcher>,
    manifest: RuntimeArchiveManifest,
}

impl RuntimeInstaller {
    pub fn new(
        runtime_root: PathBuf,
        runner: Arc<dyn ProcessRunner>,
        fetcher: Arc<dyn RuntimeArchiveFetcher>,
        manifest: RuntimeArchiveManifest,
    ) -> Self {
        Self {
            runtime_root,
            runner,
            fetcher,
            manifest,
        }
    }

    pub async fn install(
        &self,
        requirement: JavaRequirement,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        let entry = self
            .manifest
            .entry(requirement.major())
            .ok_or_else(invalid_manifest)?
            .clone();
        let bytes = self
            .fetcher
            .fetch(&entry.url, MAX_RUNTIME_ARCHIVE_BYTES)
            .await?;
        verify_archive_checksum(&bytes, &entry.sha256)?;
        fs::create_dir_all(&self.runtime_root).map_err(|_| install_error())?;
        let nonce = rand::random::<u64>();
        let temp_name = format!(".install-java-{}-{nonce}", requirement.major());
        let temp = safe_child(&self.runtime_root, &temp_name)?;
        fs::create_dir(&temp).map_err(|_| install_error())?;

        let extraction = extract_zip_archive(&bytes, &temp);
        if let Err(error) = extraction {
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }
        let Some(java) = find_java_executable(&temp) else {
            let _ = fs::remove_dir_all(&temp);
            return Err(install_error());
        };
        let (major, version) = match probe_java(self.runner.as_ref(), &java).await {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_dir_all(&temp);
                return Err(error);
            }
        };
        if major != requirement.major() {
            let _ = fs::remove_dir_all(&temp);
            return Err(install_error());
        }
        let java_home = java
            .parent()
            .and_then(Path::parent)
            .ok_or_else(install_error)?
            .to_path_buf();
        let candidate_name = format!(".candidate-java-{}-{nonce}", requirement.major());
        let candidate = safe_child(&self.runtime_root, &candidate_name)?;
        let staged = if java_home == temp {
            fs::rename(&temp, &candidate)
        } else {
            fs::rename(&java_home, &candidate)
        };
        if staged.is_err() {
            let _ = fs::remove_dir_all(&temp);
            return Err(install_error());
        }
        if let Err(error) =
            replace_runtime(&self.runtime_root, requirement.major(), &candidate, nonce)
        {
            let _ = remove_path(&candidate);
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }
        if temp.exists() {
            let _ = fs::remove_dir_all(&temp);
        }
        let final_java = java_executable(
            &self
                .runtime_root
                .join(format!("java-{}", requirement.major())),
        );
        Ok(JavaRuntimeStatus {
            requirement: requirement.major(),
            state: JavaRuntimeState::Valid,
            path: Some(final_java),
            source: Some(JavaRuntimeSource::Managed),
            version: Some(version),
        })
    }
}

pub fn verify_archive_checksum(bytes: &[u8], expected: &str) -> Result<(), LauncherError> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(LauncherError::new(
            "runtime_checksum_mismatch",
            "The downloaded Java runtime failed its integrity check.",
            None,
            true,
        ))
    }
}

fn replace_runtime(
    root: &Path,
    major: u16,
    candidate: &Path,
    nonce: u64,
) -> Result<(), LauncherError> {
    let final_name = format!("java-{major}");
    let final_path = safe_child(root, &final_name)?;
    let backup = safe_child(root, &format!(".backup-java-{major}-{nonce}"))?;
    let had_previous = final_path.exists();
    if had_previous {
        fs::rename(&final_path, &backup).map_err(|_| install_error())?;
    }
    let final_path = safe_child(root, &final_name)?;
    if let Err(error) = fs::rename(candidate, &final_path) {
        if had_previous {
            let _ = fs::rename(&backup, &final_path);
        }
        return Err(LauncherError::new(
            "runtime_install_failed",
            "The Java runtime could not be installed.",
            Some(error.to_string()),
            true,
        ));
    }
    if had_previous {
        let _ = remove_path(&backup);
    }
    Ok(())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn find_java_executable(root: &Path) -> Option<PathBuf> {
    let direct = java_executable(root);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().ok()?.is_dir() {
            if let Some(found) = find_java_executable(&path) {
                return Some(found);
            }
        }
    }
    None
}

fn safe_child(root: &Path, name: &str) -> Result<PathBuf, LauncherError> {
    AppPaths::new(root.to_path_buf()).safe_join(root, Path::new(name))
}

fn invalid_manifest() -> LauncherError {
    LauncherError::new(
        "runtime_manifest_invalid",
        "The managed Java runtime manifest is invalid.",
        None,
        false,
    )
}
fn fetch_error() -> LauncherError {
    LauncherError::new(
        "runtime_download_failed",
        "The Java runtime could not be downloaded.",
        None,
        true,
    )
}
fn install_error() -> LauncherError {
    LauncherError::new(
        "runtime_install_failed",
        "The Java runtime could not be installed.",
        None,
        true,
    )
}
