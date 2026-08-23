pub mod assets;
pub mod libraries;
mod natives;
mod path_safety;

use crate::{
    downloads::{
        verify_file, DownloadCancellationToken, DownloadProgress, DownloadService, DownloadSpec,
        ProgressSink,
    },
    error::LauncherError,
    metadata::{
        models::{Download, Library, ResolvedVersion},
        resolver::MetadataService,
    },
    paths::AppPaths,
    storage::Storage,
};
use assets::{asset_index_invalid, asset_object_path, AssetIndexDocument};
use async_trait::async_trait;
use libraries::{
    library_allowed, library_url, maven_artifact_path, validate_metadata_path, WindowsRuleContext,
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallFileKind {
    VersionJson,
    Client,
    Logging,
    AssetIndex,
    AssetObject,
    Library,
    Native,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallFile {
    pub kind: InstallFileKind,
    pub url: Option<String>,
    pub destination: PathBuf,
    pub expected_size: u64,
    pub sha1: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeArchive {
    pub archive: PathBuf,
    pub excludes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub version_id: String,
    pub files: Vec<InstallFile>,
    pub natives: Vec<NativeArchive>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationState {
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationSummary {
    pub version_id: String,
    pub files_verified: usize,
    pub natives_directory: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationStatus {
    pub operation_id: String,
    pub version_id: String,
    pub state: OperationState,
    pub summary: Option<InstallationSummary>,
    pub error: Option<LauncherError>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStatus {
    pub operation_id: String,
    pub profile_id: String,
    pub state: OperationState,
    pub error: Option<LauncherError>,
}

#[derive(Clone)]
pub struct OperationHandle {
    pub operation_id: String,
    pub cancel_token: crate::downloads::DownloadCancellationToken,
}

impl std::fmt::Debug for OperationHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperationHandle")
            .field("operation_id", &self.operation_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default)]
pub struct OperationRegistry {
    inner: Arc<Mutex<OperationRegistryInner>>,
}

#[derive(Default)]
struct OperationRegistryInner {
    operations: HashMap<String, OperationRecord>,
    active_versions: HashMap<String, String>,
    active_workflow: Option<String>,
    terminal_order: VecDeque<String>,
}

pub(super) const MAX_TERMINAL_OPERATIONS: usize = 128;

struct OperationRecord {
    subject: OperationSubject,
    state: OperationState,
    cancel_token: crate::downloads::DownloadCancellationToken,
    spawned: bool,
    summary: Option<InstallationSummary>,
    error: Option<LauncherError>,
}

enum OperationSubject {
    Installation { version_id: String },
    Workflow { profile_id: String },
}

impl OperationRegistry {
    pub fn begin(&self, version_id: &str) -> Result<OperationHandle, LauncherError> {
        validate_version_id(version_id)?;
        let mut inner = self.inner.lock().map_err(|_| operation_state_error())?;
        if inner.active_workflow.is_some() {
            return Err(LauncherError::new(
                "operation_in_progress",
                "A launch or installation operation is already in progress.",
                None,
                true,
            ));
        }
        if inner.active_versions.contains_key(version_id) {
            return Err(LauncherError::new(
                "installation_in_progress",
                "This Minecraft version is already being installed.",
                None,
                true,
            ));
        }
        let operation_id = format!("install-{:016x}", rand::random::<u64>());
        let cancel_token = crate::downloads::DownloadCancellationToken::new();
        inner
            .active_versions
            .insert(version_id.to_owned(), operation_id.clone());
        inner.operations.insert(
            operation_id.clone(),
            OperationRecord {
                subject: OperationSubject::Installation {
                    version_id: version_id.to_owned(),
                },
                state: OperationState::Running,
                cancel_token: cancel_token.clone(),
                spawned: false,
                summary: None,
                error: None,
            },
        );
        Ok(OperationHandle {
            operation_id,
            cancel_token,
        })
    }

    pub fn begin_launch(&self, profile_id: &str) -> Result<OperationHandle, LauncherError> {
        validate_profile_id(profile_id)?;
        let mut inner = self.inner.lock().map_err(|_| operation_state_error())?;
        if inner.active_workflow.is_some() {
            return Err(LauncherError::new(
                "game_already_running",
                "Minecraft is already running or being prepared.",
                None,
                true,
            ));
        }
        if !inner.active_versions.is_empty() {
            return Err(LauncherError::new(
                "operation_in_progress",
                "A Minecraft installation is already in progress.",
                None,
                true,
            ));
        }
        let operation_id = format!("launch-{:016x}", rand::random::<u64>());
        let cancel_token = crate::downloads::DownloadCancellationToken::new();
        inner.active_workflow = Some(operation_id.clone());
        inner.operations.insert(
            operation_id.clone(),
            OperationRecord {
                subject: OperationSubject::Workflow {
                    profile_id: profile_id.to_owned(),
                },
                state: OperationState::Running,
                cancel_token: cancel_token.clone(),
                spawned: false,
                summary: None,
                error: None,
            },
        );
        Ok(OperationHandle {
            operation_id,
            cancel_token,
        })
    }

    pub fn cancel(&self, operation_id: &str) -> Result<(), LauncherError> {
        let mut inner = self.inner.lock().map_err(|_| operation_state_error())?;
        if let Some(record) = inner.operations.get_mut(operation_id) {
            if matches!(
                record.state,
                OperationState::Running | OperationState::Cancelling
            ) && !record.spawned
            {
                record.cancel_token.cancel();
                record.state = OperationState::Cancelling;
            }
        }
        Ok(())
    }

    pub fn mark_spawned(&self, operation_id: &str) -> Result<(), LauncherError> {
        let mut inner = self.inner.lock().map_err(|_| operation_state_error())?;
        let record = inner
            .operations
            .get_mut(operation_id)
            .ok_or_else(operation_not_found)?;
        if !matches!(record.subject, OperationSubject::Workflow { .. }) {
            return Err(operation_not_found());
        }
        if record.cancel_token.is_cancelled() || matches!(record.state, OperationState::Cancelling)
        {
            return Err(cancelled_error());
        }
        record.spawned = true;
        record.state = OperationState::Running;
        Ok(())
    }

    pub fn finish(
        &self,
        operation_id: &str,
        state: OperationState,
        summary: Option<InstallationSummary>,
        error: Option<LauncherError>,
    ) -> Result<(), LauncherError> {
        let mut inner = self.inner.lock().map_err(|_| operation_state_error())?;
        let subject = {
            let record = inner
                .operations
                .get_mut(operation_id)
                .ok_or_else(operation_not_found)?;
            if matches!(
                record.state,
                OperationState::Completed | OperationState::Cancelled | OperationState::Failed
            ) {
                return Ok(());
            }
            record.state = state;
            record.summary = summary;
            record.error = error;
            match &record.subject {
                OperationSubject::Installation { version_id } => OperationSubject::Installation {
                    version_id: version_id.clone(),
                },
                OperationSubject::Workflow { profile_id } => OperationSubject::Workflow {
                    profile_id: profile_id.clone(),
                },
            }
        };
        match subject {
            OperationSubject::Installation { version_id } => {
                inner.active_versions.remove(&version_id);
            }
            OperationSubject::Workflow { .. } => {
                if inner.active_workflow.as_deref() == Some(operation_id) {
                    inner.active_workflow = None;
                }
            }
        }
        inner.terminal_order.push_back(operation_id.to_owned());
        while inner.terminal_order.len() > MAX_TERMINAL_OPERATIONS {
            if let Some(expired) = inner.terminal_order.pop_front() {
                inner.operations.remove(&expired);
            }
        }
        Ok(())
    }

    pub fn status(&self, operation_id: &str) -> Result<InstallationStatus, LauncherError> {
        let inner = self.inner.lock().map_err(|_| operation_state_error())?;
        let record = inner
            .operations
            .get(operation_id)
            .ok_or_else(operation_not_found)?;
        let OperationSubject::Installation { version_id } = &record.subject else {
            return Err(operation_not_found());
        };
        Ok(InstallationStatus {
            operation_id: operation_id.to_owned(),
            version_id: version_id.clone(),
            state: record.state.clone(),
            summary: record.summary.clone(),
            error: record.error.clone(),
        })
    }

    pub fn workflow_status(&self, operation_id: &str) -> Result<WorkflowStatus, LauncherError> {
        let inner = self.inner.lock().map_err(|_| operation_state_error())?;
        let record = inner
            .operations
            .get(operation_id)
            .ok_or_else(operation_not_found)?;
        let OperationSubject::Workflow { profile_id } = &record.subject else {
            return Err(operation_not_found());
        };
        Ok(WorkflowStatus {
            operation_id: operation_id.to_owned(),
            profile_id: profile_id.clone(),
            state: record.state.clone(),
            error: record.error.clone(),
        })
    }
}

fn validate_profile_id(profile_id: &str) -> Result<(), LauncherError> {
    if profile_id.is_empty()
        || !profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(LauncherError::new(
            "invalid_profile",
            "The launcher profile is invalid.",
            None,
            false,
        ));
    }
    Ok(())
}

fn operation_state_error() -> LauncherError {
    LauncherError::new(
        "operation_state_unavailable",
        "Installation operation state is unavailable.",
        None,
        true,
    )
}
fn operation_not_found() -> LauncherError {
    LauncherError::new(
        "operation_not_found",
        "The installation operation was not found.",
        None,
        false,
    )
}

#[async_trait]
pub trait VersionProvider: Send + Sync {
    async fn resolved_version(&self, id: &str) -> Result<ResolvedVersion, LauncherError>;
}

#[async_trait]
impl VersionProvider for MetadataService {
    async fn resolved_version(&self, id: &str) -> Result<ResolvedVersion, LauncherError> {
        MetadataService::resolved_version(self, id).await
    }
}

#[async_trait]
pub trait VerifiedDownloader: Send + Sync {
    async fn execute(
        &self,
        operation_id: String,
        specs: Vec<DownloadSpec>,
        cancel: DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<(), LauncherError>;
}

#[async_trait]
impl VerifiedDownloader for DownloadService {
    async fn execute(
        &self,
        operation_id: String,
        specs: Vec<DownloadSpec>,
        cancel: DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<(), LauncherError> {
        DownloadService::execute(self, operation_id, specs, cancel, progress).await
    }
}

#[async_trait]
pub trait InstallationStore: Send + Sync {
    async fn set_state(&self, version_id: &str, state: &str) -> Result<(), LauncherError>;

    async fn is_verified(&self, _version_id: &str) -> Result<bool, LauncherError> {
        Ok(false)
    }
}

#[async_trait]
impl InstallationStore for Storage {
    async fn set_state(&self, version_id: &str, state: &str) -> Result<(), LauncherError> {
        self.set_installation_state(version_id, state).await
    }

    async fn is_verified(&self, version_id: &str) -> Result<bool, LauncherError> {
        Ok(self.installation_state(version_id).await?.as_deref() == Some("verified"))
    }
}

#[derive(Clone)]
pub struct Installer {
    game_root: PathBuf,
    versions: Arc<dyn VersionProvider>,
    downloads: Arc<dyn VerifiedDownloader>,
    installations: Arc<dyn InstallationStore>,
    legacy_http: reqwest::Client,
}

impl Installer {
    pub fn with_dependencies(
        game_root: PathBuf,
        versions: Arc<dyn VersionProvider>,
        downloads: Arc<dyn VerifiedDownloader>,
        installations: Arc<dyn InstallationStore>,
    ) -> Result<Self, LauncherError> {
        AppPaths::new(game_root.clone()).safe_join(&game_root, Path::new(""))?;
        let legacy_http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|_| LauncherError::metadata_unavailable())?;
        Ok(Self {
            game_root,
            versions,
            downloads,
            installations,
            legacy_http,
        })
    }

    pub fn production(
        paths: &AppPaths,
        versions: Arc<MetadataService>,
        downloads: Arc<DownloadService>,
        installations: Arc<Storage>,
    ) -> Result<Self, LauncherError> {
        Self::with_dependencies(paths.game.clone(), versions, downloads, installations)
    }

    pub(crate) fn for_game_root(&self, game_root: PathBuf) -> Result<Self, LauncherError> {
        AppPaths::new(game_root.clone()).validate_absolute_directory(&game_root)?;
        Self::with_dependencies(
            game_root.clone(),
            self.versions.clone(),
            Arc::new(DownloadService::new(game_root)?),
            self.installations.clone(),
        )
    }

    pub fn plan(&self, version: &ResolvedVersion) -> Result<InstallPlan, LauncherError> {
        plan_installation(&self.game_root, version)
    }

    pub async fn is_verified_version(
        &self,
        version: &ResolvedVersion,
    ) -> Result<bool, LauncherError> {
        validate_version_id(&version.id)?;
        if !self.installations.is_verified(&version.id).await? {
            return Ok(false);
        }
        let plan = match plan_installation_internal(&self.game_root, version, true) {
            Ok(plan) => plan,
            Err(error) if error.code() == "asset_index_invalid" => return Ok(false),
            Err(error) => return Err(error),
        };
        let canonical_root = self
            .game_root
            .canonicalize()
            .map_err(|_| LauncherError::invalid_path())?;
        for file in &plan.files {
            let relative = file
                .destination
                .strip_prefix(&canonical_root)
                .map_err(|_| LauncherError::invalid_path())?;
            if file.expected_size == 0 && file.sha1.is_none() {
                let path =
                    AppPaths::new(self.game_root.clone()).safe_join(&self.game_root, relative)?;
                if !path
                    .metadata()
                    .is_ok_and(|metadata| metadata.file_type().is_file())
                {
                    return Ok(false);
                }
                continue;
            }
            let spec = DownloadSpec {
                url: file
                    .url
                    .clone()
                    .unwrap_or_else(|| "https://localhost.invalid/derived-file".to_owned()),
                destination: file.destination.clone(),
                expected_size: file.expected_size,
                sha1: file.sha1.clone(),
                sha256: None,
            };
            if !verify_file(&self.game_root, relative, &spec)? {
                return Ok(false);
            }
        }
        if !plan.natives.is_empty()
            && !natives::verify_native_inventory(&self.game_root, &version.id)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    pub async fn install(
        &self,
        operation_id: String,
        version_id: String,
        cancel: DownloadCancellationToken,
    ) -> Result<InstallationSummary, LauncherError> {
        self.install_with_progress(operation_id, version_id, cancel, Arc::new(NoProgress))
            .await
    }

    pub async fn install_with_progress(
        &self,
        operation_id: String,
        version_id: String,
        cancel: DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<InstallationSummary, LauncherError> {
        validate_version_id(&version_id)?;
        let version = self.versions.resolved_version(&version_id).await?;
        self.install_resolved_with_progress(operation_id, version, cancel, progress)
            .await
    }

    pub async fn install_resolved_with_progress(
        &self,
        operation_id: String,
        version: ResolvedVersion,
        cancel: DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<InstallationSummary, LauncherError> {
        validate_version_id(&version.id)?;
        let version_id = version.id.clone();
        self.installations
            .set_state(&version_id, "installing")
            .await?;
        let result = self
            .install_inner(&operation_id, version, &cancel, progress)
            .await;
        match &result {
            Ok(_) => {
                self.installations
                    .set_state(&version_id, "verified")
                    .await?
            }
            Err(error) if error.code() == "download_cancelled" => {
                self.installations
                    .set_state(&version_id, "cancelled")
                    .await?
            }
            Err(_) => self.installations.set_state(&version_id, "failed").await?,
        }
        result
    }

    async fn install_inner(
        &self,
        operation_id: &str,
        version: ResolvedVersion,
        cancel: &DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<InstallationSummary, LauncherError> {
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        let version_id = version.id.clone();
        let base = plan_installation_internal(&self.game_root, &version, false)?;
        persist_resolved_version(&self.game_root, &version, cancel)?;
        let base_specs = self.resolve_download_specs(&base, cancel, None).await?;
        let base_total = total_download_bytes(&base_specs)?;
        let base_progress: Arc<dyn ProgressSink> =
            Arc::new(PhaseProgressSink::new(progress.clone(), 0, base_total));
        self.downloads
            .execute(
                operation_id.to_owned(),
                base_specs,
                cancel.clone(),
                base_progress,
            )
            .await?;
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        let complete = plan_installation_internal(&self.game_root, &version, true)?;
        let object_specs = self
            .resolve_download_specs(&complete, cancel, Some(InstallFileKind::AssetObject))
            .await?;
        let object_total = total_download_bytes(&object_specs)?;
        let combined_total = base_total
            .checked_add(object_total)
            .ok_or_else(LauncherError::metadata_invalid)?;
        let object_progress: Arc<dyn ProgressSink> =
            Arc::new(PhaseProgressSink::new(progress, base_total, combined_total));
        self.downloads
            .execute(
                operation_id.to_owned(),
                object_specs,
                cancel.clone(),
                object_progress,
            )
            .await?;
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        let natives_directory = natives::extract_natives_transactional(
            &self.game_root,
            &version_id,
            &complete.natives,
            cancel,
        )?;
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        Ok(InstallationSummary {
            version_id,
            files_verified: complete.files.len(),
            natives_directory,
        })
    }

    async fn resolve_download_specs(
        &self,
        plan: &InstallPlan,
        cancel: &DownloadCancellationToken,
        only_kind: Option<InstallFileKind>,
    ) -> Result<Vec<DownloadSpec>, LauncherError> {
        let mut specs = Vec::new();
        for file in &plan.files {
            if only_kind.is_some_and(|kind| file.kind != kind) {
                continue;
            }
            let Some(url) = &file.url else {
                continue;
            };
            if cancel.is_cancelled() {
                return Err(cancelled_error());
            }
            let expected_size = if file.expected_size == 0 && file.sha1.is_none() {
                let response = self
                    .legacy_http
                    .head(url)
                    .send()
                    .await
                    .map_err(|_| legacy_size_error())?;
                if !response.status().is_success() {
                    return Err(legacy_size_error());
                }
                response
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(legacy_size_error)?
            } else {
                file.expected_size
            };
            if cancel.is_cancelled() {
                return Err(cancelled_error());
            }
            specs.push(DownloadSpec {
                url: url.clone(),
                destination: file.destination.clone(),
                expected_size,
                sha1: file.sha1.clone(),
                sha256: None,
            });
        }
        Ok(specs)
    }
}

struct NoProgress;
impl ProgressSink for NoProgress {
    fn emit(&self, _event: DownloadProgress) {}
}

pub(super) struct PhaseProgressSink {
    inner: Arc<dyn ProgressSink>,
    offset: u64,
    total: u64,
}

impl PhaseProgressSink {
    pub(super) fn new(inner: Arc<dyn ProgressSink>, offset: u64, total: u64) -> Self {
        Self {
            inner,
            offset,
            total,
        }
    }
}

impl ProgressSink for PhaseProgressSink {
    fn emit(&self, event: DownloadProgress) {
        self.inner.emit(DownloadProgress {
            operation_id: event.operation_id,
            total_bytes: self.total,
            completed_bytes: self
                .offset
                .saturating_add(event.completed_bytes)
                .min(self.total),
            current_file: event.current_file,
        });
    }
}

fn total_download_bytes(specs: &[DownloadSpec]) -> Result<u64, LauncherError> {
    specs.iter().try_fold(0_u64, |total, spec| {
        total
            .checked_add(spec.expected_size)
            .ok_or_else(LauncherError::metadata_invalid)
    })
}

fn persist_resolved_version(
    game_root: &Path,
    version: &ResolvedVersion,
    cancel: &DownloadCancellationToken,
) -> Result<(), LauncherError> {
    if cancel.is_cancelled() {
        return Err(cancelled_error());
    }
    let relative = Path::new("versions")
        .join(&version.id)
        .join(format!("{}.json", version.id));
    if let Some(parent) = relative.parent() {
        natives::create_directories(game_root, parent)?;
    }
    let safety = AppPaths::new(game_root.to_path_buf());
    let destination = safety.safe_join(game_root, &relative)?;
    let bytes = serde_json::to_vec(version).map_err(|_| LauncherError::metadata_invalid())?;
    if destination.exists() && fs::read(&destination).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    let temporary_relative = relative.with_file_name(format!(
        "{}.metadata-{}.tmp",
        version.id,
        rand::random::<u64>()
    ));
    let temporary = safety.safe_join(game_root, &temporary_relative)?;
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| LauncherError::storage_unavailable())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| LauncherError::storage_unavailable())?;
    }
    if cancel.is_cancelled() {
        let _ = fs::remove_file(temporary);
        return Err(cancelled_error());
    }
    replace_file_safely(game_root, &relative, &temporary_relative)?;
    let destination = safety.safe_join(game_root, &relative)?;
    if !fs::read(destination).is_ok_and(|current| current == bytes) {
        return Err(LauncherError::storage_unavailable());
    }
    Ok(())
}

fn replace_file_safely(
    root: &Path,
    destination_relative: &Path,
    temporary_relative: &Path,
) -> Result<(), LauncherError> {
    let safety = AppPaths::new(root.to_path_buf());
    let destination = safety.safe_join(root, destination_relative)?;
    let backup_relative =
        destination_relative.with_file_name(format!("version-{}.backup", rand::random::<u64>()));
    let had_destination = destination.exists();
    if had_destination {
        if !destination
            .metadata()
            .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err(LauncherError::invalid_path());
        }
        let backup = safety.safe_join(root, &backup_relative)?;
        let destination = safety.safe_join(root, destination_relative)?;
        fs::rename(destination, backup).map_err(|_| LauncherError::storage_unavailable())?;
    }
    let temporary = safety.safe_join(root, temporary_relative)?;
    let destination = safety.safe_join(root, destination_relative)?;
    if fs::rename(temporary, destination).is_err() {
        if had_destination {
            let backup = safety.safe_join(root, &backup_relative)?;
            let destination = safety.safe_join(root, destination_relative)?;
            fs::rename(backup, destination).map_err(|_| LauncherError::storage_unavailable())?;
        }
        return Err(LauncherError::storage_unavailable());
    }
    if had_destination {
        let backup = safety.safe_join(root, &backup_relative)?;
        fs::remove_file(backup).map_err(|_| LauncherError::storage_unavailable())?;
    }
    Ok(())
}

fn cancelled_error() -> LauncherError {
    LauncherError::new(
        "download_cancelled",
        "The download was cancelled.",
        None,
        true,
    )
}

fn legacy_size_error() -> LauncherError {
    LauncherError::new(
        "library_size_unavailable",
        "A legacy Minecraft library could not be sized for verified download.",
        None,
        true,
    )
}

pub fn plan_installation(
    game_root: &Path,
    version: &ResolvedVersion,
) -> Result<InstallPlan, LauncherError> {
    plan_installation_internal(game_root, version, true)
}

fn plan_installation_internal(
    game_root: &Path,
    version: &ResolvedVersion,
    expand_assets: bool,
) -> Result<InstallPlan, LauncherError> {
    validate_version_id(&version.id)?;
    let safety = AppPaths::new(game_root.to_path_buf());
    safety.safe_join(game_root, Path::new(""))?;
    let mut plan = InstallPlan {
        version_id: version.id.clone(),
        files: Vec::new(),
        natives: Vec::new(),
    };

    let version_bytes =
        serde_json::to_vec(version).map_err(|_| LauncherError::metadata_invalid())?;
    plan.files.push(InstallFile {
        kind: InstallFileKind::VersionJson,
        url: None,
        destination: safe_destination(
            game_root,
            Path::new("versions")
                .join(&version.id)
                .join(format!("{}.json", version.id)),
        )?,
        expected_size: version_bytes.len() as u64,
        sha1: Some(format!("{:x}", Sha1::digest(&version_bytes))),
    });
    if let Some(client) = &version.downloads.client {
        plan.files.push(file_from_download(
            game_root,
            InstallFileKind::Client,
            client,
            Path::new("versions")
                .join(&version.id)
                .join(format!("{}.jar", version.id)),
        )?);
    }
    if let Some(logging) = logging_download(version)? {
        let path = Path::new("assets").join("log_configs").join(&logging.id);
        plan.files.push(file_from_download(
            game_root,
            InstallFileKind::Logging,
            &logging.file,
            path,
        )?);
    }
    if let Some(index) = &version.asset_index {
        let index_relative = Path::new("assets")
            .join("indexes")
            .join(format!("{}.json", index.id));
        let index_download = Download {
            sha1: index.sha1.clone(),
            size: index.size,
            url: index.url.clone(),
            path: None,
        };
        let index_file = file_from_download(
            game_root,
            InstallFileKind::AssetIndex,
            &index_download,
            &index_relative,
        )?;
        plan.files.push(index_file);
        let index_path = safe_destination(game_root, &index_relative)?;
        if expand_assets && index_path.exists() {
            let bytes = fs::read(&index_path).map_err(|_| asset_index_invalid())?;
            if index.size.is_some_and(|size| size != bytes.len() as u64)
                || index.sha1.as_ref().is_some_and(|hash| {
                    format!("{:x}", Sha1::digest(&bytes)) != hash.to_ascii_lowercase()
                })
            {
                return Err(asset_index_invalid());
            }
            let document: AssetIndexDocument =
                serde_json::from_slice(&bytes).map_err(|_| asset_index_invalid())?;
            let mut unique_objects = std::collections::BTreeMap::new();
            for object in document.objects.into_values() {
                let hash = object.hash.to_ascii_lowercase();
                asset_object_path(&hash)?;
                if unique_objects
                    .insert(hash, object.size)
                    .is_some_and(|existing| existing != object.size)
                {
                    return Err(asset_index_invalid());
                }
            }
            for (hash, size) in unique_objects {
                let relative = asset_object_path(&hash)?;
                plan.files.push(InstallFile {
                    kind: InstallFileKind::AssetObject,
                    url: Some(format!(
                        "https://resources.download.minecraft.net/{}/{}",
                        &hash[..2],
                        hash
                    )),
                    destination: safe_destination(game_root, &relative)?,
                    expected_size: size,
                    sha1: Some(hash),
                });
            }
        }
    }
    for library in &version.libraries {
        add_library(game_root, library, &mut plan)?;
    }
    Ok(plan)
}

fn add_library(
    game_root: &Path,
    library: &Library,
    plan: &mut InstallPlan,
) -> Result<(), LauncherError> {
    if !library_allowed(library, &WindowsRuleContext::default())? {
        return Ok(());
    }
    if let Some(downloads) = &library.downloads {
        if let Some(artifact) = &downloads.artifact {
            let relative = artifact
                .path
                .as_deref()
                .map(validate_metadata_path)
                .transpose()?
                .unwrap_or(maven_artifact_path(&library.name)?);
            let mut artifact = artifact.clone();
            if artifact.url.is_empty() {
                artifact.url = library_url(library.url.as_deref(), &relative)?;
            }
            plan.files.push(file_from_download(
                game_root,
                InstallFileKind::Library,
                &artifact,
                Path::new("libraries").join(relative),
            )?);
        }
        if let Some(template) = library
            .natives
            .as_ref()
            .and_then(|natives| natives.get("windows"))
        {
            let classifier = template.replace("${arch}", "64");
            let download = downloads
                .classifiers
                .get(&classifier)
                .ok_or_else(LauncherError::metadata_invalid)?;
            let coordinate = if let Some((base, extension)) = library.name.split_once('@') {
                format!("{base}:{classifier}@{extension}")
            } else {
                format!("{}:{classifier}", library.name)
            };
            let relative = download
                .path
                .as_deref()
                .map(validate_metadata_path)
                .transpose()?
                .unwrap_or(maven_artifact_path(&coordinate)?);
            let mut download = download.clone();
            if download.url.is_empty() {
                download.url = library_url(library.url.as_deref(), &relative)?;
            }
            let destination = Path::new("libraries").join(relative);
            let file =
                file_from_download(game_root, InstallFileKind::Native, &download, &destination)?;
            plan.natives.push(NativeArchive {
                archive: file.destination.clone(),
                excludes: extract_excludes(library)?,
            });
            plan.files.push(file);
        }
    } else {
        let relative = maven_artifact_path(&library.name)?;
        let url = library_url(library.url.as_deref(), &relative)?;
        plan.files.push(InstallFile {
            kind: InstallFileKind::Library,
            url: Some(url),
            destination: safe_destination(game_root, Path::new("libraries").join(relative))?,
            expected_size: 0,
            sha1: None,
        });
        if let Some(template) = library
            .natives
            .as_ref()
            .and_then(|natives| natives.get("windows"))
        {
            let classifier = template.replace("${arch}", "64");
            let coordinate = if let Some((base, extension)) = library.name.split_once('@') {
                format!("{base}:{classifier}@{extension}")
            } else {
                format!("{}:{classifier}", library.name)
            };
            let relative = maven_artifact_path(&coordinate)?;
            let url = library_url(library.url.as_deref(), &relative)?;
            let destination = safe_destination(game_root, Path::new("libraries").join(relative))?;
            plan.natives.push(NativeArchive {
                archive: destination.clone(),
                excludes: extract_excludes(library)?,
            });
            plan.files.push(InstallFile {
                kind: InstallFileKind::Native,
                url: Some(url),
                destination,
                expected_size: 0,
                sha1: None,
            });
        }
    }
    Ok(())
}

fn file_from_download(
    game_root: &Path,
    kind: InstallFileKind,
    download: &Download,
    relative: impl AsRef<Path>,
) -> Result<InstallFile, LauncherError> {
    if download.url.is_empty() || download.size.is_none() {
        return Err(LauncherError::metadata_invalid());
    }
    Ok(InstallFile {
        kind,
        url: Some(download.url.clone()),
        destination: safe_destination(game_root, relative.as_ref())?,
        expected_size: download.size.expect("checked"),
        sha1: download.sha1.clone(),
    })
}

fn safe_destination(
    game_root: &Path,
    relative: impl AsRef<Path>,
) -> Result<PathBuf, LauncherError> {
    if !path_safety::is_strict_windows_relative_path(relative.as_ref()) {
        return Err(LauncherError::metadata_invalid());
    }
    AppPaths::new(game_root.to_path_buf()).safe_join(game_root, relative.as_ref())
}

fn validate_version_id(id: &str) -> Result<(), LauncherError> {
    let lowercase = id.to_ascii_lowercase();
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.ends_with('.')
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || lowercase.ends_with(".part")
        || lowercase.ends_with(".part.lock")
    {
        return Err(LauncherError::metadata_invalid());
    }
    Ok(())
}

#[derive(Deserialize)]
struct LoggingRoot {
    client: Option<LoggingClient>,
}
#[derive(Deserialize)]
struct LoggingClient {
    file: LoggingFile,
}
#[derive(Deserialize)]
struct LoggingFile {
    id: String,
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}
struct LoggingDownload {
    id: String,
    file: Download,
}

fn logging_download(version: &ResolvedVersion) -> Result<Option<LoggingDownload>, LauncherError> {
    let Some(value) = &version.logging else {
        return Ok(None);
    };
    let root: LoggingRoot =
        serde_json::from_value(value.clone()).map_err(|_| LauncherError::metadata_invalid())?;
    Ok(root.client.map(|client| LoggingDownload {
        id: client.file.id,
        file: Download {
            sha1: client.file.sha1,
            size: client.file.size,
            url: client.file.url,
            path: None,
        },
    }))
}

fn extract_excludes(library: &Library) -> Result<Vec<String>, LauncherError> {
    let Some(value) = &library.extract else {
        return Ok(Vec::new());
    };
    let excludes = value
        .get("exclude")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(LauncherError::metadata_invalid)?;
    excludes
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(LauncherError::metadata_invalid)
        })
        .collect()
}

#[cfg(test)]
mod tests;
