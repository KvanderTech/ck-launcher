#![cfg_attr(windows, windows_subsystem = "windows")]

mod transaction;
use std::{env, fs, io, path::Path, path::PathBuf, process::Command, time::Duration};

fn after_process_exit(
    wait: impl FnOnce() -> io::Result<bool>,
    apply: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    if !wait()? {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "The launcher is still running; no files were replaced",
        ));
    }
    apply()
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::c_void,
        hash::{Hash, Hasher},
        ptr,
    };
    type Handle = *mut c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
        fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> Handle;
        fn ReleaseMutex(handle: Handle) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(owner: Handle, text: *const u16, title: *const u16, flags: u32) -> i32;
    }
    struct OwnedHandle(Handle);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub fn wait_for_exit(pid: u32, timeout: Duration) -> io::Result<bool> {
        if pid == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid launcher process",
            ));
        }
        // Keep the process handle, not its reusable PID, while waiting. These APIs work on Win7.
        let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(87) {
                Ok(true)
            } else {
                Err(error)
            };
        }
        let handle = OwnedHandle(handle);
        match unsafe {
            WaitForSingleObject(
                handle.0,
                timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
        } {
            0 => Ok(true),
            258 => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub struct UpdateLock(OwnedHandle);
    impl Drop for UpdateLock {
        fn drop(&mut self) {
            unsafe {
                ReleaseMutex(self.0 .0);
            }
        }
    }
    pub fn lock(target: &Path) -> io::Result<UpdateLock> {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        target.to_string_lossy().to_lowercase().hash(&mut hash);
        let name: Vec<u16> = format!("Local\\CKLauncherUpdate-{:016x}", hash.finish())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = OwnedHandle(handle);
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            0 | 128 => Ok(UpdateLock(handle)),
            258 => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Another launcher update is already running",
            )),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn report(error: &io::Error) {
        let message: Vec<u16> = format!("Не удалось обновить ЦК Лаунчер.\n\n{error}\n\nЗакройте лаунчер и повторите обновление. Если восстановление не удалось, резервные файлы сохранены по указанному пути.")
            .encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "ЦК Лаунчер — обновление"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(ptr::null_mut(), message.as_ptr(), title.as_ptr(), 0x10);
        }
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: updater <pid> <source> <target>",
        ));
    }
    let pid = args[1]
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid launcher process"))?;
    let source = PathBuf::from(&args[2]).canonicalize()?;
    let target = PathBuf::from(&args[3]).canonicalize()?;
    #[cfg(windows)]
    let _lock = windows::lock(&target)?;
    #[cfg(windows)]
    after_process_exit(
        || windows::wait_for_exit(pid, Duration::from_secs(30)),
        || transaction::apply(&source, &target),
    )?;
    #[cfg(not(windows))]
    {
        let _ = (pid, &source, &target);
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "The native updater is only supported on Windows",
        ));
    }
    #[cfg(windows)]
    {
        Command::new(target.join("ck-launcher-qt.exe"))
            .current_dir(&target)
            .spawn()?;
        cleanup_download(&source);
        Ok(())
    }
}

fn cleanup_download(source: &Path) {
    // Only remove our own downloaded package folder, never an arbitrary source parent.
    if let Ok(executable) = env::current_exe().and_then(|path| path.canonicalize()) {
        if let Some(root) = executable.parent() {
            if source == root.join("package")
                && root
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("CKLauncherUpdate-"))
            {
                let _ = fs::remove_dir_all(root);
            }
        }
    }
}

fn main() {
    if let Err(error) = run() {
        #[cfg(windows)]
        windows::report(&error);
        #[cfg(not(windows))]
        eprintln!("Launcher update failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeout_never_enters_file_replacement() {
        let result = after_process_exit(
            || Ok(false),
            || panic!("must not replace a running launcher"),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }
    #[test]
    fn process_tracking_error_is_not_treated_as_a_successful_exit() {
        assert!(after_process_exit(
            || Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot inspect process"
            )),
            || panic!("must not replace unknown process state")
        )
        .is_err());
    }
    #[test]
    fn clean_exit_enters_file_replacement_once() {
        let mut applied = 0;
        after_process_exit(
            || Ok(true),
            || {
                applied += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(applied, 1);
    }
    #[cfg(windows)]
    #[test]
    fn zero_pid_is_rejected_without_polling_an_unrelated_process() {
        assert_eq!(
            windows::wait_for_exit(0, Duration::ZERO)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_live_process_handle_cannot_be_mistaken_for_an_exited_pid() {
        assert!(!windows::wait_for_exit(std::process::id(), Duration::ZERO).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_updater_cannot_acquire_the_same_installation() {
        let target = env::temp_dir().join(format!("ck-updater-lock-test-{}", std::process::id()));
        let guard = windows::lock(&target).unwrap();
        let other_target = target.clone();
        assert!(std::thread::spawn(move || {
            match windows::lock(&other_target) {
                Ok(_) => false,
                Err(error) => error.kind() == io::ErrorKind::WouldBlock,
            }
        })
        .join()
        .unwrap());
        drop(guard);
        assert!(windows::lock(&target).is_ok());
    }
}
