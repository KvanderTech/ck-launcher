use crate::events::EventBus;
use crate::{
    error::LauncherError,
    installer::{OperationRegistry, OperationState},
    launcher::{
        process::{EventSink, GameProcessEvent},
        GameProcessStatus, Launcher, OperationId,
    },
    orchestration::{LaunchOrchestrator, WorkflowEvent, WorkflowEventSink},
    paths::AppPaths,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const GAME_STARTED_EVENT: &str = "launcher://game-started";
pub const GAME_EXITED_EVENT: &str = "launcher://game-exited";
pub const ERROR_EVENT: &str = "launcher://error";

pub(crate) struct GameEventSink {
    app: EventBus,
    operations: OperationRegistry,
}

impl GameEventSink {
    pub(crate) fn new(app: EventBus, operations: OperationRegistry) -> Arc<Self> {
        Arc::new(Self { app, operations })
    }
}

impl EventSink for GameEventSink {
    fn emit(&self, event: GameProcessEvent) {
        match &event {
            GameProcessEvent::Exited { operation_id, .. } => {
                let _ = self
                    .operations
                    .finish(operation_id, OperationState::Completed, None, None);
            }
            GameProcessEvent::Error {
                operation_id,
                error,
                terminal: true,
                ..
            } => {
                let _ = self.operations.finish(
                    operation_id,
                    OperationState::Failed,
                    None,
                    Some(error.clone()),
                );
            }
            GameProcessEvent::Started { .. } | GameProcessEvent::Error { .. } => {}
        }
        let name = match &event {
            GameProcessEvent::Started { .. } => GAME_STARTED_EVENT,
            GameProcessEvent::Exited { .. } => GAME_EXITED_EVENT,
            GameProcessEvent::Error { .. } => ERROR_EVENT,
        };
        let _ = self.app.emit(name, event);
    }
}

pub(crate) struct LauncherWorkflowEventSink {
    app: EventBus,
}

impl LauncherWorkflowEventSink {
    pub(crate) fn new(app: EventBus) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl WorkflowEventSink for LauncherWorkflowEventSink {
    fn emit(&self, event: WorkflowEvent) {
        match event {
            WorkflowEvent::Progress(progress) => {
                let _ = self
                    .app
                    .emit(crate::commands::install::PROGRESS_EVENT, progress);
            }
            WorkflowEvent::Error(error) => {
                let _ = self.app.emit(ERROR_EVENT, error);
            }
        }
    }
}

pub(crate) async fn launch_or_install(
    profile_id: String,
    orchestrator: &LaunchOrchestrator,
) -> Result<OperationId, LauncherError> {
    let handle = orchestrator.reserve(&profile_id)?;
    let operation_id = handle.operation_id.clone();
    let orchestrator = orchestrator.clone();
    crate::tasks::spawn(async move {
        let _ = orchestrator.execute(profile_id, handle).await;
    });
    Ok(operation_id)
}

pub async fn launch(profile_id: String, launcher: &Launcher) -> Result<OperationId, LauncherError> {
    launcher.launch(&profile_id).await
}

pub async fn launch_status(
    operation_id: String,
    launcher: &Launcher,
) -> Result<GameProcessStatus, LauncherError> {
    launcher.status(&operation_id)
}

pub async fn stop_game(operation_id: String, launcher: &Launcher) -> Result<(), LauncherError> {
    launcher.stop(&operation_id)
}

pub async fn open_latest_game_log(paths: &AppPaths) -> Result<(), LauncherError> {
    let path = latest_game_log_path(&paths)?;
    open_log_file(&path)
}

pub async fn read_latest_game_log(paths: &AppPaths) -> Result<String, LauncherError> {
    let path = latest_game_log_path(&paths)?;
    crate::logs::read_tail(&path)
}

fn latest_game_log_path(paths: &AppPaths) -> Result<PathBuf, LauncherError> {
    let path = paths.safe_join(&paths.logs, Path::new("latest.log"))?;
    if !path
        .metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return Err(LauncherError::new(
            "game_log_not_found",
            "No sanitized Minecraft log is available yet.",
            None,
            true,
        ));
    }
    Ok(path)
}

#[cfg(windows)]
fn open_log_file(path: &Path) -> Result<(), LauncherError> {
    use std::{iter::once, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let operation: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(once(0))
        .collect();
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        return Err(LauncherError::new(
            "game_log_open_failed",
            "The sanitized Minecraft log could not be opened.",
            None,
            true,
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_log_file(_path: &Path) -> Result<(), LauncherError> {
    Err(LauncherError::new(
        "game_log_open_unavailable",
        "Opening the Minecraft log is available only on Windows.",
        None,
        false,
    ))
}
