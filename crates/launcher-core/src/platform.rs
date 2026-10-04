pub fn open_external_url(url: String) -> Result<(), crate::error::LauncherError> {
    if !is_social_url(&url) && !is_curseforge_search(&url) {
        return Err(crate::error::LauncherError::new(
            "external_url_denied",
            "Эта ссылка не разрешена.",
            None,
            false,
        ));
    }
    open_browser(&url)
}

fn is_social_url(url: &str) -> bool {
    const ALLOWED: [&str; 3] = [
        "https://t.me/kvanth_launcher",
        "https://discord.gg/2CkZsVN8nm",
        "https://github.com/KvanderTech/ck-launcher",
    ];
    ALLOWED.contains(&url)
}

fn is_curseforge_search(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|parsed| {
        parsed.scheme() == "https"
            && parsed.host_str() == Some("www.curseforge.com")
            && parsed.port().is_none()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.path() == "/minecraft/search"
    })
}

#[cfg(test)]
mod tests {
    use super::{is_curseforge_search, is_social_url};

    #[test]
    fn home_social_links_are_allowed() {
        for url in [
            "https://t.me/kvanth_launcher",
            "https://discord.gg/2CkZsVN8nm",
            "https://github.com/KvanderTech/ck-launcher",
        ] {
            assert!(is_social_url(url), "{url}");
        }
        assert!(!is_social_url("https://t.me/comfortcentr"));
    }

    #[test]
    fn curseforge_search_allows_only_official_minecraft_search() {
        assert!(is_curseforge_search(
            "https://www.curseforge.com/minecraft/search?class=modpacks&version=1.21.1"
        ));
        for denied in [
            "http://www.curseforge.com/minecraft/search",
            "https://www.curseforge.com.evil.test/minecraft/search",
            "https://user@www.curseforge.com/minecraft/search",
            "https://www.curseforge.com:8443/minecraft/search",
            "https://www.curseforge.com/minecraft/modpacks",
        ] {
            assert!(!is_curseforge_search(denied), "{denied}");
        }
    }
}

pub(crate) fn open_browser(url: &str) -> Result<(), crate::error::LauncherError> {
    #[cfg(windows)]
    {
        use std::{iter::once, os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

        let operation: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(once(0))
            .collect();
        let target: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(once(0))
            .collect();
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
        if result as isize > 32 {
            return Ok(());
        }
    }
    #[cfg(not(windows))]
    {
        if std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    Err(crate::error::LauncherError::new(
        "browser_open_failed",
        "Не удалось открыть системный браузер.",
        None,
        true,
    ))
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
