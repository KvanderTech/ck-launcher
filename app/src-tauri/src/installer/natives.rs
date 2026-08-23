use super::{path_safety::is_strict_windows_relative_path, validate_version_id, NativeArchive};
use crate::{downloads::DownloadCancellationToken, error::LauncherError, paths::AppPaths};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

const STAGING_PREFIX: &str = "natives.installing-";
const BACKUP_PREFIX: &str = "natives.backup-";
pub(super) const MAX_NATIVE_ENTRIES: usize = 4096;
const MAX_NATIVE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const MAX_TOTAL_NATIVE_BYTES: u64 = 512 * 1024 * 1024;

pub(super) fn extract_natives_transactional(
    game_root: &Path,
    version_id: &str,
    archives: &[NativeArchive],
    cancel: &DownloadCancellationToken,
) -> Result<PathBuf, LauncherError> {
    validate_version_id(version_id)?;
    let safety = AppPaths::new(game_root.to_path_buf());
    safety.safe_join(game_root, Path::new(""))?;
    let version_relative = Path::new("versions").join(version_id);
    create_directories(game_root, &version_relative)?;
    recover_interrupted(game_root, &version_relative)?;
    if cancel.is_cancelled() {
        return Err(cancelled());
    }

    let nonce = rand::random::<u64>();
    let staging_relative = version_relative.join(format!("{STAGING_PREFIX}{nonce}"));
    let staging = safety.safe_join(game_root, &staging_relative)?;
    fs::create_dir(&staging).map_err(|_| native_storage_error())?;
    safety.safe_join(game_root, &staging_relative)?;

    let extracted = (|| {
        let mut budget = NativeBudget::default();
        for archive in archives {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            extract_archive(game_root, &staging_relative, archive, cancel, &mut budget)?;
        }
        activate_staging(game_root, &version_relative, &staging_relative)
    })();
    if extracted.is_err() {
        if let Ok(staging) = safety.safe_join(game_root, &staging_relative) {
            let _ = fs::remove_dir_all(staging);
        }
    }
    extracted
}

fn extract_archive(
    game_root: &Path,
    staging_relative: &Path,
    archive: &NativeArchive,
    cancel: &DownloadCancellationToken,
    budget: &mut NativeBudget,
) -> Result<(), LauncherError> {
    let safety = AppPaths::new(game_root.to_path_buf());
    let archive_relative = relative_to_root(game_root, &archive.archive)?;
    let archive_path = safety.safe_join(game_root, &archive_relative)?;
    if !archive_path
        .metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return Err(native_archive_invalid());
    }
    let file = File::open(archive_path).map_err(|_| native_storage_error())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| native_archive_invalid())?;
    if zip.len() > MAX_NATIVE_ENTRIES {
        return Err(native_archive_invalid());
    }
    let sizes = (0..zip.len())
        .map(|index| {
            zip.by_index(index)
                .map(|entry| entry.size())
                .map_err(|_| native_archive_invalid())
        })
        .collect::<Result<Vec<_>, _>>()?;
    budget.add(zip.len(), sizes)?;
    let excludes = archive
        .excludes
        .iter()
        .map(|exclude| normalized_exclude(exclude))
        .collect::<Result<Vec<_>, _>>()?;

    for index in 0..zip.len() {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let entry = zip.by_index(index).map_err(|_| native_archive_invalid())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(native_archive_invalid());
        }
        let enclosed = entry.enclosed_name().ok_or_else(native_archive_invalid)?;
        validate_archive_relative(&enclosed)?;
        let normalized = enclosed.to_string_lossy().replace('\\', "/");
        if excluded(&normalized, &excludes) {
            continue;
        }
        let destination_relative = staging_relative.join(enclosed);
        if entry.is_dir() {
            create_directories(game_root, &destination_relative)?;
            continue;
        }
        if let Some(parent) = destination_relative.parent() {
            create_directories(game_root, parent)?;
        }
        let destination = safety.safe_join(game_root, &destination_relative)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| native_archive_invalid())?;
        let expected = entry.size();
        let copied = io::copy(&mut entry.take(expected.saturating_add(1)), &mut output)
            .map_err(|_| native_storage_error())?;
        if copied != expected {
            return Err(native_archive_invalid());
        }
        output
            .flush()
            .and_then(|_| output.sync_all())
            .map_err(|_| native_storage_error())?;
    }
    Ok(())
}

fn activate_staging(
    game_root: &Path,
    version_relative: &Path,
    staging_relative: &Path,
) -> Result<PathBuf, LauncherError> {
    activate_staging_with_cleanup(game_root, version_relative, staging_relative, |path| {
        fs::remove_dir_all(path)
    })
}

pub(super) fn activate_staging_with_cleanup(
    game_root: &Path,
    version_relative: &Path,
    staging_relative: &Path,
    cleanup: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<PathBuf, LauncherError> {
    let safety = AppPaths::new(game_root.to_path_buf());
    let destination_relative = version_relative.join("natives");
    let destination = safety.safe_join(game_root, &destination_relative)?;
    let backup_relative =
        version_relative.join(format!("{BACKUP_PREFIX}{}", rand::random::<u64>()));
    let had_destination = destination.exists();
    if had_destination {
        if !destination
            .metadata()
            .is_ok_and(|metadata| metadata.file_type().is_dir())
        {
            return Err(LauncherError::invalid_path());
        }
        let backup = safety.safe_join(game_root, &backup_relative)?;
        let destination = safety.safe_join(game_root, &destination_relative)?;
        fs::rename(destination, backup).map_err(|_| native_storage_error())?;
    }
    let staging = safety.safe_join(game_root, staging_relative)?;
    let destination = safety.safe_join(game_root, &destination_relative)?;
    if fs::rename(staging, &destination).is_err() {
        if had_destination {
            let backup = safety.safe_join(game_root, &backup_relative)?;
            let destination = safety.safe_join(game_root, &destination_relative)?;
            fs::rename(backup, destination).map_err(|_| native_state_inconsistent())?;
        }
        return Err(native_storage_error());
    }
    let destination = safety.safe_join(game_root, &destination_relative)?;
    if !destination.is_dir() {
        return Err(native_state_inconsistent());
    }
    if had_destination {
        let backup = safety.safe_join(game_root, &backup_relative)?;
        // Activation is already complete and verified at this point. A locked backup is
        // discoverable by its reserved prefix and will be retried by pre-install recovery.
        let _ = cleanup(&backup);
    }
    Ok(destination)
}

pub(super) fn recover_interrupted(
    game_root: &Path,
    version_relative: &Path,
) -> Result<(), LauncherError> {
    let safety = AppPaths::new(game_root.to_path_buf());
    let version_root = safety.safe_join(game_root, version_relative)?;
    let destination_relative = version_relative.join("natives");
    let mut backups = Vec::new();
    for entry in fs::read_dir(&version_root).map_err(|_| native_storage_error())? {
        let entry = entry.map_err(|_| native_storage_error())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = version_relative.join(&name);
        let path = safety.safe_join(game_root, &relative)?;
        if name.starts_with(STAGING_PREFIX) {
            if !path
                .metadata()
                .is_ok_and(|metadata| metadata.file_type().is_dir())
            {
                return Err(LauncherError::invalid_path());
            }
            fs::remove_dir_all(path).map_err(|_| native_storage_error())?;
        } else if name.starts_with(BACKUP_PREFIX) {
            backups.push(relative);
        }
    }
    let destination = safety.safe_join(game_root, &destination_relative)?;
    for backup_relative in backups {
        let backup = safety.safe_join(game_root, &backup_relative)?;
        if destination.exists() {
            fs::remove_dir_all(backup).map_err(|_| native_storage_error())?;
        } else {
            let destination = safety.safe_join(game_root, &destination_relative)?;
            fs::rename(backup, destination).map_err(|_| native_state_inconsistent())?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn validate_native_budget(
    entry_count: usize,
    sizes: impl IntoIterator<Item = u64>,
) -> Result<(), LauncherError> {
    NativeBudget::default().add(entry_count, sizes)
}

#[derive(Default)]
struct NativeBudget {
    entries: usize,
    bytes: u64,
}

impl NativeBudget {
    fn add(
        &mut self,
        entry_count: usize,
        sizes: impl IntoIterator<Item = u64>,
    ) -> Result<(), LauncherError> {
        self.entries = self
            .entries
            .checked_add(entry_count)
            .ok_or_else(native_archive_invalid)?;
        if self.entries > MAX_NATIVE_ENTRIES {
            return Err(native_archive_invalid());
        }
        let mut observed = 0_usize;
        for size in sizes {
            observed = observed.checked_add(1).ok_or_else(native_archive_invalid)?;
            if size > MAX_NATIVE_ENTRY_BYTES {
                return Err(native_archive_invalid());
            }
            self.bytes = self
                .bytes
                .checked_add(size)
                .ok_or_else(native_archive_invalid)?;
            if self.bytes > MAX_TOTAL_NATIVE_BYTES {
                return Err(native_archive_invalid());
            }
        }
        if observed != entry_count {
            return Err(native_archive_invalid());
        }
        Ok(())
    }
}

pub(super) fn create_directories(root: &Path, relative: &Path) -> Result<(), LauncherError> {
    let safety = AppPaths::new(root.to_path_buf());
    let mut current = PathBuf::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(LauncherError::invalid_path());
        };
        current.push(component);
        let path = safety.safe_join(root, &current)?;
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(native_storage_error()),
        }
        let path = safety.safe_join(root, &current)?;
        if !path
            .metadata()
            .is_ok_and(|metadata| metadata.file_type().is_dir())
        {
            return Err(LauncherError::invalid_path());
        }
    }
    Ok(())
}

fn normalized_exclude(exclude: &str) -> Result<String, LauncherError> {
    let normalized = exclude.replace('\\', "/");
    validate_archive_relative(Path::new(normalized.trim_end_matches('/')))?;
    Ok(format!("{}/", normalized.trim_matches('/')).to_ascii_lowercase())
}

fn excluded(path: &str, excludes: &[String]) -> bool {
    let path = path.trim_start_matches('/').to_ascii_lowercase();
    path == "meta-inf"
        || path.starts_with("meta-inf/")
        || excludes.iter().any(|exclude| path.starts_with(exclude))
}

fn validate_archive_relative(path: &Path) -> Result<(), LauncherError> {
    if !is_strict_windows_relative_path(path) {
        return Err(native_archive_invalid());
    }
    Ok(())
}

fn relative_to_root(root: &Path, path: &Path) -> Result<PathBuf, LauncherError> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Ok(relative.to_path_buf());
    }
    let root_text = root.to_string_lossy();
    let root_text = root_text.strip_prefix(r"\\?\").unwrap_or(&root_text);
    let path_text = path.to_string_lossy();
    let path_text = path_text.strip_prefix(r"\\?\").unwrap_or(&path_text);
    Path::new(path_text)
        .strip_prefix(Path::new(root_text))
        .map(Path::to_path_buf)
        .map_err(|_| LauncherError::invalid_path())
}

fn cancelled() -> LauncherError {
    LauncherError::new(
        "download_cancelled",
        "The download was cancelled.",
        None,
        true,
    )
}
fn native_archive_invalid() -> LauncherError {
    LauncherError::new(
        "native_archive_invalid",
        "A native library archive is invalid.",
        None,
        false,
    )
}
fn native_storage_error() -> LauncherError {
    LauncherError::new(
        "native_storage_failed",
        "Native libraries could not be installed.",
        None,
        true,
    )
}
fn native_state_inconsistent() -> LauncherError {
    LauncherError::new(
        "native_state_inconsistent",
        "Native libraries could not be restored consistently.",
        None,
        true,
    )
}
