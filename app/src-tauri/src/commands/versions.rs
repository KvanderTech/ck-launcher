use crate::{
    error::LauncherError,
    metadata::{models::GameVersionSummary, resolver::MetadataService},
    profiles::{MemorySettingsStatus, ProfileService},
    runtime::{requirement_for_version, JavaRequirement},
    storage::LauncherProfile,
};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn list_game_versions(
    metadata: State<'_, Arc<MetadataService>>,
) -> Result<Vec<GameVersionSummary>, LauncherError> {
    metadata.stable_releases().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn required_java_for_version(
    version_id: String,
    metadata: State<'_, Arc<MetadataService>>,
) -> Result<JavaRequirement, LauncherError> {
    let version = metadata.resolved_version(&version_id).await?;
    requirement_for_version(&version)
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

#[tauri::command(rename_all = "camelCase")]
pub async fn update_profile_memory(
    memory_mb: u32,
    profiles: State<'_, ProfileService>,
) -> Result<LauncherProfile, LauncherError> {
    profiles.update_memory(memory_mb).await
}

#[tauri::command]
pub async fn memory_status(
    profiles: State<'_, ProfileService>,
) -> Result<MemorySettingsStatus, LauncherError> {
    profiles.memory_status().await
}
