use crate::{
    downloads::{DownloadProgress, ProgressSink},
    error::LauncherError,
    installer::{InstallationStatus, Installer, OperationRegistry, OperationState},
};
use serde::Serialize;
use std::{path::Path, sync::Arc};
use tauri::{AppHandle, Emitter, State};

pub const PROGRESS_EVENT: &str = "launcher://progress";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherProgressEvent {
    operation_id: String,
    stage: &'static str,
    completed_bytes: u64,
    total_bytes: u64,
    current_file: Option<String>,
}

struct TauriProgressSink {
    app: AppHandle,
}

impl ProgressSink for TauriProgressSink {
    fn emit(&self, event: DownloadProgress) {
        let current_file = safe_progress_name(&event.current_file);
        let _ = self.app.emit(
            PROGRESS_EVENT,
            LauncherProgressEvent {
                operation_id: event.operation_id,
                stage: "downloading",
                completed_bytes: event.completed_bytes,
                total_bytes: event.total_bytes,
                current_file,
            },
        );
    }
}

fn safe_progress_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_version(
    version_id: String,
    app: AppHandle,
    installer: State<'_, Installer>,
    operations: State<'_, OperationRegistry>,
) -> Result<String, LauncherError> {
    let handle = operations.begin(&version_id)?;
    let operation_id = handle.operation_id.clone();
    let spawned_operation_id = operation_id.clone();
    let installer = installer.inner().clone();
    let operations = operations.inner().clone();
    let progress: Arc<dyn ProgressSink> = Arc::new(TauriProgressSink { app });
    tauri::async_runtime::spawn(async move {
        let result = installer
            .install_with_progress(
                spawned_operation_id.clone(),
                version_id,
                handle.cancel_token,
                progress,
            )
            .await;
        let (state, summary, error) = match result {
            Ok(summary) => (OperationState::Completed, Some(summary), None),
            Err(error) if error.code() == "download_cancelled" => {
                (OperationState::Cancelled, None, Some(error))
            }
            Err(error) => (OperationState::Failed, None, Some(error)),
        };
        let _ = operations.finish(&spawned_operation_id, state, summary, error);
    });
    Ok(operation_id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn cancel_operation(
    operation_id: String,
    operations: State<'_, OperationRegistry>,
) -> Result<(), LauncherError> {
    operations.cancel(&operation_id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn installation_status(
    operation_id: String,
    operations: State<'_, OperationRegistry>,
) -> Result<InstallationStatus, LauncherError> {
    operations.status(&operation_id)
}

#[cfg(test)]
mod tests {
    use super::safe_progress_name;
    use std::path::Path;

    #[test]
    fn progress_exposes_only_a_filename_not_local_paths_or_url_secrets() {
        assert_eq!(
            safe_progress_name(Path::new(r"C:\Users\secret\client.jar")),
            Some("client.jar".to_owned())
        );
    }
}
