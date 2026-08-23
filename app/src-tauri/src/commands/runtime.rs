use crate::{
    error::LauncherError,
    runtime::{JavaRequirement, JavaRuntimeStatus, RuntimeManager},
};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub async fn runtime_statuses(
    manager: State<'_, RuntimeManager>,
) -> Result<Vec<JavaRuntimeStatus>, LauncherError> {
    manager.statuses().await
}

#[tauri::command]
pub async fn detect_runtime(
    requirement: JavaRequirement,
    manager: State<'_, RuntimeManager>,
) -> Result<JavaRuntimeStatus, LauncherError> {
    manager.resolve(requirement, None).await
}

#[tauri::command]
pub async fn install_runtime(
    requirement: JavaRequirement,
    manager: State<'_, RuntimeManager>,
) -> Result<JavaRuntimeStatus, LauncherError> {
    manager.install(requirement).await
}

#[tauri::command]
pub async fn choose_runtime_path(
    requirement: JavaRequirement,
    manager: State<'_, RuntimeManager>,
) -> Result<Option<JavaRuntimeStatus>, LauncherError> {
    let Some(path) = choose_java_executable()? else {
        return Ok(None);
    };
    manager
        .choose_manual_runtime(requirement, path)
        .await
        .map(Some)
}

#[cfg(windows)]
fn choose_java_executable() -> Result<Option<PathBuf>, LauncherError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, OFN_DONTADDTORECENT, OFN_EXPLORER,
        OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_NODEREFERENCELINKS, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let mut file = vec![0u16; 32_768];
    let filter: Vec<u16> = "Java executable\0java.exe\0Executables\0*.exe\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "Выберите java.exe\0".encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_DONTADDTORECENT
            | OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_NODEREFERENCELINKS
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    let selected = unsafe { GetOpenFileNameW(&mut dialog) };
    if selected == 0 {
        let error = unsafe { CommDlgExtendedError() };
        if error == 0 {
            return Ok(None);
        }
        return Err(LauncherError::new(
            "runtime_picker_failed",
            "The Java file picker could not be opened.",
            None,
            true,
        ));
    }
    let length = file
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..length],
    ))))
}

#[cfg(not(windows))]
fn choose_java_executable() -> Result<Option<PathBuf>, LauncherError> {
    Err(LauncherError::new(
        "runtime_picker_unavailable",
        "Java selection is available only on Windows.",
        None,
        false,
    ))
}
