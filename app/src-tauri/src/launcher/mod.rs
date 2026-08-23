mod arguments;
mod classpath;
pub(crate) mod process;

use crate::{
    auth::AuthService,
    error::LauncherError,
    metadata::models::ResolvedVersion,
    metadata::resolver::MetadataService,
    paths::AppPaths,
    profiles::{clamp_memory, PhysicalMemory},
    runtime::{JavaRequirement, JavaRuntimeState, JavaRuntimeStatus, RuntimeManager},
    storage::{LauncherProfile, ProfileStore},
};
use arguments::{resolve_legacy, resolve_modern, safe_metadata_jvm};
use async_trait::async_trait;
use classpath::{build_classpath, validate_directory, validate_regular_file};
use process::{EventSink, GameProcessEvent, ProcessLog, ProcessSpawner};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
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
}

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
                .insert(profile_id.to_owned(), operation_id.clone());
            registry.operations.insert(
                operation_id.clone(),
                GameProcessStatus {
                    operation_id: operation_id.clone(),
                    profile_id: profile_id.to_owned(),
                    pid: None,
                    exit_code: None,
                    error: None,
                },
            );
        }
        let result = self.start_reserved(profile_id, &operation_id).await;
        if let Err(error) = &result {
            self.finish_error(profile_id, &operation_id, error.clone())?;
        }
        result.map(|()| operation_id)
    }

    async fn start_reserved(
        &self,
        profile_id: &str,
        operation_id: &str,
    ) -> Result<(), LauncherError> {
        let prepared = self.context.prepare(profile_id).await?;
        let log = ProcessLog::open(&self.logs_root, prepared.secrets.clone())?;
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
    registry.terminal_order.push_back(operation_id.to_owned());
    while registry.terminal_order.len() > MAX_TERMINAL_PROCESSES {
        if let Some(expired) = registry.terminal_order.pop_front() {
            registry.operations.remove(&expired);
        }
    }
    Ok(())
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
    paths: AppPaths,
    memory: Arc<dyn PhysicalMemory>,
}

impl ProductionLaunchContext {
    pub(crate) fn new(
        auth: Arc<AuthService>,
        profiles: Arc<dyn ProfileStore>,
        metadata: Arc<MetadataService>,
        runtimes: Arc<RuntimeManager>,
        paths: AppPaths,
        memory: Arc<dyn PhysicalMemory>,
    ) -> Self {
        Self {
            auth,
            profiles,
            metadata,
            runtimes,
            paths,
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
        let version = self.metadata.resolved_version(version_id).await?;
        let major = version
            .java_version
            .as_ref()
            .map(|java| java.major_version)
            .unwrap_or(8);
        let requirement =
            JavaRequirement::new(u16::try_from(major).map_err(|_| runtime_unavailable())?)?;
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
            game_root: self.paths.game.clone(),
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
        ("user_type", "msa".to_owned()),
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
