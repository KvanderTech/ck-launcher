use crate::{
    error::LauncherError,
    metadata::{models::GameVersionSummary, resolver::MetadataService},
    profiles::ProfileService,
    storage::LauncherProfile,
};
use tauri::State;

#[tauri::command]
pub async fn list_game_versions(
    metadata: State<'_, MetadataService>,
) -> Result<Vec<GameVersionSummary>, LauncherError> {
    metadata.stable_releases().await
}

#[tauri::command]
pub async fn get_profile(
    profiles: State<'_, ProfileService>,
) -> Result<LauncherProfile, LauncherError> {
    profiles.get_profile().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_profile(
    profile: LauncherProfile,
    profiles: State<'_, ProfileService>,
) -> Result<LauncherProfile, LauncherError> {
    profiles.update_profile(profile).await
}
