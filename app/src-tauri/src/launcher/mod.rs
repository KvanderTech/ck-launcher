mod arguments;
mod classpath;
pub(crate) mod process;

use crate::{
    auth::AuthService,
    error::LauncherError,
    metadata::models::ResolvedVersion,
    metadata::resolver::MetadataService,
    profiles::{clamp_memory, validated_profile_game_directory, PhysicalMemory},
    runtime::{requirement_for_version, JavaRuntimeState, JavaRuntimeStatus, RuntimeManager},
    storage::{LauncherProfile, ProfileStore},
};
use arguments::{resolve_legacy, resolve_modern, safe_metadata_jvm};
use async_trait::async_trait;
use classpath::{build_classpath, validate_directory, validate_regular_file};
use process::{EventSink, GameProcessEvent, ProcessLog, ProcessSpawner};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub type OperationId = String;

#[async_trait]
pub(crate) trait LaunchContextProvider: Send + Sync {
    async fn prepare(&self, profile_id: &str) -> Result<PreparedLaunch, LauncherError>;
}

pub(crate) trait LaunchPathInspector: Send + Sync {
    fn validate(&self, path: &Path) -> Result<(), LauncherError>;
}

struct FileSystemLaunchPathInspector;

impl LaunchPathInspector for FileSystemLaunchPathInspector {
    fn validate(&self, path: &Path) -> Result<(), LauncherError> {
        if path.is_dir() {
            validate_directory(path)
        } else {
            validate_regular_file(path)
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProcessStatus {
    pub operation_id: String,
    pub profile_id: String,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub error: Option<LauncherError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Launcher {
    context: Arc<dyn LaunchContextProvider>,
    spawner: Arc<dyn ProcessSpawner>,
    events: Arc<dyn EventSink>,
    logs_root: PathBuf,
    path_inspector: Arc<dyn LaunchPathInspector>,
    registry: Arc<Mutex<ProcessRegistry>>,
}

#[derive(Default)]
struct ProcessRegistry {
    operations: HashMap<String, GameProcessStatus>,
    active_profiles: HashMap<String, String>,
    stopping: HashSet<String>,
    terminal_order: VecDeque<String>,
}

const MAX_TERMINAL_PROCESSES: usize = 128;

impl Launcher {
    pub(crate) fn new(
        context: Arc<dyn LaunchContextProvider>,
        spawner: Arc<dyn ProcessSpawner>,
        events: Arc<dyn EventSink>,
        logs_root: PathBuf,
    ) -> Self {
        Self::with_path_inspector(
            context,
            spawner,
            events,
            logs_root,
            Arc::new(FileSystemLaunchPathInspector),
        )
    }

    fn with_path_inspector(
        context: Arc<dyn LaunchContextProvider>,
        spawner: Arc<dyn ProcessSpawner>,
        events: Arc<dyn EventSink>,
        logs_root: PathBuf,
        path_inspector: Arc<dyn LaunchPathInspector>,
    ) -> Self {
        Self {
            context,
            spawner,
            events,
            logs_root,
            path_inspector,
            registry: Arc::new(Mutex::new(ProcessRegistry::default())),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_path_inspector(
        context: Arc<dyn LaunchContextProvider>,
        spawner: Arc<dyn ProcessSpawner>,
        events: Arc<dyn EventSink>,
        logs_root: PathBuf,
        path_inspector: Arc<dyn LaunchPathInspector>,
    ) -> Self {
        Self::with_path_inspector(context, spawner, events, logs_root, path_inspector)
    }

    pub(crate) fn production(
        context: Arc<dyn LaunchContextProvider>,
        events: Arc<dyn EventSink>,
        logs_root: PathBuf,
    ) -> Self {
        Self::new(
            context,
            Arc::new(process::TokioProcessSpawner),
            events,
            logs_root,
        )
    }

    pub async fn launch(&self, profile_id: &str) -> Result<OperationId, LauncherError> {
        validate_profile_id(profile_id)?;
        let operation_id = format!("launch-{:016x}", rand::random::<u64>());
        self.reserve(profile_id, &operation_id)?;
        let result = self.start_reserved(profile_id, &operation_id).await;
        if let Err(error) = &result {
            self.finish_error(profile_id, &operation_id, error.clone())?;
        }
        result.map(|()| operation_id)
    }

    pub(crate) fn game_active(&self) -> Result<bool, LauncherError> {
        Ok(!self
            .registry
            .lock()
            .map_err(|_| process_state_error())?
            .active_profiles
            .is_empty())
    }

    pub(crate) async fn launch_prepared(
        &self,
        profile_id: &str,
        operation_id: &str,
        prepared: PreparedLaunch,
    ) -> Result<(), LauncherError> {
        validate_profile_id(profile_id)?;
        self.reserve(profile_id, operation_id)?;
        let result = self
            .start_prepared(profile_id, operation_id, prepared)
            .await;
        if let Err(error) = &result {
            finish_registry(
                &self.registry,
                profile_id,
                operation_id,
                None,
                Some(error.clone()),
            )?;
        }
        result
    }

    fn reserve(&self, profile_id: &str, operation_id: &str) -> Result<(), LauncherError> {
        {
            let mut registry = self.registry.lock().map_err(|_| process_state_error())?;
            if !registry.active_profiles.is_empty() {
                return Err(LauncherError::new(
                    "game_already_running",
                    "Minecraft is already running.",
                    None,
                    true,
                ));
            }
            registry
                .active_profiles
                .insert(profile_id.to_owned(), operation_id.to_owned());
            registry.operations.insert(
                operation_id.to_owned(),
                GameProcessStatus {
                    operation_id: operation_id.to_owned(),
                    profile_id: profile_id.to_owned(),
                    pid: None,
                    exit_code: None,
                    error: None,
                    log_path: None,
                },
            );
        }
        Ok(())
    }

    async fn start_reserved(
        &self,
        profile_id: &str,
        operation_id: &str,
    ) -> Result<(), LauncherError> {
        let prepared = self.context.prepare(profile_id).await?;
        self.start_prepared(profile_id, operation_id, prepared)
            .await
    }

    async fn start_prepared(
        &self,
        profile_id: &str,
        operation_id: &str,
        prepared: PreparedLaunch,
    ) -> Result<(), LauncherError> {
        let log = ProcessLog::open(&self.logs_root, prepared.secrets.clone())?;
        let log_path = self.logs_root.join("latest.log");
        self.registry
            .lock()
            .map_err(|_| process_state_error())?
            .operations
            .get_mut(operation_id)
            .ok_or_else(process_state_error)?
            .log_path = Some(log_path.clone());
        for path in &prepared.validated_paths {
            self.path_inspector.validate(path)?;
        }
        let child = self.spawner.spawn(prepared.command).await?;
        let pid = child.pid();
        self.registry
            .lock()
            .map_err(|_| process_state_error())?
            .operations
            .get_mut(operation_id)
            .ok_or_else(process_state_error)?
            .pid = Some(pid);
        self.events.emit(GameProcessEvent::Started {
            operation_id: operation_id.to_owned(),
            profile_id: profile_id.to_owned(),
            pid,
        });
        let registry = self.registry.clone();
        let events = self.events.clone();
        let operation_id = operation_id.to_owned();
        let profile_id = profile_id.to_owned();
        tauri::async_runtime::spawn(async move {
            match child.wait(log).await {
                Ok(outcome) => {
                    if stop_was_requested(&registry, &operation_id).unwrap_or(false) {
                        let _ = finish_registry(
                            &registry,
                            &profile_id,
                            &operation_id,
                            Some(outcome.exit_code),
                            outcome.auxiliary_error,
                        );
                        events.emit(GameProcessEvent::Exited {
                            operation_id,
                            profile_id,
                            exit_code: outcome.exit_code,
                        });
                        return;
                    }
                    if outcome.exit_code != 0 {
                        let error = game_exit_error(outcome.exit_code);
                        let _ = finish_registry(
                            &registry,
                            &profile_id,
                            &operation_id,
                            Some(outcome.exit_code),
                            Some(error.clone()),
                        );
                        if let Some(auxiliary) = outcome.auxiliary_error {
                            events.emit(GameProcessEvent::Error {
                                operation_id: operation_id.clone(),
                                profile_id: profile_id.clone(),
                                error: auxiliary,
                                terminal: false,
                                log_path: Some(log_path.clone()),
                            });
                        }
                        events.emit(GameProcessEvent::Error {
                            operation_id,
                            profile_id,
                            error,
                            terminal: true,
                            log_path: Some(log_path),
                        });
                        return;
                    }
                    let _ = finish_registry(
                        &registry,
                        &profile_id,
                        &operation_id,
                        Some(outcome.exit_code),
                        outcome.auxiliary_error.clone(),
                    );
                    if let Some(error) = outcome.auxiliary_error {
                        events.emit(GameProcessEvent::Error {
                            operation_id: operation_id.clone(),
                            profile_id: profile_id.clone(),
                            error,
                            terminal: false,
                            log_path: Some(log_path.clone()),
                        });
                    }
                    events.emit(GameProcessEvent::Exited {
                        operation_id,
                        profile_id,
                        exit_code: outcome.exit_code,
                    });
                }
                Err(error) => {
                    let _ = finish_registry(
                        &registry,
                        &profile_id,
                        &operation_id,
                        None,
                        Some(error.clone()),
                    );
                    events.emit(GameProcessEvent::Error {
                        operation_id,
                        profile_id,
                        error,
                        terminal: true,
                        log_path: Some(log_path),
                    });
                }
            }
        });
        Ok(())
    }

    fn finish_error(
        &self,
        profile_id: &str,
        operation_id: &str,
        error: LauncherError,
    ) -> Result<(), LauncherError> {
        finish_registry(
            &self.registry,
            profile_id,
            operation_id,
            None,
            Some(error.clone()),
        )?;
        self.events.emit(GameProcessEvent::Error {
            operation_id: operation_id.to_owned(),
            profile_id: profile_id.to_owned(),
            error,
            terminal: true,
            log_path: self
                .registry
                .lock()
                .ok()
                .and_then(|registry| registry.operations.get(operation_id)?.log_path.clone()),
        });
        Ok(())
    }

    pub fn status(&self, operation_id: &str) -> Result<GameProcessStatus, LauncherError> {
        self.registry
            .lock()
            .map_err(|_| process_state_error())?
            .operations
            .get(operation_id)
            .cloned()
            .ok_or_else(|| {
                LauncherError::new(
                    "operation_not_found",
                    "The launch operation was not found.",
                    None,
                    false,
                )
            })
    }

    pub fn stop(&self, operation_id: &str) -> Result<(), LauncherError> {
        let pid = {
            let mut registry = self.registry.lock().map_err(|_| process_state_error())?;
            let status = registry.operations.get(operation_id).ok_or_else(|| {
                LauncherError::new(
                    "operation_not_found",
                    "The launch operation was not found.",
                    None,
                    true,
                )
            })?;
            let active = registry
                .active_profiles
                .get(&status.profile_id)
                .is_some_and(|active| active == operation_id);
            let pid = status
                .pid
                .filter(|pid| *pid > 0)
                .filter(|_| active)
                .ok_or_else(|| {
                    LauncherError::new(
                        "game_not_running",
                        "Minecraft is no longer running.",
                        None,
                        true,
                    )
                })?;
            registry.stopping.insert(operation_id.to_owned());
            pid
        };
        if let Err(error) = terminate_process(pid) {
            if let Ok(mut registry) = self.registry.lock() {
                registry.stopping.remove(operation_id);
            }
            return Err(error);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn active_count(&self) -> Result<usize, LauncherError> {
        Ok(self
            .registry
            .lock()
            .map_err(|_| process_state_error())?
            .active_profiles
            .len())
    }
}

fn game_exit_error(exit_code: i32) -> LauncherError {
    LauncherError::new(
        "game_exit",
        format!("Minecraft exited with code {exit_code}."),
        Some(format!("Minecraft exited with code {exit_code}.")),
        true,
    )
}

fn finish_registry(
    registry: &Arc<Mutex<ProcessRegistry>>,
    profile_id: &str,
    operation_id: &str,
    exit_code: Option<i32>,
    error: Option<LauncherError>,
) -> Result<(), LauncherError> {
    let mut registry = registry.lock().map_err(|_| process_state_error())?;
    if let Some(status) = registry.operations.get_mut(operation_id) {
        status.exit_code = exit_code;
        status.error = error;
    }
    registry.active_profiles.remove(profile_id);
    registry.stopping.remove(operation_id);
    registry.terminal_order.push_back(operation_id.to_owned());
    while registry.terminal_order.len() > MAX_TERMINAL_PROCESSES {
        if let Some(expired) = registry.terminal_order.pop_front() {
            registry.operations.remove(&expired);
        }
    }
    Ok(())
}

fn stop_was_requested(
    registry: &Arc<Mutex<ProcessRegistry>>,
    operation_id: &str,
) -> Result<bool, LauncherError> {
    Ok(registry
        .lock()
        .map_err(|_| process_state_error())?
        .stopping
        .contains(operation_id))
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> Result<(), LauncherError> {
    let result = std::process::Command::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
    if result.is_ok_and(|status| status.success()) {
        return Ok(());
    }
    Err(LauncherError::new(
        "game_stop_failed",
        "Minecraft could not be stopped.",
        None,
        true,
    ))
}

#[cfg(not(windows))]
fn terminate_process(_pid: u32) -> Result<(), LauncherError> {
    Err(LauncherError::new(
        "game_stop_unavailable",
        "Stopping Minecraft is available only on Windows.",
        None,
        false,
    ))
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

fn process_state_error() -> LauncherError {
    LauncherError::new(
        "process_state_unavailable",
        "Minecraft process state is unavailable.",
        None,
        true,
    )
}

pub(crate) struct ProductionLaunchContext {
    auth: Arc<AuthService>,
    profiles: Arc<dyn ProfileStore>,
    metadata: Arc<MetadataService>,
    runtimes: Arc<RuntimeManager>,
    memory: Arc<dyn PhysicalMemory>,
}

impl ProductionLaunchContext {
    pub(crate) fn new(
        auth: Arc<AuthService>,
        profiles: Arc<dyn ProfileStore>,
        metadata: Arc<MetadataService>,
        runtimes: Arc<RuntimeManager>,
        memory: Arc<dyn PhysicalMemory>,
    ) -> Self {
        Self {
            auth,
            profiles,
            metadata,
            runtimes,
            memory,
        }
    }
}

#[async_trait]
impl LaunchContextProvider for ProductionLaunchContext {
    async fn prepare(&self, profile_id: &str) -> Result<PreparedLaunch, LauncherError> {
        let profile = self
            .profiles
            .active_profile()
            .await?
            .filter(|profile| profile.id == profile_id)
            .ok_or_else(|| {
                LauncherError::new(
                    "profile_not_found",
                    "The launcher profile was not found.",
                    None,
                    false,
                )
            })?;
        let version_id = profile.version_id.as_deref().ok_or_else(|| {
            LauncherError::new(
                "version_required",
                "Choose a Minecraft version before launching.",
                None,
                true,
            )
        })?;
        let game_root = validated_profile_game_directory(&profile)?;
        let version = self.metadata.resolved_version(version_id).await?;
        let requirement = requirement_for_version(&version)?;
        let runtime = self
            .runtimes
            .resolve(
                requirement,
                profile.java_override.as_ref().map(PathBuf::from),
            )
            .await?;
        let (account, access_token) = self
            .auth
            .refresh_active_minecraft_account()
            .await?
            .into_parts();
        build_launch(LaunchBuildRequest {
            account: LaunchAccount::new(
                account.minecraft_name,
                account.minecraft_uuid,
                access_token,
            ),
            version,
            profile,
            runtime,
            game_root,
            physical_memory_mb: self.memory.physical_memory_mb(),
        })
    }
}

pub struct LaunchCommand {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}

impl fmt::Debug for LaunchCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LaunchCommand")
            .field("executable", &self.executable)
            .field("args", &"[REDACTED]")
            .field("arg_count", &self.args.len())
            .field("cwd", &self.cwd)
            .finish()
    }
}

impl PartialEq for LaunchCommand {
    fn eq(&self, other: &Self) -> bool {
        self.executable == other.executable && self.args == other.args && self.cwd == other.cwd
    }
}

impl Eq for LaunchCommand {}

pub(crate) struct LaunchAccount {
    player_name: String,
    uuid: String,
    access_token: String,
}

impl LaunchAccount {
    pub(crate) fn new(
        player_name: impl Into<String>,
        uuid: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Self {
        Self {
            player_name: player_name.into(),
            uuid: uuid.into(),
            access_token: access_token.into(),
        }
    }
}

pub(crate) struct LaunchBuildRequest {
    pub account: LaunchAccount,
    pub version: ResolvedVersion,
    pub profile: LauncherProfile,
    pub runtime: JavaRuntimeStatus,
    pub game_root: PathBuf,
    pub physical_memory_mb: u64,
}

pub(crate) struct PreparedLaunch {
    pub command: LaunchCommand,
    pub validated_paths: Vec<PathBuf>,
    pub secrets: Vec<String>,
}

impl fmt::Debug for PreparedLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedLaunch")
            .field("command", &self.command)
            .field("validated_paths", &self.validated_paths)
            .field("secrets", &"[REDACTED]")
            .finish()
    }
}

pub(crate) fn build_launch(request: LaunchBuildRequest) -> Result<PreparedLaunch, LauncherError> {
    let executable = request
        .runtime
        .path
        .clone()
        .filter(|_| request.runtime.state == JavaRuntimeState::Valid)
        .ok_or_else(runtime_unavailable)?;
    validate_regular_file(&executable)?;
    validate_directory(&request.game_root)?;
    let cwd = PathBuf::from(&request.profile.game_dir);
    validate_directory(&cwd)?;
    if cwd.canonicalize().map_err(|_| invalid_launch_path())?
        != request
            .game_root
            .canonicalize()
            .map_err(|_| invalid_launch_path())?
    {
        return Err(invalid_launch_path());
    }
    let (classpath, mut validated_paths) = build_classpath(&request.game_root, &request.version)?;
    let natives = request
        .game_root
        .join("versions")
        .join(&request.version.id)
        .join("natives");
    validate_directory(&natives)?;
    let assets_root = request.game_root.join("assets");
    validate_directory(&assets_root)?;
    let asset_index = request
        .version
        .asset_index
        .as_ref()
        .map(|index| index.id.clone())
        .or_else(|| request.version.assets.clone())
        .unwrap_or_default();
    let natives_argument = natives.to_string_lossy().into_owned();
    let launcher_name = "CKLauncher";
    let launcher_version = env!("CARGO_PKG_VERSION");
    let mut variables = BTreeMap::from([
        ("auth_player_name", request.account.player_name.clone()),
        ("auth_uuid", request.account.uuid.clone()),
        ("auth_access_token", request.account.access_token.clone()),
        ("version_name", request.version.id.clone()),
        ("game_directory", cwd.to_string_lossy().into_owned()),
        ("assets_root", assets_root.to_string_lossy().into_owned()),
        ("assets_index_name", asset_index),
        ("natives_directory", natives_argument.clone()),
        ("classpath", classpath.clone()),
        ("launcher_name", launcher_name.to_owned()),
        ("launcher_version", launcher_version.to_owned()),
        (
            "user_type",
            if request.account.access_token == "0" {
                "legacy".to_owned()
            } else {
                "msa".to_owned()
            },
        ),
        ("version_type", "release".to_owned()),
        ("auth_xuid", String::new()),
        ("clientid", String::new()),
    ]);
    let modern_jvm = resolve_modern(&request.version.arguments.jvm, &variables)?;
    let mut args = safe_metadata_jvm(
        modern_jvm,
        &classpath,
        &natives_argument,
        launcher_name,
        launcher_version,
    )?;
    args.push("-Xms512M".to_owned());
    args.push(format!(
        "-Xmx{}M",
        clamp_memory(request.profile.memory_mb, request.physical_memory_mb)
    ));
    args.push(format!("-Djava.library.path={natives_argument}"));
    if let Some((argument, path)) = logging_argument(&request.game_root, &request.version)? {
        args.push(argument);
        validated_paths.push(path);
    }
    args.push("-cp".to_owned());
    args.push(classpath);
    let main_class = request
        .version
        .main_class
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(argument_invalid)?;
    if !is_qualified_java_class(main_class) {
        return Err(argument_invalid());
    }
    args.push(main_class.to_owned());
    let game_args = if request.version.arguments.game.is_empty() {
        request
            .version
            .minecraft_arguments
            .as_deref()
            .map(|legacy| resolve_legacy(legacy, &variables))
            .transpose()?
            .unwrap_or_default()
    } else {
        resolve_modern(&request.version.arguments.game, &variables)?
    };
    args.extend(game_args);
    validated_paths.extend([executable.clone(), cwd.clone(), natives, assets_root]);
    variables.clear();
    Ok(PreparedLaunch {
        command: LaunchCommand {
            executable,
            args: args.into_iter().map(OsString::from).collect(),
            cwd,
        },
        validated_paths,
        secrets: vec![request.account.access_token],
    })
}

fn is_qualified_java_class(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|segment| {
            let mut bytes = segment.bytes();
            bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
                && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        })
}

fn logging_argument(
    game_root: &Path,
    version: &ResolvedVersion,
) -> Result<Option<(String, PathBuf)>, LauncherError> {
    let Some(client) = version
        .logging
        .as_ref()
        .and_then(|value| value.get("client"))
    else {
        return Ok(None);
    };
    let argument = client
        .get("argument")
        .and_then(|value| value.as_str())
        .ok_or_else(argument_invalid)?;
    if argument != "-Dlog4j.configurationFile=${path}" {
        return Err(argument_invalid());
    }
    let id = client
        .get("file")
        .and_then(|value| value.get("id"))
        .and_then(|value| value.as_str())
        .ok_or_else(argument_invalid)?;
    if id.is_empty() || Path::new(id).components().count() != 1 {
        return Err(argument_invalid());
    }
    let path = game_root.join("assets").join("log_configs").join(id);
    validate_regular_file(&path)?;
    let resolved = format!("-Dlog4j.configurationFile={}", path.to_string_lossy());
    Ok(Some((resolved, path)))
}

fn runtime_unavailable() -> LauncherError {
    LauncherError::new(
        "runtime_unavailable",
        "A compatible Java runtime is required.",
        None,
        true,
    )
}

fn invalid_launch_path() -> LauncherError {
    LauncherError::new(
        "invalid_launch_path",
        "A required Minecraft launch path is missing or unsafe.",
        None,
        false,
    )
}

fn argument_invalid() -> LauncherError {
    LauncherError::new(
        "launch_argument_invalid",
        "Minecraft launch metadata contains an unsupported argument.",
        None,
        false,
    )
}

#[cfg(test)]
mod tests;
