pub mod archive;
pub mod detect;
pub mod install;

use crate::downloads::DownloadCancellationToken;
use crate::error::LauncherError;
use crate::metadata::models::ResolvedVersion;
use detect::probe_java;
pub use detect::{parse_java_major, ProcessOutput, ProcessRunner, TokioProcessRunner};
use install::{
    recover_interrupted_swaps_on_startup, BoundedReqwestRuntimeArchiveFetcher,
    RuntimeArchiveFetcher, RuntimeArchiveManifest, RuntimeInstaller,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const SUPPORTED_JAVA_MAJORS: [u16; 4] = [8, 17, 21, 25];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct JavaRequirement(u16);

impl JavaRequirement {
    pub fn new(major: u16) -> Result<Self, LauncherError> {
        if SUPPORTED_JAVA_MAJORS.contains(&major) {
            Ok(Self(major))
        } else {
            Err(LauncherError::new(
                "unsupported_java_version",
                "This Java version is not supported by the launcher.",
                None,
                false,
            ))
        }
    }

    pub fn major(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for JavaRequirement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let major = u16::deserialize(deserializer)?;
        Self::new(major).map_err(|_| serde::de::Error::custom("unsupported Java major"))
    }
}

pub fn requirement_for_version(
    version: &ResolvedVersion,
) -> Result<JavaRequirement, LauncherError> {
    let major = version
        .java_version
        .as_ref()
        .map(|java| java.major_version)
        .unwrap_or(8);
    let major = u16::try_from(major).map_err(|_| {
        LauncherError::new(
            "runtime_unavailable",
            "A compatible Java runtime is unavailable.",
            None,
            true,
        )
    })?;
    JavaRequirement::new(major)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JavaRuntimeSource {
    Managed,
    Manual,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JavaRuntimeState {
    Valid,
    Missing,
    Installing,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntimeStatus {
    pub requirement: u16,
    pub state: JavaRuntimeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<JavaRuntimeSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

pub struct RuntimeManager {
    runtime_root: PathBuf,
    runner: Arc<dyn ProcessRunner>,
    system_candidates: Vec<PathBuf>,
    manual_overrides: tokio::sync::RwLock<HashMap<u16, PathBuf>>,
    installer: Option<RuntimeInstaller>,
    installing: tokio::sync::Mutex<HashSet<u16>>,
}

impl RuntimeManager {
    pub fn new(
        runtime_root: PathBuf,
        runner: Arc<dyn ProcessRunner>,
        system_candidates: Vec<PathBuf>,
    ) -> Self {
        let manual_overrides = load_manual_overrides(&runtime_root);
        Self {
            runtime_root,
            runner,
            system_candidates,
            manual_overrides: tokio::sync::RwLock::new(manual_overrides),
            installer: None,
            installing: tokio::sync::Mutex::new(HashSet::new()),
        }
    }

    pub fn with_installer(
        runtime_root: PathBuf,
        runner: Arc<dyn ProcessRunner>,
        system_candidates: Vec<PathBuf>,
        fetcher: Arc<dyn RuntimeArchiveFetcher>,
        manifest: RuntimeArchiveManifest,
    ) -> Self {
        let mut manager = Self::new(runtime_root.clone(), runner.clone(), system_candidates);
        manager.installer = Some(RuntimeInstaller::new(
            runtime_root,
            runner,
            fetcher,
            manifest,
        ));
        manager
    }

    pub fn production(runtime_root: PathBuf) -> Result<Self, LauncherError> {
        recover_interrupted_swaps_on_startup(&runtime_root)?;
        let system_candidates = system_java_candidates();
        let runner: Arc<dyn ProcessRunner> = Arc::new(TokioProcessRunner);
        let fetcher = Arc::new(BoundedReqwestRuntimeArchiveFetcher::new()?);
        let manifest = RuntimeArchiveManifest::bundled()?;
        Ok(Self::with_installer(
            runtime_root,
            runner,
            system_candidates,
            fetcher,
            manifest,
        ))
    }

    pub async fn resolve(
        &self,
        requirement: JavaRequirement,
        override_path: Option<PathBuf>,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        self.resolve_cancellable(requirement, override_path, DownloadCancellationToken::new())
            .await
    }

    pub async fn resolve_cancellable(
        &self,
        requirement: JavaRequirement,
        override_path: Option<PathBuf>,
        cancel: DownloadCancellationToken,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        ensure_not_cancelled(&cancel)?;
        let managed = self
            .runtime_root
            .join(format!("java-{}", requirement.major()))
            .join("bin")
            .join("java.exe");
        let persisted = self
            .manual_overrides
            .read()
            .await
            .get(&requirement.major())
            .cloned();
        let manual = override_path.or(persisted);
        let mut invalid = None;
        let candidates = std::iter::once((managed, JavaRuntimeSource::Managed))
            .chain(
                manual
                    .into_iter()
                    .map(|path| (normalize_java_path(path), JavaRuntimeSource::Manual)),
            )
            .chain(
                self.system_candidates
                    .iter()
                    .cloned()
                    .map(|path| (normalize_java_path(path), JavaRuntimeSource::System)),
            );

        for (path, source) in candidates {
            ensure_not_cancelled(&cancel)?;
            if !path.is_file() {
                continue;
            }
            let probe_path = path.clone();
            let probe = probe_java(self.runner.as_ref(), &probe_path);
            let cancelled = cancel.cancelled();
            futures_util::pin_mut!(probe, cancelled);
            let result = match futures_util::future::select(cancelled, probe).await {
                futures_util::future::Either::Left(_) => return Err(cancelled_error()),
                futures_util::future::Either::Right((result, _)) => result,
            };
            match result {
                Ok((major, version)) if major == requirement.major() => {
                    return Ok(JavaRuntimeStatus {
                        requirement: requirement.major(),
                        state: JavaRuntimeState::Valid,
                        path: Some(path),
                        source: Some(source),
                        version: Some(version),
                    })
                }
                Ok((_major, version)) => {
                    invalid.get_or_insert((path, source, Some(version)));
                }
                Err(_) => {
                    invalid.get_or_insert((path, source, None));
                }
            }
        }
        if let Some((path, source, version)) = invalid {
            return Ok(JavaRuntimeStatus {
                requirement: requirement.major(),
                state: JavaRuntimeState::Invalid,
                path: Some(path),
                source: Some(source),
                version,
            });
        }
        Ok(JavaRuntimeStatus {
            requirement: requirement.major(),
            state: JavaRuntimeState::Missing,
            path: None,
            source: None,
            version: None,
        })
    }

    pub async fn statuses(&self) -> Result<Vec<JavaRuntimeStatus>, LauncherError> {
        let installing = self.installing.lock().await.clone();
        let mut statuses = Vec::with_capacity(SUPPORTED_JAVA_MAJORS.len());
        for major in SUPPORTED_JAVA_MAJORS {
            if installing.contains(&major) {
                statuses.push(JavaRuntimeStatus {
                    requirement: major,
                    state: JavaRuntimeState::Installing,
                    path: None,
                    source: Some(JavaRuntimeSource::Managed),
                    version: None,
                });
            } else {
                statuses.push(self.resolve(JavaRequirement::new(major)?, None).await?);
            }
        }
        Ok(statuses)
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
        {
            let mut installing = self.installing.lock().await;
            if !installing.insert(requirement.major()) {
                return Ok(JavaRuntimeStatus {
                    requirement: requirement.major(),
                    state: JavaRuntimeState::Installing,
                    path: None,
                    source: Some(JavaRuntimeSource::Managed),
                    version: None,
                });
            }
        }
        let result = match &self.installer {
            Some(installer) => installer.install_cancellable(requirement, cancel).await,
            None => Err(LauncherError::new(
                "runtime_install_unavailable",
                "Managed Java installation is unavailable.",
                None,
                true,
            )),
        };
        self.installing.lock().await.remove(&requirement.major());
        result
    }

    pub async fn choose_manual_runtime(
        &self,
        requirement: JavaRequirement,
        path: PathBuf,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        let executable = normalize_java_path(path)
            .canonicalize()
            .map_err(|_| java_invalid())?;
        let (major, version) = probe_java(self.runner.as_ref(), &executable).await?;
        if major != requirement.major() {
            return Err(java_invalid());
        }
        let mut overrides = self.manual_overrides.write().await;
        let mut next = overrides.clone();
        next.insert(requirement.major(), executable.clone());
        persist_manual_overrides(&self.runtime_root, &next)?;
        *overrides = next;
        Ok(JavaRuntimeStatus {
            requirement: requirement.major(),
            state: JavaRuntimeState::Valid,
            path: Some(executable),
            source: Some(JavaRuntimeSource::Manual),
            version: Some(version),
        })
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

fn normalize_java_path(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        path
    } else {
        path.join("bin").join("java.exe")
    }
}

fn system_java_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        paths.push(PathBuf::from(home));
    }
    if let Some(search_path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&search_path).map(|dir| dir.join("java.exe")));
    }
    paths
}

pub(crate) fn java_executable(home: &Path) -> PathBuf {
    home.join("bin").join("java.exe")
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManualOverridesFile {
    schema_version: u32,
    paths: BTreeMap<u16, PathBuf>,
}

fn load_manual_overrides(root: &Path) -> HashMap<u16, PathBuf> {
    let path = root.join("java-overrides-v1.json");
    let Ok(bytes) = fs::read(path) else {
        return HashMap::new();
    };
    let Ok(file) = serde_json::from_slice::<ManualOverridesFile>(&bytes) else {
        return HashMap::new();
    };
    if file.schema_version != 1 {
        return HashMap::new();
    }
    file.paths
        .into_iter()
        .filter(|(major, _)| SUPPORTED_JAVA_MAJORS.contains(major))
        .collect()
}

fn persist_manual_overrides(
    root: &Path,
    overrides: &HashMap<u16, PathBuf>,
) -> Result<(), LauncherError> {
    fs::create_dir_all(root).map_err(|_| LauncherError::storage_unavailable())?;
    let safety = crate::paths::AppPaths::new(root.to_path_buf());
    let nonce = rand::random::<u64>();
    let temp = safety.safe_join(root, Path::new(&format!(".java-overrides-{nonce}.tmp")))?;
    let target = safety.safe_join(root, Path::new("java-overrides-v1.json"))?;
    let backup = safety.safe_join(root, Path::new(&format!(".java-overrides-{nonce}.bak")))?;
    let file = ManualOverridesFile {
        schema_version: 1,
        paths: overrides
            .iter()
            .map(|(major, path)| (*major, path.clone()))
            .collect(),
    };
    let bytes =
        serde_json::to_vec_pretty(&file).map_err(|_| LauncherError::storage_unavailable())?;
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| LauncherError::storage_unavailable())?;
    writer
        .write_all(&bytes)
        .and_then(|_| writer.sync_all())
        .map_err(|_| LauncherError::storage_unavailable())?;
    let had_previous = target.exists();
    if had_previous {
        fs::rename(&target, &backup).map_err(|_| LauncherError::storage_unavailable())?;
    }
    if fs::rename(&temp, &target).is_err() {
        if had_previous {
            let _ = fs::rename(&backup, &target);
        }
        return Err(LauncherError::storage_unavailable());
    }
    if had_previous {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn java_invalid() -> LauncherError {
    LauncherError::new(
        "java_runtime_invalid",
        "The selected Java runtime is missing, incompatible, or could not be started.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        requirement_for_version, JavaRequirement, JavaRuntimeSource, ProcessOutput, ProcessRunner,
        RuntimeManager,
    };
    use crate::{
        error::LauncherError,
        metadata::models::{JavaVersion, ResolvedVersion, VersionJson},
    };
    use async_trait::async_trait;
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    struct Java17Runner;
    #[async_trait]
    impl ProcessRunner for Java17Runner {
        async fn run(
            &self,
            _executable: &Path,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<ProcessOutput, LauncherError> {
            Ok(ProcessOutput {
                success: true,
                stdout: String::new(),
                stderr: "openjdk version \"17.0.20\"".to_owned(),
            })
        }
    }
    fn temporary_root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ck-runtime-manager-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).expect("runtime root");
        root
    }

    #[test]
    fn manual_path_is_persisted_only_after_a_successful_matching_probe() {
        tauri::async_runtime::block_on(async {
            let root = temporary_root();
            let chosen = root.join("chosen-java.exe");
            std::fs::write(&chosen, b"fake").expect("chosen java");
            let manager = RuntimeManager::new(root.clone(), Arc::new(Java17Runner), Vec::new());
            let status = manager
                .choose_manual_runtime(JavaRequirement::new(17).unwrap(), chosen.clone())
                .await
                .expect("matching Java persists");
            assert_eq!(status.source, Some(JavaRuntimeSource::Manual));

            let reloaded = RuntimeManager::new(root.clone(), Arc::new(Java17Runner), Vec::new());
            let status = reloaded
                .resolve(JavaRequirement::new(17).unwrap(), None)
                .await
                .expect("persisted runtime resolves");
            let canonical_chosen = chosen.canonicalize().expect("chosen path canonicalizes");
            assert_eq!(status.path.as_deref(), Some(canonical_chosen.as_path()));

            let mismatch = root.join("wrong-java.exe");
            std::fs::write(&mismatch, b"fake").expect("wrong java");
            let error = manager
                .choose_manual_runtime(JavaRequirement::new(21).unwrap(), mismatch)
                .await
                .expect_err("mismatched Java is not persisted");
            assert_eq!(error.code(), "java_runtime_invalid");
            std::fs::remove_dir_all(root).expect("runtime root removed");
        });
    }

    #[test]
    fn command_dto_rejects_unsupported_java_majors_during_deserialization() {
        assert!(serde_json::from_str::<JavaRequirement>("99").is_err());
        assert_eq!(
            serde_json::from_str::<JavaRequirement>("21")
                .unwrap()
                .major(),
            21
        );
    }

    #[test]
    fn version_metadata_drives_the_same_java_requirement_used_for_launch() {
        let declared = ResolvedVersion::from(VersionJson {
            java_version: Some(JavaVersion {
                component: "java-runtime-gamma".to_owned(),
                major_version: 21,
            }),
            ..VersionJson::default()
        });
        let legacy = ResolvedVersion::from(VersionJson::default());

        assert_eq!(requirement_for_version(&declared).unwrap().major(), 21);
        assert_eq!(requirement_for_version(&legacy).unwrap().major(), 8);
    }
}
