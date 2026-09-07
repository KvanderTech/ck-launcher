#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::{
        recover_interrupted_swaps_on_startup, verify_archive_checksum, RuntimeArchiveEntry,
        RuntimeArchiveFetcher, RuntimeArchiveManifest, RuntimeInstaller, RuntimeSwapFileSystem,
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
        async fn fetch(
            &self,
            _url: &str,
            _max_bytes: usize,
            _cancel: &crate::downloads::DownloadCancellationToken,
        ) -> Result<Vec<u8>, LauncherError> {
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
    struct FileContentRunner;
    #[async_trait]
    impl ProcessRunner for FileContentRunner {
        async fn run(
            &self,
            executable: &Path,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<ProcessOutput, LauncherError> {
            let valid = std::fs::read(executable).is_ok_and(|bytes| bytes == b"old java");
            Ok(ProcessOutput {
                success: valid,
                stdout: String::new(),
                stderr: "openjdk version \"17.0.20\"".to_owned(),
            })
        }
    }

    struct FailingSwapFileSystem {
        fail_activation: bool,
        fail_rollback: bool,
    }

    impl RuntimeSwapFileSystem for FailingSwapFileSystem {
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            let from_name = from
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if self.fail_activation && from_name.starts_with(".candidate-java-") {
                return Err(std::io::Error::other("injected activation failure"));
            }
            if self.fail_rollback && from_name == ".backup-java-17" {
                return Err(std::io::Error::other("injected rollback failure"));
            }
            std::fs::rename(from, to)
        }

        fn remove_path(&self, path: &Path) -> std::io::Result<()> {
            super::remove_path(path)
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

    fn create_probe_valid_old_runtime(root: &Path) {
        let previous = root.join("java-17");
        std::fs::create_dir_all(previous.join("bin")).expect("previous runtime");
        std::fs::write(previous.join("bin/java.exe"), b"old java").expect("old Java executable");
        std::fs::write(previous.join("marker"), b"previous").expect("previous marker");
    }

    #[test]
    fn versioned_manifest_requires_windows_x64_url_and_sha_for_every_supported_major() {
        let manifest = RuntimeArchiveManifest::bundled().expect("bundled manifest is valid");
        assert_eq!(manifest.schema_version(), 1);
        for major in [8, 16, 17, 21, 25] {
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
        crate::tasks::block_on(async {
            let root = temporary_root();
            create_probe_valid_old_runtime(&root);
            let previous = root.join("java-17");
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
        crate::tasks::block_on(async {
            let root = temporary_root();
            create_probe_valid_old_runtime(&root);
            let previous = root.join("java-17");
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
        crate::tasks::block_on(async {
            let root = temporary_root();
            create_probe_valid_old_runtime(&root);
            let previous = root.join("java-17");
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

    #[test]
    fn activation_and_rollback_failure_returns_inconsistency_and_retains_discoverable_backup() {
        crate::tasks::block_on(async {
            let root = temporary_root();
            create_probe_valid_old_runtime(&root);
            let bytes = runtime_zip();
            let sha = format!("{:x}", Sha256::digest(&bytes));
            let installer = RuntimeInstaller::with_swap_file_system(
                root.clone(),
                Arc::new(FixedRunner),
                Arc::new(FakeFetcher(bytes)),
                manifest(sha),
                Arc::new(FailingSwapFileSystem {
                    fail_activation: true,
                    fail_rollback: true,
                }),
            );

            let error = installer
                .install(JavaRequirement::new(17).unwrap())
                .await
                .expect_err("failed rollback is surfaced");

            assert_eq!(error.code(), "runtime_state_inconsistent");
            assert!(root.join(".backup-java-17/bin/java.exe").is_file());
            assert!(!root.join("java-17").exists());
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[test]
    fn activation_failure_with_successful_rollback_restores_probe_valid_old_runtime() {
        crate::tasks::block_on(async {
            let root = temporary_root();
            create_probe_valid_old_runtime(&root);
            let bytes = runtime_zip();
            let sha = format!("{:x}", Sha256::digest(&bytes));
            let installer = RuntimeInstaller::with_swap_file_system(
                root.clone(),
                Arc::new(FixedRunner),
                Arc::new(FakeFetcher(bytes)),
                manifest(sha),
                Arc::new(FailingSwapFileSystem {
                    fail_activation: true,
                    fail_rollback: false,
                }),
            );

            let error = installer
                .install(JavaRequirement::new(17).unwrap())
                .await
                .expect_err("activation failure is returned");

            assert_eq!(error.code(), "runtime_install_failed");
            assert_eq!(
                std::fs::read(root.join("java-17/bin/java.exe")).expect("old Java restored"),
                b"old java"
            );
            assert!(!root.join(".backup-java-17").exists());
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[test]
    fn startup_and_preinstall_recovery_restore_a_probe_valid_backup() {
        crate::tasks::block_on(async {
            let root = temporary_root();
            let backup = root.join(".backup-java-17");
            std::fs::create_dir_all(backup.join("bin")).expect("backup runtime");
            std::fs::write(backup.join("bin/java.exe"), b"old java").expect("old Java executable");

            recover_interrupted_swaps_on_startup(&root)
                .expect("startup recovery restores missing final");
            assert!(root.join("java-17/bin/java.exe").is_file());
            assert!(!backup.exists());

            std::fs::rename(root.join("java-17"), &backup)
                .expect("simulate second interrupted swap");
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(FixedRunner),
                Arc::new(FakeFetcher(Vec::new())),
                manifest("0".repeat(64)),
            );
            installer
                .recover_interrupted_swap(JavaRequirement::new(17).unwrap())
                .await
                .expect("pre-install recovery probes restored backup");
            assert!(root.join("java-17/bin/java.exe").is_file());
            assert!(!backup.exists());
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[test]
    fn preinstall_recovery_keeps_a_valid_activated_runtime_and_removes_its_backup() {
        crate::tasks::block_on(async {
            let root = temporary_root();
            create_probe_valid_old_runtime(&root);
            let backup = root.join(".backup-java-17");
            std::fs::create_dir_all(backup.join("bin")).expect("backup runtime");
            std::fs::write(backup.join("bin/java.exe"), b"older java")
                .expect("backup Java executable");
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(FixedRunner),
                Arc::new(FakeFetcher(Vec::new())),
                manifest("0".repeat(64)),
            );

            installer
                .recover_interrupted_swap(JavaRequirement::new(17).unwrap())
                .await
                .expect("activated runtime wins after probe");

            assert_eq!(
                std::fs::read(root.join("java-17/bin/java.exe")).expect("activated Java retained"),
                b"old java"
            );
            assert!(!backup.exists());
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[test]
    fn preinstall_recovery_replaces_an_invalid_activated_runtime_with_probe_valid_backup() {
        crate::tasks::block_on(async {
            let root = temporary_root();
            std::fs::create_dir_all(root.join("java-17/bin")).expect("broken final runtime");
            std::fs::write(root.join("java-17/bin/java.exe"), b"broken java")
                .expect("broken Java executable");
            let backup = root.join(".backup-java-17");
            std::fs::create_dir_all(backup.join("bin")).expect("backup runtime");
            std::fs::write(backup.join("bin/java.exe"), b"old java")
                .expect("valid backup Java executable");
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(FileContentRunner),
                Arc::new(FakeFetcher(Vec::new())),
                manifest("0".repeat(64)),
            );

            installer
                .recover_interrupted_swap(JavaRequirement::new(17).unwrap())
                .await
                .expect("valid backup replaces broken activation");

            assert_eq!(
                std::fs::read(root.join("java-17/bin/java.exe")).expect("valid Java restored"),
                b"old java"
            );
            assert!(!backup.exists());
            std::fs::remove_dir_all(root).expect("temporary root removed");
        });
    }

    #[cfg(windows)]
    #[test]
    fn recovery_rejects_a_backup_whose_bin_component_is_a_junction() {
        use std::process::Command;

        crate::tasks::block_on(async {
            let root = temporary_root();
            std::fs::create_dir_all(root.join("java-17/bin")).expect("broken final runtime");
            std::fs::write(root.join("java-17/bin/java.exe"), b"broken java")
                .expect("broken Java executable");
            let backup = root.join(".backup-java-17");
            std::fs::create_dir(&backup).expect("backup root");
            let external = temporary_root();
            std::fs::write(external.join("java.exe"), b"old java")
                .expect("external Java executable");
            let junction = backup.join("bin");
            let output = Command::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(&junction)
                .arg(&external)
                .output()
                .expect("junction command starts");
            assert!(output.status.success(), "junction fixture must be created");
            let installer = RuntimeInstaller::new(
                root.clone(),
                Arc::new(FileContentRunner),
                Arc::new(FakeFetcher(Vec::new())),
                manifest("0".repeat(64)),
            );

            let result = installer
                .recover_interrupted_swap(JavaRequirement::new(17).unwrap())
                .await;

            std::fs::remove_dir(&junction).expect("junction is removed without following it");
            std::fs::remove_dir_all(root).expect("runtime root removed");
            std::fs::remove_dir_all(external).expect("external root removed");
            assert_eq!(
                result.expect_err("junction-backed Java is rejected").code(),
                "invalid_path"
            );
        });
    }

    #[test]
    fn backup_discovery_ignores_loose_prefix_lookalike_directories() {
        let root = temporary_root();
        for name in [
            ".backup-java-17-attacker",
            ".backup-java-17-",
            ".backup-java-17-12-extra",
            ".backup-java-17-18446744073709551616",
        ] {
            std::fs::create_dir(root.join(name)).expect("lookalike directory");
        }

        assert!(super::discover_backup(&root, 17)
            .expect("backup discovery")
            .is_none());
        std::fs::remove_dir_all(root).expect("runtime root removed");
    }

    #[test]
    fn backup_discovery_accepts_current_and_numeric_legacy_names() {
        for name in [".backup-java-17", ".backup-java-17-18446744073709551615"] {
            let root = temporary_root();
            let expected = root.join(name);
            std::fs::create_dir(&expected).expect("generated backup directory");

            assert_eq!(
                super::discover_backup(&root, 17).expect("backup discovery"),
                Some(expected)
            );
            std::fs::remove_dir_all(root).expect("runtime root removed");
        }
    }
}
use super::{
    archive::extract_zip_archive_cancellable, detect::probe_java, java_executable, JavaRequirement,
    JavaRuntimeSource, JavaRuntimeState, JavaRuntimeStatus, ProcessRunner,
};
use crate::{
    downloads::{DownloadCancellationToken, DownloadHttpClient, DownloadTimeouts},
    error::LauncherError,
    paths::AppPaths,
};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_RUNTIME_ARCHIVE_BYTES: usize = 536_870_912;

pub(crate) trait RuntimeSwapFileSystem: Send + Sync {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove_path(&self, path: &Path) -> std::io::Result<()>;
}

struct StandardRuntimeSwapFileSystem;

impl RuntimeSwapFileSystem for StandardRuntimeSwapFileSystem {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_path(&self, path: &Path) -> std::io::Result<()> {
        remove_path(path)
    }
}

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
    async fn fetch(
        &self,
        url: &str,
        max_bytes: usize,
        cancel: &DownloadCancellationToken,
    ) -> Result<Vec<u8>, LauncherError>;
}

pub struct BoundedReqwestRuntimeArchiveFetcher {
    client: DownloadHttpClient,
}

impl BoundedReqwestRuntimeArchiveFetcher {
    pub fn new() -> Result<Self, LauncherError> {
        let client =
            DownloadHttpClient::new(DownloadTimeouts::default()).map_err(|_| fetch_error())?;
        Ok(Self { client })
    }
}

#[async_trait]
impl RuntimeArchiveFetcher for BoundedReqwestRuntimeArchiveFetcher {
    async fn fetch(
        &self,
        url: &str,
        max_bytes: usize,
        cancel: &DownloadCancellationToken,
    ) -> Result<Vec<u8>, LauncherError> {
        self.client
            .fetch_bytes_bounded_cancellable(url, max_bytes, cancel)
            .await
            .map_err(|error| {
                if error.code() == "download_cancelled" {
                    error
                } else {
                    fetch_error()
                }
            })
    }
}

pub struct RuntimeInstaller {
    runtime_root: PathBuf,
    runner: Arc<dyn ProcessRunner>,
    fetcher: Arc<dyn RuntimeArchiveFetcher>,
    manifest: RuntimeArchiveManifest,
    swap_files: Arc<dyn RuntimeSwapFileSystem>,
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
            swap_files: Arc::new(StandardRuntimeSwapFileSystem),
        }
    }

    #[cfg(test)]
    fn with_swap_file_system(
        runtime_root: PathBuf,
        runner: Arc<dyn ProcessRunner>,
        fetcher: Arc<dyn RuntimeArchiveFetcher>,
        manifest: RuntimeArchiveManifest,
        swap_files: Arc<dyn RuntimeSwapFileSystem>,
    ) -> Self {
        Self {
            runtime_root,
            runner,
            fetcher,
            manifest,
            swap_files,
        }
    }

    pub async fn install(
        &self,
        requirement: JavaRequirement,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        self.install_cancellable(requirement, DownloadCancellationToken::new())
            .await
    }

    pub async fn install_cancellable(
        &self,
        requirement: JavaRequirement,
        cancel: DownloadCancellationToken,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        ensure_not_cancelled(&cancel)?;
        await_runtime_or_cancel(&cancel, self.recover_interrupted_swap(requirement)).await?;
        let entry = self
            .manifest
            .entry(requirement.major())
            .ok_or_else(invalid_manifest)?
            .clone();
        let bytes = self
            .fetcher
            .fetch(&entry.url, MAX_RUNTIME_ARCHIVE_BYTES, &cancel)
            .await?;
        ensure_not_cancelled(&cancel)?;
        verify_archive_checksum(&bytes, &entry.sha256)?;
        fs::create_dir_all(&self.runtime_root).map_err(|_| install_error())?;
        let nonce = rand::random::<u64>();
        let temp_name = format!(".install-java-{}-{nonce}", requirement.major());
        let temp = safe_child(&self.runtime_root, &temp_name)?;
        fs::create_dir(&temp).map_err(|_| install_error())?;

        let extraction = extract_zip_archive_cancellable(&bytes, &temp, &cancel);
        if let Err(error) = extraction {
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }
        let Some(java) = find_java_executable(&temp) else {
            let _ = fs::remove_dir_all(&temp);
            return Err(install_error());
        };
        ensure_not_cancelled(&cancel)?;
        let probe_path = java.clone();
        let (major, version) =
            match await_runtime_or_cancel(&cancel, probe_java(self.runner.as_ref(), &probe_path))
                .await
            {
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
        if let Err(error) = replace_runtime(
            &self.runtime_root,
            requirement.major(),
            &candidate,
            self.swap_files.as_ref(),
        ) {
            let _ = self.swap_files.remove_path(&candidate);
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

    pub async fn recover_interrupted_swap(
        &self,
        requirement: JavaRequirement,
    ) -> Result<(), LauncherError> {
        let Some(backup) = discover_backup(&self.runtime_root, requirement.major())? else {
            return Ok(());
        };
        let final_name = format!("java-{}", requirement.major());
        let mut final_path = safe_child(&self.runtime_root, &final_name)?;
        if !final_path.exists() {
            let backup = revalidate_child(&self.runtime_root, &backup)?;
            final_path = safe_child(&self.runtime_root, &final_name)?;
            self.swap_files
                .rename(&backup, &final_path)
                .map_err(|error| runtime_inconsistent(Some(error.to_string())))?;
        }

        let final_java = validated_java_executable(&self.runtime_root, &final_path)?;
        match probe_java(self.runner.as_ref(), &final_java).await {
            Ok((major, _)) if major == requirement.major() => {
                if backup.exists() {
                    let backup = revalidate_child(&self.runtime_root, &backup)?;
                    let _ = self.swap_files.remove_path(&backup);
                }
                Ok(())
            }
            _ if backup.exists() => {
                let backup_java = validated_java_executable(&self.runtime_root, &backup)?;
                match probe_java(self.runner.as_ref(), &backup_java).await {
                    Ok((major, _)) if major == requirement.major() => {}
                    _ => return Err(runtime_inconsistent(None)),
                }
                let failed_name = format!(
                    ".failed-java-{}-{}",
                    requirement.major(),
                    rand::random::<u64>()
                );
                let final_path = safe_child(&self.runtime_root, &final_name)?;
                let failed = safe_child(&self.runtime_root, &failed_name)?;
                self.swap_files
                    .rename(&final_path, &failed)
                    .map_err(|error| runtime_inconsistent(Some(error.to_string())))?;
                let backup = revalidate_child(&self.runtime_root, &backup)?;
                let final_path = safe_child(&self.runtime_root, &final_name)?;
                if let Err(activation_error) = self.swap_files.rename(&backup, &final_path) {
                    let failed = revalidate_child(&self.runtime_root, &failed)?;
                    let final_path = safe_child(&self.runtime_root, &final_name)?;
                    if let Err(rollback_error) = self.swap_files.rename(&failed, &final_path) {
                        return Err(runtime_inconsistent(Some(format!(
                            "backup activation failed: {activation_error}; rollback failed: {rollback_error}"
                        ))));
                    }
                    return Err(runtime_inconsistent(Some(activation_error.to_string())));
                }
                let failed = revalidate_child(&self.runtime_root, &failed)?;
                let _ = self.swap_files.remove_path(&failed);
                Ok(())
            }
            _ => Err(runtime_inconsistent(None)),
        }
    }
}

fn ensure_not_cancelled(cancel: &DownloadCancellationToken) -> Result<(), LauncherError> {
    if cancel.is_cancelled() {
        Err(LauncherError::new(
            "download_cancelled",
            "The operation was cancelled.",
            None,
            true,
        ))
    } else {
        Ok(())
    }
}

async fn await_runtime_or_cancel<T>(
    cancel: &DownloadCancellationToken,
    future: impl std::future::Future<Output = Result<T, LauncherError>>,
) -> Result<T, LauncherError> {
    let cancelled = cancel.cancelled();
    futures_util::pin_mut!(cancelled, future);
    match futures_util::future::select(cancelled, future).await {
        futures_util::future::Either::Left(_) => Err(LauncherError::new(
            "download_cancelled",
            "The operation was cancelled.",
            None,
            true,
        )),
        futures_util::future::Either::Right((result, _)) => result,
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
    swap_files: &dyn RuntimeSwapFileSystem,
) -> Result<(), LauncherError> {
    let final_name = format!("java-{major}");
    let final_path = safe_child(root, &final_name)?;
    let backup_name = format!(".backup-java-{major}");
    let backup = safe_child(root, &backup_name)?;
    if backup.exists() {
        return Err(runtime_inconsistent(None));
    }
    let candidate = revalidate_child(root, candidate)?;
    let had_previous = final_path.exists();
    if had_previous {
        let final_path = safe_child(root, &final_name)?;
        let backup = safe_child(root, &backup_name)?;
        swap_files
            .rename(&final_path, &backup)
            .map_err(|_| install_error())?;
    }
    let candidate = match revalidate_child(root, &candidate) {
        Ok(candidate) => candidate,
        Err(error) => {
            if had_previous {
                restore_backup(root, &backup_name, &final_name, swap_files)?;
            }
            return Err(error);
        }
    };
    let final_path = safe_child(root, &final_name)?;
    if let Err(error) = swap_files.rename(&candidate, &final_path) {
        if had_previous {
            restore_backup(root, &backup_name, &final_name, swap_files).map_err(
                |rollback_error| {
                    runtime_inconsistent(Some(format!(
                        "activation failed: {error}; rollback failed: {rollback_error}"
                    )))
                },
            )?;
        }
        return Err(LauncherError::new(
            "runtime_install_failed",
            "The Java runtime could not be installed.",
            Some(error.to_string()),
            true,
        ));
    }
    if had_previous {
        let backup = safe_child(root, &backup_name)?;
        let _ = swap_files.remove_path(&backup);
    }
    Ok(())
}

fn restore_backup(
    root: &Path,
    backup_name: &str,
    final_name: &str,
    swap_files: &dyn RuntimeSwapFileSystem,
) -> Result<(), LauncherError> {
    let backup = safe_child(root, backup_name)?;
    let final_path = safe_child(root, final_name)?;
    swap_files
        .rename(&backup, &final_path)
        .map_err(|error| runtime_inconsistent(Some(error.to_string())))
}

pub(crate) fn recover_interrupted_swaps_on_startup(root: &Path) -> Result<(), LauncherError> {
    if !root.exists() {
        return Ok(());
    }
    let swap_files = StandardRuntimeSwapFileSystem;
    for major in super::SUPPORTED_JAVA_MAJORS {
        let Some(backup) = discover_backup(root, major)? else {
            continue;
        };
        let final_name = format!("java-{major}");
        let final_path = safe_child(root, &final_name)?;
        if !final_path.exists() {
            let backup = revalidate_child(root, &backup)?;
            let final_path = safe_child(root, &final_name)?;
            swap_files
                .rename(&backup, &final_path)
                .map_err(|error| runtime_inconsistent(Some(error.to_string())))?;
        }
    }
    Ok(())
}

fn discover_backup(root: &Path, major: u16) -> Result<Option<PathBuf>, LauncherError> {
    let mut backups = fs::read_dir(root)
        .map_err(|_| install_error())?
        .filter_map(Result::ok)
        .filter(|entry| is_generated_backup_name(&entry.file_name().to_string_lossy(), major))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    if backups.len() > 1 {
        return Err(runtime_inconsistent(None));
    }
    Ok(backups.pop())
}

fn is_generated_backup_name(name: &str, major: u16) -> bool {
    let current_name = format!(".backup-java-{major}");
    if name == current_name {
        return true;
    }

    name.strip_prefix(&format!("{current_name}-"))
        .is_some_and(|suffix| suffix.parse::<u64>().is_ok())
}

fn validated_java_executable(root: &Path, runtime_home: &Path) -> Result<PathBuf, LauncherError> {
    let runtime_name = runtime_home
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| runtime_inconsistent(None))?;
    let relative = Path::new(runtime_name).join("bin").join("java.exe");
    AppPaths::new(root.to_path_buf()).safe_join(root, &relative)
}

fn revalidate_child(root: &Path, path: &Path) -> Result<PathBuf, LauncherError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| runtime_inconsistent(None))?;
    safe_child(root, name)
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

fn runtime_inconsistent(details: Option<String>) -> LauncherError {
    LauncherError::new(
        "runtime_state_inconsistent",
        "The managed Java runtime could not be restored consistently.",
        details,
        true,
    )
}
