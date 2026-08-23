use crate::{
    error::LauncherError,
    launcher::{
        process::{EventSink, GameProcessEvent},
        GameProcessStatus, Launcher, OperationId,
    },
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

pub const GAME_STARTED_EVENT: &str = "launcher://game-started";
pub const GAME_EXITED_EVENT: &str = "launcher://game-exited";
pub const ERROR_EVENT: &str = "launcher://error";

pub(crate) struct TauriGameEventSink {
    app: AppHandle,
}

impl TauriGameEventSink {
    pub(crate) fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl EventSink for TauriGameEventSink {
    fn emit(&self, event: GameProcessEvent) {
        let name = match &event {
            GameProcessEvent::Started { .. } => GAME_STARTED_EVENT,
            GameProcessEvent::Exited { .. } => GAME_EXITED_EVENT,
            GameProcessEvent::Error { .. } => ERROR_EVENT,
        };
        let _ = self.app.emit(name, event);
    }
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
