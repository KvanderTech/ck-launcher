pub fn open_external_url(url: String) -> Result<(), crate::error::LauncherError> {
    const ALLOWED: [&str; 3] = [
        "https://t.me/comfortcentr",
        "https://discord.gg/2CkZsVN8nm",
        "https://github.com/KvanderTech/ck-launcher",
    ];
    if !ALLOWED.contains(&url.as_str()) {
        return Err(crate::error::LauncherError::new(
            "external_url_denied",
            "Эта ссылка не разрешена.",
            None,
            false,
        ));
    }
    open_browser(&url)
}

pub(crate) fn open_browser(url: &str) -> Result<(), crate::error::LauncherError> {
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn()
        .map_err(|_| {
            crate::error::LauncherError::new(
                "browser_open_failed",
                "Не удалось открыть системный браузер.",
                None,
                true,
            )
        })?;
    Ok(())
}

/// Select a filesystem object in Explorer without invoking its associated executable.
#[cfg(windows)]
pub fn show_in_folder(path: &std::path::Path) -> Result<(), crate::error::LauncherError> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
        },
        UI::Shell::{SHOpenFolderAndSelectItems, SHParseDisplayName},
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        // windows-sys 0.61 links CoTaskMemFree to combase.dll (Windows 8+).
        // Its longstanding ole32 export also works on Windows 7. Resolve it explicitly
        // so no raw-dylib import can select combase at load time.
        let ole32 = GetModuleHandleA(c"ole32.dll".as_ptr().cast());
        let free = GetProcAddress(ole32, c"CoTaskMemFree".as_ptr().cast()).ok_or_else(|| {
            crate::error::LauncherError::new(
                "folder_open_failed",
                "Не удалось открыть Проводник.",
                None,
                true,
            )
        })?;
        let free: unsafe extern "system" fn(*const core::ffi::c_void) = std::mem::transmute(free);
        let initialized = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let mut pidl = ptr::null_mut();
        let parsed = SHParseDisplayName(
            wide.as_ptr(),
            ptr::null_mut(),
            &mut pidl,
            0,
            ptr::null_mut(),
        );
        let result = if parsed >= 0 {
            SHOpenFolderAndSelectItems(pidl, 0, ptr::null(), 0)
        } else {
            parsed
        };
        if !pidl.is_null() {
            free(pidl.cast());
        }
        if initialized {
            CoUninitialize();
        }
        if result < 0 {
            return Err(crate::error::LauncherError::new(
                "folder_open_failed",
                "Не удалось показать файл в Проводнике.",
                None,
                true,
            ));
        }
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn show_in_folder(_: &std::path::Path) -> Result<(), crate::error::LauncherError> {
    Err(crate::error::LauncherError::new(
        "platform_unsupported",
        "Эта функция доступна в Windows.",
        None,
        false,
    ))
}
