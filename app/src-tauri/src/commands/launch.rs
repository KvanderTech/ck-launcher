use crate::{
    error::LauncherError,
    installer::{OperationRegistry, OperationState},
    launcher::{
        process::{EventSink, GameProcessEvent},
        GameProcessStatus, Launcher, OperationId,
    },
    orchestration::{LaunchOrchestrator, WorkflowEvent, WorkflowEventSink},
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

pub const GAME_STARTED_EVENT: &str = "launcher://game-started";
pub const GAME_EXITED_EVENT: &str = "launcher://game-exited";
pub const ERROR_EVENT: &str = "launcher://error";

pub(crate) struct TauriGameEventSink {
    app: AppHandle,
    operations: OperationRegistry,
}

impl TauriGameEventSink {
    pub(crate) fn new(app: AppHandle, operations: OperationRegistry) -> Arc<Self> {
        Arc::new(Self { app, operations })
    }
}

impl EventSink for TauriGameEventSink {
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

pub(crate) struct TauriWorkflowEventSink {
    app: AppHandle,
}

impl TauriWorkflowEventSink {
    pub(crate) fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl WorkflowEventSink for TauriWorkflowEventSink {
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

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn launch_or_install(
    profile_id: String,
    orchestrator: State<'_, LaunchOrchestrator>,
) -> Result<OperationId, LauncherError> {
    let handle = orchestrator.reserve(&profile_id)?;
    let operation_id = handle.operation_id.clone();
    let orchestrator = orchestrator.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = orchestrator.execute(profile_id, handle).await;
    });
    Ok(operation_id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn launch(
    profile_id: String,
    launcher: State<'_, Launcher>,
) -> Result<OperationId, LauncherError> {
    launcher.launch(&profile_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn launch_status(
    operation_id: String,
    launcher: State<'_, Launcher>,
) -> Result<GameProcessStatus, LauncherError> {
    launcher.status(&operation_id)
}
