use crate::{
    error::LauncherError,
    metadata::{models::GameVersionSummary, resolver::MetadataService},
    profiles::{MemorySettingsStatus, ProfileService},
    runtime::{requirement_for_version, JavaRequirement},
    storage::LauncherProfile,
};
use std::{path::PathBuf, sync::Arc};

pub async fn list_game_versions(
    metadata: &Arc<MetadataService>,
) -> Result<Vec<GameVersionSummary>, LauncherError> {
    metadata.stable_releases().await
}

pub async fn required_java_for_version(
    version_id: String,
    metadata: &Arc<MetadataService>,
) -> Result<JavaRequirement, LauncherError> {
    let version = metadata.resolved_version(&version_id).await?;
    requirement_for_version(&version)
}

pub async fn get_profile(profiles: &ProfileService) -> Result<LauncherProfile, LauncherError> {
    profiles.get_profile().await
}

pub async fn update_profile(
    profile: LauncherProfile,
    profiles: &ProfileService,
) -> Result<LauncherProfile, LauncherError> {
    profiles.update_profile(profile).await
}

pub async fn choose_game_directory(
    profiles: &ProfileService,
) -> Result<Option<LauncherProfile>, LauncherError> {
    let Some(path) = choose_directory()? else {
        return Ok(None);
    };
    profiles.select_game_directory(path).await.map(Some)
}

pub async fn update_profile_memory(
    memory_mb: u32,
    profiles: &ProfileService,
) -> Result<LauncherProfile, LauncherError> {
    profiles.update_memory(memory_mb).await
}

pub async fn memory_status(
    profiles: &ProfileService,
) -> Result<MemorySettingsStatus, LauncherError> {
    profiles.memory_status().await
}

#[cfg(windows)]
fn choose_directory() -> Result<Option<PathBuf>, LauncherError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Shell::{
        ILFree, SHBrowseForFolderW, SHGetPathFromIDListEx, BIF_EDITBOX, BIF_NEWDIALOGSTYLE,
        BIF_RETURNONLYFSDIRS, BROWSEINFOW, GPFIDL_DEFAULT,
    };

    let title: Vec<u16> = "Выберите папку Minecraft\0".encode_utf16().collect();
    let mut display_name = vec![0_u16; 32_768];
    let dialog = BROWSEINFOW {
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
        ..Default::default()
    };
    let item = unsafe { SHBrowseForFolderW(&dialog) };
    if item.is_null() {
        return Ok(None);
    }
    let mut selected = vec![0_u16; 32_768];
    let resolved = unsafe {
        SHGetPathFromIDListEx(
            item,
            selected.as_mut_ptr(),
            selected.len() as u32,
            GPFIDL_DEFAULT,
        )
    };
    unsafe { ILFree(item) };
    if resolved == 0 {
        return Err(LauncherError::new(
            "game_directory_picker_failed",
            "The game directory picker could not resolve the selected folder.",
            None,
            true,
        ));
    }
    let length = selected
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(selected.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &selected[..length],
    ))))
}

#[cfg(not(windows))]
fn choose_directory() -> Result<Option<PathBuf>, LauncherError> {
    Err(LauncherError::new(
        "game_directory_picker_unavailable",
        "Game directory selection is available only on Windows.",
        None,
        false,
    ))
}
