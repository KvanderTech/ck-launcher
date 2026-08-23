use crate::{
    auth::AuthService,
    downloads::{DownloadCancellationToken, DownloadProgress, ProgressSink},
    error::LauncherError,
    installer::{Installer, OperationHandle, OperationRegistry, OperationState},
    launcher::{build_launch, LaunchAccount, LaunchBuildRequest, Launcher, PreparedLaunch},
    metadata::{models::ResolvedVersion, resolver::MetadataService},
    paths::AppPaths,
    profiles::PhysicalMemory,
    runtime::{requirement_for_version, JavaRuntimeState, JavaRuntimeStatus, RuntimeManager},
    storage::{LauncherProfile, ProfileStore},
};
use async_trait::async_trait;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WorkflowStage {
    Authenticating,
    ResolvingMetadata,
    ResolvingJava,
    Installing,
    Launching,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchProgress {
    pub operation_id: String,
    pub stage: WorkflowStage,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub current_file: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowErrorEvent {
    pub operation_id: String,
    pub profile_id: String,
    pub stage: WorkflowStage,
    pub error: LauncherError,
}

#[derive(Clone, Debug)]
pub(crate) enum WorkflowEvent {
    Progress(LaunchProgress),
    Error(WorkflowErrorEvent),
}

pub(crate) trait WorkflowEventSink: Send + Sync {
    fn emit(&self, event: WorkflowEvent);
}

#[derive(Clone)]
pub(crate) struct ProgressBridge {
    operation_id: String,
    events: Arc<dyn WorkflowEventSink>,
    stage: Arc<Mutex<Option<WorkflowStage>>>,
}

impl ProgressBridge {
    pub(crate) fn new(operation_id: String, events: Arc<dyn WorkflowEventSink>) -> Self {
        Self {
            operation_id,
            events,
            stage: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn advance(&self, stage: WorkflowStage) {
        let should_emit = self
            .stage
            .lock()
            .map(|mut current| {
                if current.is_some_and(|value| stage < value) {
                    false
                } else {
                    *current = Some(stage);
                    true
                }
            })
            .unwrap_or(false);
        if should_emit {
            self.events.emit(WorkflowEvent::Progress(LaunchProgress {
                operation_id: self.operation_id.clone(),
                stage,
                completed_bytes: 0,
                total_bytes: 0,
                current_file: None,
            }));
        }
    }
}

impl ProgressSink for ProgressBridge {
    fn emit(&self, event: DownloadProgress) {
        let installing = self
            .stage
            .lock()
            .map(|stage| *stage == Some(WorkflowStage::Installing))
            .unwrap_or(false);
        if !installing {
            return;
        }
        self.events.emit(WorkflowEvent::Progress(LaunchProgress {
            operation_id: self.operation_id.clone(),
            stage: WorkflowStage::Installing,
            completed_bytes: event.completed_bytes,
            total_bytes: event.total_bytes,
            current_file: safe_file_name(&event.current_file),
        }));
    }
}

fn safe_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

#[async_trait]
pub(crate) trait WorkflowBackend: Send + Sync {
    fn game_active(&self) -> Result<bool, LauncherError>;
    async fn authenticate(&self) -> Result<LaunchAccount, LauncherError>;
    async fn metadata(
        &self,
        profile_id: &str,
    ) -> Result<(LauncherProfile, ResolvedVersion), LauncherError>;
    async fn runtime(
        &self,
        profile: &LauncherProfile,
        version: &ResolvedVersion,
    ) -> Result<JavaRuntimeStatus, LauncherError>;
    async fn installation_verified(&self, version: &ResolvedVersion)
        -> Result<bool, LauncherError>;
    async fn install(
        &self,
        operation_id: &str,
        version: &ResolvedVersion,
        cancel: DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<(), LauncherError>;
    fn build(
        &self,
        account: LaunchAccount,
        profile: LauncherProfile,
        version: ResolvedVersion,
        runtime: JavaRuntimeStatus,
    ) -> Result<PreparedLaunch, LauncherError>;
    async fn spawn(
        &self,
        profile_id: &str,
        operation_id: &str,
        prepared: PreparedLaunch,
    ) -> Result<(), LauncherError>;
}

#[derive(Clone)]
pub(crate) struct LaunchOrchestrator {
    backend: Arc<dyn WorkflowBackend>,
    operations: OperationRegistry,
    events: Arc<dyn WorkflowEventSink>,
}

impl LaunchOrchestrator {
    pub(crate) fn new(
        backend: Arc<dyn WorkflowBackend>,
        operations: OperationRegistry,
        events: Arc<dyn WorkflowEventSink>,
    ) -> Self {
        Self {
            backend,
            operations,
            events,
        }
    }

    pub(crate) fn reserve(&self, profile_id: &str) -> Result<OperationHandle, LauncherError> {
        if self.backend.game_active()? {
            return Err(LauncherError::new(
                "game_already_running",
                "Minecraft is already running.",
                None,
                true,
            ));
        }
        self.operations.begin_launch(profile_id)
    }

    pub(crate) async fn execute(
        &self,
        profile_id: String,
        handle: OperationHandle,
    ) -> Result<(), LauncherError> {
        let progress = ProgressBridge::new(handle.operation_id.clone(), self.events.clone());

        progress.advance(WorkflowStage::Authenticating);
        let account = match self.backend.authenticate().await {
            Ok(account) => account,
            Err(error) => {
                return self.terminate(
                    &profile_id,
                    &handle.operation_id,
                    WorkflowStage::Authenticating,
                    error,
                )
            }
        };
        self.ensure_not_cancelled(&profile_id, &handle, WorkflowStage::Authenticating)?;

        progress.advance(WorkflowStage::ResolvingMetadata);
        let (profile, version) = match self.backend.metadata(&profile_id).await {
            Ok(value) => value,
            Err(error) => {
                return self.terminate(
                    &profile_id,
                    &handle.operation_id,
                    WorkflowStage::ResolvingMetadata,
                    error,
                )
            }
        };
        self.ensure_not_cancelled(&profile_id, &handle, WorkflowStage::ResolvingMetadata)?;

        progress.advance(WorkflowStage::ResolvingJava);
        let runtime = match self.backend.runtime(&profile, &version).await {
            Ok(runtime) => runtime,
            Err(error) => {
                return self.terminate(
                    &profile_id,
                    &handle.operation_id,
                    WorkflowStage::ResolvingJava,
                    error,
                )
            }
        };
        self.ensure_not_cancelled(&profile_id, &handle, WorkflowStage::ResolvingJava)?;

        let installed = match self.backend.installation_verified(&version).await {
            Ok(installed) => installed,
            Err(error) => {
                return self.terminate(
                    &profile_id,
                    &handle.operation_id,
                    WorkflowStage::Installing,
                    error,
                )
            }
        };
        if !installed {
            progress.advance(WorkflowStage::Installing);
            let install_progress: Arc<dyn ProgressSink> = Arc::new(progress.clone());
            if let Err(error) = self
                .backend
                .install(
                    &handle.operation_id,
                    &version,
                    handle.cancel_token.clone(),
                    install_progress,
                )
                .await
            {
                return self.terminate(
                    &profile_id,
                    &handle.operation_id,
                    WorkflowStage::Installing,
                    error,
                );
            }
            self.ensure_not_cancelled(&profile_id, &handle, WorkflowStage::Installing)?;
        }

        progress.advance(WorkflowStage::Launching);
        let prepared = match self.backend.build(account, profile, version, runtime) {
            Ok(prepared) => prepared,
            Err(error) => {
                return self.terminate(
                    &profile_id,
                    &handle.operation_id,
                    WorkflowStage::Launching,
                    error,
                )
            }
        };
        self.ensure_not_cancelled(&profile_id, &handle, WorkflowStage::Launching)?;
        self.enter_spawn_boundary(&profile_id, &handle.operation_id)?;
        if let Err(error) = self
            .backend
            .spawn(&profile_id, &handle.operation_id, prepared)
            .await
        {
            return self.terminate(
                &profile_id,
                &handle.operation_id,
                WorkflowStage::Launching,
                error,
            );
        }
        Ok(())
    }

    fn enter_spawn_boundary(
        &self,
        profile_id: &str,
        operation_id: &str,
    ) -> Result<(), LauncherError> {
        match self.operations.mark_spawned(operation_id) {
            Ok(()) => Ok(()),
            Err(error) => self.terminate(profile_id, operation_id, WorkflowStage::Launching, error),
        }
    }

    fn ensure_not_cancelled(
        &self,
        profile_id: &str,
        handle: &OperationHandle,
        stage: WorkflowStage,
    ) -> Result<(), LauncherError> {
        if handle.cancel_token.is_cancelled() {
            self.terminate(profile_id, &handle.operation_id, stage, cancelled_error())
        } else {
            Ok(())
        }
    }

    fn terminate(
        &self,
        profile_id: &str,
        operation_id: &str,
        stage: WorkflowStage,
        error: LauncherError,
    ) -> Result<(), LauncherError> {
        let state = if error.code() == "download_cancelled" {
            OperationState::Cancelled
        } else {
            OperationState::Failed
        };
        self.operations
            .finish(operation_id, state, None, Some(error.clone()))?;
        self.events.emit(WorkflowEvent::Error(WorkflowErrorEvent {
            operation_id: operation_id.to_owned(),
            profile_id: profile_id.to_owned(),
            stage,
            error: error.clone(),
        }));
        Err(error)
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

pub(crate) struct ProductionWorkflowBackend {
    auth: Arc<AuthService>,
    profiles: Arc<dyn ProfileStore>,
    metadata: Arc<MetadataService>,
    runtimes: Arc<RuntimeManager>,
    installer: Arc<Installer>,
    launcher: Arc<Launcher>,
    paths: AppPaths,
    memory: Arc<dyn PhysicalMemory>,
}

impl ProductionWorkflowBackend {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        auth: Arc<AuthService>,
        profiles: Arc<dyn ProfileStore>,
        metadata: Arc<MetadataService>,
        runtimes: Arc<RuntimeManager>,
        installer: Arc<Installer>,
        launcher: Arc<Launcher>,
        paths: AppPaths,
        memory: Arc<dyn PhysicalMemory>,
    ) -> Self {
        Self {
            auth,
            profiles,
            metadata,
            runtimes,
            installer,
            launcher,
            paths,
            memory,
        }
    }
}

#[async_trait]
impl WorkflowBackend for ProductionWorkflowBackend {
    fn game_active(&self) -> Result<bool, LauncherError> {
        self.launcher.game_active()
    }

    async fn authenticate(&self) -> Result<LaunchAccount, LauncherError> {
        let (account, access_token) = self
            .auth
            .refresh_active_minecraft_account()
            .await?
            .into_parts();
        Ok(LaunchAccount::new(
            account.minecraft_name,
            account.minecraft_uuid,
            access_token,
        ))
    }

    async fn metadata(
        &self,
        profile_id: &str,
    ) -> Result<(LauncherProfile, ResolvedVersion), LauncherError> {
        let profile = self
            .profiles
            .active_profile()
            .await?
            .filter(|profile| profile.id == profile_id)
            .ok_or_else(profile_not_found)?;
        let version_id = profile.version_id.as_deref().ok_or_else(version_required)?;
        let version = self.metadata.resolved_version(version_id).await?;
        Ok((profile, version))
    }

    async fn runtime(
        &self,
        profile: &LauncherProfile,
        version: &ResolvedVersion,
    ) -> Result<JavaRuntimeStatus, LauncherError> {
        let requirement = requirement_for_version(version)?;
        let resolved = self
            .runtimes
            .resolve(
                requirement,
                profile.java_override.as_ref().map(PathBuf::from),
            )
            .await?;
        let runtime = if resolved.state == JavaRuntimeState::Valid {
            resolved
        } else {
            self.runtimes.install(requirement).await?
        };
        if runtime.state != JavaRuntimeState::Valid || runtime.path.is_none() {
            return Err(runtime_unavailable());
        }
        Ok(runtime)
    }

    async fn installation_verified(
        &self,
        version: &ResolvedVersion,
    ) -> Result<bool, LauncherError> {
        self.installer.is_verified_version(version).await
    }

    async fn install(
        &self,
        operation_id: &str,
        version: &ResolvedVersion,
        cancel: DownloadCancellationToken,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<(), LauncherError> {
        self.installer
            .install_resolved_with_progress(
                operation_id.to_owned(),
                version.clone(),
                cancel,
                progress,
            )
            .await
            .map(|_| ())
    }

    fn build(
        &self,
        account: LaunchAccount,
        profile: LauncherProfile,
        version: ResolvedVersion,
        runtime: JavaRuntimeStatus,
    ) -> Result<PreparedLaunch, LauncherError> {
        build_launch(LaunchBuildRequest {
            account,
            version,
            profile,
            runtime,
            game_root: self.paths.game.clone(),
            physical_memory_mb: self.memory.physical_memory_mb(),
        })
    }

    async fn spawn(
        &self,
        profile_id: &str,
        operation_id: &str,
        prepared: PreparedLaunch,
    ) -> Result<(), LauncherError> {
        self.launcher
            .launch_prepared(profile_id, operation_id, prepared)
            .await
    }
}

fn profile_not_found() -> LauncherError {
    LauncherError::new(
        "profile_not_found",
        "The launcher profile was not found.",
        None,
        false,
    )
}

fn version_required() -> LauncherError {
    LauncherError::new(
        "version_required",
        "Choose a Minecraft version before launching.",
        None,
        true,
    )
}

fn runtime_unavailable() -> LauncherError {
    LauncherError::new(
        "runtime_unavailable",
        "A compatible Java runtime is required.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        LaunchOrchestrator, ProgressBridge, WorkflowBackend, WorkflowErrorEvent, WorkflowEvent,
        WorkflowEventSink, WorkflowStage,
    };
    use crate::{
        downloads::{DownloadCancellationToken, DownloadProgress, ProgressSink},
        error::LauncherError,
        installer::{OperationRegistry, OperationState},
        launcher::{LaunchAccount, LaunchCommand, PreparedLaunch},
        metadata::models::{ResolvedVersion, VersionArguments, VersionDownloads},
        runtime::{JavaRuntimeSource, JavaRuntimeState, JavaRuntimeStatus},
        storage::LauncherProfile,
    };
    use async_trait::async_trait;
    use std::{
        ffi::OsString,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FailurePoint {
        Auth,
        Metadata,
        Runtime,
        Install,
        Build,
        Spawn,
    }

    #[derive(Default)]
    struct RecordingEvents(Mutex<Vec<WorkflowEvent>>);

    impl RecordingEvents {
        fn snapshot(&self) -> Vec<WorkflowEvent> {
            self.0.lock().expect("event lock").clone()
        }
    }

    impl WorkflowEventSink for RecordingEvents {
        fn emit(&self, event: WorkflowEvent) {
            self.0.lock().expect("event lock").push(event);
        }
    }

    struct MockBackend {
        calls: Mutex<Vec<&'static str>>,
        installed: Mutex<bool>,
        fail_once: Mutex<Option<FailurePoint>>,
        cancel_at: Option<WorkflowStage>,
        cancel: Mutex<Option<DownloadCancellationToken>>,
        terminalize_spawn: Mutex<Option<OperationRegistry>>,
    }

    impl MockBackend {
        fn new(installed: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                installed: Mutex::new(installed),
                fail_once: Mutex::new(None),
                cancel_at: None,
                cancel: Mutex::new(None),
                terminalize_spawn: Mutex::new(None),
            }
        }

        fn failing(point: FailurePoint) -> Self {
            let backend = Self::new(false);
            *backend.fail_once.lock().expect("failure lock") = Some(point);
            backend
        }

        fn cancelling(stage: WorkflowStage) -> Self {
            Self {
                cancel_at: Some(stage),
                ..Self::new(false)
            }
        }

        fn attach_cancel(&self, cancel: DownloadCancellationToken) {
            *self.cancel.lock().expect("cancel lock") = Some(cancel);
        }

        fn terminalize_spawn_with(&self, operations: OperationRegistry) {
            *self
                .terminalize_spawn
                .lock()
                .expect("terminal registry lock") = Some(operations);
        }

        fn record(&self, call: &'static str, stage: WorkflowStage) -> Result<(), LauncherError> {
            self.calls.lock().expect("calls lock").push(call);
            if self.cancel_at == Some(stage) {
                if let Some(cancel) = self.cancel.lock().expect("cancel lock").as_ref() {
                    cancel.cancel();
                }
            }
            Ok(())
        }

        fn maybe_fail(&self, point: FailurePoint) -> Result<(), LauncherError> {
            let mut failure = self.fail_once.lock().expect("failure lock");
            if *failure == Some(point) {
                *failure = None;
                return Err(LauncherError::new(
                    "workflow_fixture_failure",
                    "The fixture stage failed.",
                    None,
                    true,
                ));
            }
            Ok(())
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().expect("calls lock").clone()
        }
    }

    #[async_trait]
    impl WorkflowBackend for MockBackend {
        fn game_active(&self) -> Result<bool, LauncherError> {
            Ok(false)
        }

        async fn authenticate(&self) -> Result<LaunchAccount, LauncherError> {
            self.record("auth", WorkflowStage::Authenticating)?;
            self.maybe_fail(FailurePoint::Auth)?;
            Ok(LaunchAccount::new("Player", "uuid", "access-secret"))
        }

        async fn metadata(
            &self,
            _profile_id: &str,
        ) -> Result<(LauncherProfile, ResolvedVersion), LauncherError> {
            self.record("metadata", WorkflowStage::ResolvingMetadata)?;
            self.maybe_fail(FailurePoint::Metadata)?;
            Ok((fixture_profile(), fixture_version()))
        }

        async fn runtime(
            &self,
            _profile: &LauncherProfile,
            _version: &ResolvedVersion,
        ) -> Result<JavaRuntimeStatus, LauncherError> {
            self.record("runtime", WorkflowStage::ResolvingJava)?;
            self.maybe_fail(FailurePoint::Runtime)?;
            Ok(JavaRuntimeStatus {
                requirement: 21,
                state: JavaRuntimeState::Valid,
                path: Some(PathBuf::from("java.exe")),
                source: Some(JavaRuntimeSource::Managed),
                version: Some("21".to_owned()),
            })
        }

        async fn installation_verified(
            &self,
            _version: &ResolvedVersion,
        ) -> Result<bool, LauncherError> {
            self.calls.lock().expect("calls lock").push("verify");
            Ok(*self.installed.lock().expect("installed lock"))
        }

        async fn install(
            &self,
            operation_id: &str,
            _version: &ResolvedVersion,
            cancel: DownloadCancellationToken,
            progress: Arc<dyn ProgressSink>,
        ) -> Result<(), LauncherError> {
            self.record("install", WorkflowStage::Installing)?;
            progress.emit(DownloadProgress {
                operation_id: operation_id.to_owned(),
                completed_bytes: 5,
                total_bytes: 10,
                current_file: PathBuf::from(r"C:\private\client.jar"),
            });
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            self.maybe_fail(FailurePoint::Install)?;
            *self.installed.lock().expect("installed lock") = true;
            Ok(())
        }

        fn build(
            &self,
            _account: LaunchAccount,
            _profile: LauncherProfile,
            _version: ResolvedVersion,
            _runtime: JavaRuntimeStatus,
        ) -> Result<PreparedLaunch, LauncherError> {
            self.record("build", WorkflowStage::Launching)?;
            self.maybe_fail(FailurePoint::Build)?;
            Ok(PreparedLaunch {
                command: LaunchCommand {
                    executable: PathBuf::from("java.exe"),
                    args: vec![OsString::from("safe")],
                    cwd: PathBuf::from("game"),
                },
                validated_paths: Vec::new(),
                secrets: vec!["access-secret".to_owned()],
            })
        }

        async fn spawn(
            &self,
            _profile_id: &str,
            operation_id: &str,
            _prepared: PreparedLaunch,
        ) -> Result<(), LauncherError> {
            self.calls.lock().expect("calls lock").push("spawn");
            if let Some(operations) = self
                .terminalize_spawn
                .lock()
                .expect("terminal registry lock")
                .as_ref()
            {
                operations.finish(
                    operation_id,
                    OperationState::Failed,
                    None,
                    Some(LauncherError::new(
                        "process_spawn_failed",
                        "The game process could not start.",
                        None,
                        true,
                    )),
                )?;
            }
            self.maybe_fail(FailurePoint::Spawn)
        }
    }

    fn fixture_profile() -> LauncherProfile {
        LauncherProfile {
            id: "default".to_owned(),
            name: "Default".to_owned(),
            version_id: Some("1.21.8".to_owned()),
            memory_mb: 4096,
            game_dir: "game".to_owned(),
            java_override: None,
        }
    }

    fn fixture_version() -> ResolvedVersion {
        ResolvedVersion {
            id: "1.21.8".to_owned(),
            main_class: Some("net.minecraft.client.main.Main".to_owned()),
            assets: None,
            asset_index: None,
            downloads: VersionDownloads::default(),
            libraries: Vec::new(),
            logging: None,
            java_version: None,
            arguments: VersionArguments::default(),
            minecraft_arguments: None,
        }
    }

    fn cancelled() -> LauncherError {
        LauncherError::new(
            "download_cancelled",
            "The operation was cancelled.",
            None,
            true,
        )
    }

    fn execute(
        backend: Arc<MockBackend>,
        events: Arc<RecordingEvents>,
        operations: OperationRegistry,
    ) -> Result<(String, OperationState), LauncherError> {
        tauri::async_runtime::block_on(async {
            let orchestrator = LaunchOrchestrator::new(backend.clone(), operations.clone(), events);
            let handle = orchestrator.reserve("default")?;
            backend.attach_cancel(handle.cancel_token.clone());
            let operation_id = handle.operation_id.clone();
            orchestrator.execute("default".to_owned(), handle).await?;
            Ok((
                operation_id.clone(),
                operations.workflow_status(&operation_id)?.state,
            ))
        })
    }

    #[test]
    fn missing_install_runs_exact_safe_stage_order_and_keeps_operation_active_after_spawn() {
        let backend = Arc::new(MockBackend::new(false));
        let events = Arc::new(RecordingEvents::default());
        let operations = OperationRegistry::default();

        let (operation_id, state) =
            execute(backend.clone(), events.clone(), operations).expect("workflow reaches spawn");

        assert_eq!(
            backend.calls(),
            ["auth", "metadata", "runtime", "verify", "install", "build", "spawn"]
        );
        assert_eq!(state, OperationState::Running);
        let progress: Vec<_> = events
            .snapshot()
            .into_iter()
            .filter_map(|event| match event {
                WorkflowEvent::Progress(progress) => Some(progress),
                WorkflowEvent::Error(_) => None,
            })
            .collect();
        assert!(progress
            .iter()
            .all(|event| event.operation_id == operation_id));
        assert_eq!(
            progress.iter().map(|event| event.stage).collect::<Vec<_>>(),
            [
                WorkflowStage::Authenticating,
                WorkflowStage::ResolvingMetadata,
                WorkflowStage::ResolvingJava,
                WorkflowStage::Installing,
                WorkflowStage::Installing,
                WorkflowStage::Launching,
            ]
        );
        assert_eq!(progress[4].current_file.as_deref(), Some("client.jar"));
    }

    #[test]
    fn verified_install_skips_installer_without_changing_remaining_order() {
        let backend = Arc::new(MockBackend::new(true));
        execute(
            backend.clone(),
            Arc::new(RecordingEvents::default()),
            OperationRegistry::default(),
        )
        .expect("verified workflow reaches spawn");
        assert_eq!(
            backend.calls(),
            ["auth", "metadata", "runtime", "verify", "build", "spawn"]
        );
    }

    #[test]
    fn cancellation_during_each_long_prelaunch_stage_stops_all_later_stages() {
        let cases = [
            (WorkflowStage::ResolvingMetadata, vec!["auth", "metadata"]),
            (
                WorkflowStage::ResolvingJava,
                vec!["auth", "metadata", "runtime"],
            ),
            (
                WorkflowStage::Installing,
                vec!["auth", "metadata", "runtime", "verify", "install"],
            ),
        ];
        for (stage, expected_calls) in cases {
            let backend = Arc::new(MockBackend::cancelling(stage));
            let events = Arc::new(RecordingEvents::default());
            let operations = OperationRegistry::default();
            let error = execute(backend.clone(), events.clone(), operations.clone())
                .expect_err("cancellation is terminal");
            assert_eq!(error.code(), "download_cancelled");
            assert_eq!(backend.calls(), expected_calls, "stage {stage:?}");
            let event = events
                .snapshot()
                .into_iter()
                .find_map(|event| match event {
                    WorkflowEvent::Error(error) => Some(error),
                    WorkflowEvent::Progress(_) => None,
                })
                .expect("cancellation error event");
            assert_eq!(event.stage, stage);
        }
    }

    #[test]
    fn retry_after_failure_gets_a_new_id_clears_error_and_reuses_verified_install_state() {
        tauri::async_runtime::block_on(async {
            let backend = Arc::new(MockBackend::failing(FailurePoint::Spawn));
            let events = Arc::new(RecordingEvents::default());
            let operations = OperationRegistry::default();
            let orchestrator = LaunchOrchestrator::new(backend.clone(), operations.clone(), events);
            let first = orchestrator.reserve("default").expect("first reservation");
            let first_id = first.operation_id.clone();
            let first_error = orchestrator
                .execute("default".to_owned(), first)
                .await
                .expect_err("first spawn fails");
            assert_eq!(first_error.code(), "workflow_fixture_failure");
            assert_eq!(
                operations
                    .workflow_status(&first_id)
                    .expect("first status")
                    .state,
                OperationState::Failed
            );

            let second = orchestrator.reserve("default").expect("retry reservation");
            let second_id = second.operation_id.clone();
            assert_ne!(first_id, second_id);
            assert!(operations
                .workflow_status(&second_id)
                .expect("new status")
                .error
                .is_none());
            orchestrator
                .execute("default".to_owned(), second)
                .await
                .expect("retry reaches spawn");
            assert_eq!(
                backend.calls(),
                [
                    "auth", "metadata", "runtime", "verify", "install", "build", "spawn", "auth",
                    "metadata", "runtime", "verify", "build", "spawn",
                ]
            );
        });
    }

    #[test]
    fn duplicate_click_and_conflicting_install_are_rejected_by_the_same_registry() {
        let backend = Arc::new(MockBackend::new(false));
        let operations = OperationRegistry::default();
        let orchestrator = LaunchOrchestrator::new(
            backend,
            operations.clone(),
            Arc::new(RecordingEvents::default()),
        );
        let first = orchestrator.reserve("default").expect("first reservation");
        let duplicate = orchestrator
            .reserve("default")
            .expect_err("duplicate launch is rejected");
        assert_eq!(duplicate.code(), "game_already_running");
        let install = operations
            .begin("1.21.8")
            .expect_err("install conflicts with launch workflow");
        assert_eq!(install.code(), "operation_in_progress");
        operations
            .finish(
                &first.operation_id,
                OperationState::Cancelled,
                None,
                Some(cancelled()),
            )
            .expect("cleanup");
    }

    #[test]
    fn every_stage_failure_emits_its_stage_and_never_spawns_after_an_earlier_failure() {
        let cases = [
            (FailurePoint::Auth, WorkflowStage::Authenticating),
            (FailurePoint::Metadata, WorkflowStage::ResolvingMetadata),
            (FailurePoint::Runtime, WorkflowStage::ResolvingJava),
            (FailurePoint::Install, WorkflowStage::Installing),
            (FailurePoint::Build, WorkflowStage::Launching),
        ];
        for (failure, expected_stage) in cases {
            let backend = Arc::new(MockBackend::failing(failure));
            let events = Arc::new(RecordingEvents::default());
            let error = execute(
                backend.clone(),
                events.clone(),
                OperationRegistry::default(),
            )
            .expect_err("stage fails");
            assert_eq!(error.code(), "workflow_fixture_failure");
            assert!(!backend.calls().contains(&"spawn"));
            let terminal = events
                .snapshot()
                .into_iter()
                .filter_map(|event| match event {
                    WorkflowEvent::Error(event) => Some(event),
                    WorkflowEvent::Progress(_) => None,
                })
                .collect::<Vec<WorkflowErrorEvent>>();
            assert_eq!(terminal.len(), 1);
            assert_eq!(terminal[0].stage, expected_stage);
            assert!(!format!("{:?}", terminal[0]).contains("access-secret"));
        }
    }

    #[test]
    fn cancellation_after_spawn_is_idempotent_and_does_not_cancel_the_game() {
        let backend = Arc::new(MockBackend::new(true));
        let operations = OperationRegistry::default();
        let events = Arc::new(RecordingEvents::default());
        let (operation_id, state) =
            execute(backend, events, operations.clone()).expect("workflow reaches spawn");
        assert_eq!(state, OperationState::Running);

        operations.cancel(&operation_id).expect("first cancel");
        operations.cancel(&operation_id).expect("second cancel");

        assert_eq!(
            operations
                .workflow_status(&operation_id)
                .expect("status")
                .state,
            OperationState::Running
        );
    }

    #[test]
    fn cancellation_at_the_atomic_spawn_boundary_terminates_and_releases_the_reservation() {
        let operations = OperationRegistry::default();
        let events = Arc::new(RecordingEvents::default());
        let orchestrator = LaunchOrchestrator::new(
            Arc::new(MockBackend::new(true)),
            operations.clone(),
            events.clone(),
        );
        let handle = orchestrator.reserve("default").expect("reservation");
        operations
            .cancel(&handle.operation_id)
            .expect("race cancellation");

        let error = orchestrator
            .enter_spawn_boundary("default", &handle.operation_id)
            .expect_err("cancel wins before spawn boundary");

        assert_eq!(error.code(), "download_cancelled");
        assert_eq!(
            operations
                .workflow_status(&handle.operation_id)
                .expect("terminal status")
                .state,
            OperationState::Cancelled
        );
        assert!(orchestrator.reserve("default").is_ok());
        let errors = events
            .snapshot()
            .into_iter()
            .filter_map(|event| match event {
                WorkflowEvent::Error(error) => Some(error),
                WorkflowEvent::Progress(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].stage, WorkflowStage::Launching);
    }

    #[test]
    fn spawn_failure_emits_workflow_stage_even_if_process_sink_finished_the_registry_first() {
        let operations = OperationRegistry::default();
        let backend = Arc::new(MockBackend::failing(FailurePoint::Spawn));
        backend.terminalize_spawn_with(operations.clone());
        let events = Arc::new(RecordingEvents::default());

        let error = execute(backend, events.clone(), operations)
            .expect_err("spawn failure remains an orchestration failure");

        assert_eq!(error.code(), "workflow_fixture_failure");
        let errors = events
            .snapshot()
            .into_iter()
            .filter_map(|event| match event {
                WorkflowEvent::Error(error) => Some(error),
                WorkflowEvent::Progress(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].stage, WorkflowStage::Launching);
    }

    #[test]
    fn a_terminal_operation_cannot_be_finished_or_emit_a_terminal_transition_twice() {
        let operations = OperationRegistry::default();
        let handle = operations.begin_launch("default").expect("reservation");
        let first_error = LauncherError::new("first", "First.", None, true);
        operations
            .finish(
                &handle.operation_id,
                OperationState::Failed,
                None,
                Some(first_error),
            )
            .expect("first terminal transition");
        operations
            .finish(&handle.operation_id, OperationState::Completed, None, None)
            .expect("duplicate terminal transition is idempotent");

        let status = operations
            .workflow_status(&handle.operation_id)
            .expect("status");
        assert_eq!(status.state, OperationState::Failed);
        assert_eq!(status.error.expect("first error remains").code(), "first");
    }

    #[test]
    fn progress_stage_order_is_monotonic_and_nested_install_progress_cannot_regress_it() {
        let events = Arc::new(RecordingEvents::default());
        let progress = ProgressBridge::new("operation".to_owned(), events.clone());
        progress.advance(WorkflowStage::Installing);
        progress.advance(WorkflowStage::ResolvingMetadata);
        progress.emit(DownloadProgress {
            operation_id: "operation".to_owned(),
            completed_bytes: 4,
            total_bytes: 8,
            current_file: PathBuf::from("library.jar"),
        });
        progress.advance(WorkflowStage::Launching);
        progress.emit(DownloadProgress {
            operation_id: "operation".to_owned(),
            completed_bytes: 8,
            total_bytes: 8,
            current_file: PathBuf::from("late.jar"),
        });

        let stages = events
            .snapshot()
            .into_iter()
            .filter_map(|event| match event {
                WorkflowEvent::Progress(progress) => Some(progress.stage),
                WorkflowEvent::Error(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            stages,
            [
                WorkflowStage::Installing,
                WorkflowStage::Installing,
                WorkflowStage::Launching
            ]
        );
    }
}
