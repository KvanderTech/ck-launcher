use crate::{downloads::DownloadCancellationToken, error::LauncherError, paths::AppPaths};
use std::{
    fs::{self, OpenOptions},
    io::{self, Cursor},
    path::{Component, Path},
};

const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_EXTRACTED_BYTES: u64 = 1_073_741_824;

/// Extracts a ZIP into a freshly created, launcher-private directory.
///
/// Every entry is validated again immediately before its write. Files are opened with
/// `create_new`; because `root` is a unique private directory and archive links are rejected,
/// an entry cannot replace an existing file. `AppPaths::safe_join` also rejects reparse/symlink
/// components at the last practical boundary before directory creation and file open.
pub fn extract_zip_archive(bytes: &[u8], root: &Path) -> Result<(), LauncherError> {
    extract_zip_archive_cancellable(bytes, root, &DownloadCancellationToken::new())
}

pub(crate) fn extract_zip_archive_cancellable(
    bytes: &[u8],
    root: &Path,
    cancel: &DownloadCancellationToken,
) -> Result<(), LauncherError> {
    ensure_not_cancelled(cancel)?;
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| invalid_archive())?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(invalid_archive());
    }
    let total_size = (0..archive.len()).try_fold(0u64, |total, index| {
        let entry = archive.by_index(index).map_err(|_| invalid_archive())?;
        total.checked_add(entry.size()).ok_or_else(invalid_archive)
    })?;
    if total_size > MAX_EXTRACTED_BYTES {
        return Err(invalid_archive());
    }

    let safety = AppPaths::new(root.to_path_buf());
    for index in 0..archive.len() {
        ensure_not_cancelled(cancel)?;
        let mut entry = archive.by_index(index).map_err(|_| invalid_archive())?;
        let relative = validate_entry(&entry)?;
        let destination = safety
            .safe_join(root, relative)
            .map_err(|_| unsafe_archive())?;
        if entry.is_dir() {
            fs::create_dir_all(&destination).map_err(|_| invalid_archive())?;
            continue;
        }
        let parent = destination.parent().ok_or_else(invalid_archive)?;
        fs::create_dir_all(parent).map_err(|_| invalid_archive())?;
        let destination = safety
            .safe_join(root, relative)
            .map_err(|_| unsafe_archive())?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| invalid_archive())?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            ensure_not_cancelled(cancel)?;
            let read = io::Read::read(&mut entry, &mut buffer).map_err(|_| invalid_archive())?;
            if read == 0 {
                break;
            }
            io::Write::write_all(&mut file, &buffer[..read]).map_err(|_| invalid_archive())?;
        }
        file.sync_all().map_err(|_| invalid_archive())?;
    }
    Ok(())
}

fn ensure_not_cancelled(cancel: &DownloadCancellationToken) -> Result<(), LauncherError> {
    if cancel.is_cancelled() {
        Err(LauncherError::new(
            "download_cancelled",
            "The operation was cancelled.",
            None,
            true,
        ))
    } else {
        Ok(())
    }
}

fn validate_entry<'a>(entry: &'a zip::read::ZipFile<'_>) -> Result<&'a Path, LauncherError> {
    let relative = Path::new(entry.name());
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(unsafe_archive());
    }
    if let Some(mode) = entry.unix_mode() {
        let kind = mode & 0o170000;
        if kind != 0 && kind != 0o100000 && kind != 0o040000 {
            return Err(unsafe_archive());
        }
    }
    Ok(relative)
}

fn unsafe_archive() -> LauncherError {
    LauncherError::new(
        "runtime_archive_unsafe",
        "The Java runtime archive contains an unsafe path or link.",
        None,
        false,
    )
}

fn invalid_archive() -> LauncherError {
    LauncherError::new(
        "runtime_archive_invalid",
        "The Java runtime archive is invalid.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::extract_zip_archive;
    use std::{
        fs,
        io::{Cursor, Write},
        path::PathBuf,
    };
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn temporary_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ck-runtime-archive-{label}-{}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("temporary root");
        root
    }

    fn zip_with_file(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(name, SimpleFileOptions::default())
            .expect("entry");
        archive.write_all(contents).expect("contents");
        archive.finish().expect("zip").into_inner()
    }

    #[test]
    fn extracts_a_regular_nested_runtime_file_below_the_private_root() {
        let root = temporary_root("valid");
        extract_zip_archive(&zip_with_file("jdk/bin/java.exe", b"java"), &root)
            .expect("valid archive extracts");
        assert_eq!(
            fs::read(root.join("jdk/bin/java.exe")).expect("java extracted"),
            b"java"
        );
        fs::remove_dir_all(root).expect("temporary root removed");
    }

    #[test]
    fn rejects_traversal_absolute_prefix_and_symlink_entries() {
        for (label, bytes) in [
            ("traversal", zip_with_file("../escape.exe", b"escape")),
            ("absolute", zip_with_file("/absolute.exe", b"escape")),
            ("prefix", zip_with_file("C:/escape.exe", b"escape")),
            ("symlink", {
                let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
                archive
                    .add_symlink(
                        "jdk/bin/java.exe",
                        "../../escape.exe",
                        SimpleFileOptions::default(),
                    )
                    .expect("symlink entry");
                archive.finish().expect("zip").into_inner()
            }),
        ] {
            let root = temporary_root(label);
            let error = extract_zip_archive(&bytes, &root).expect_err("unsafe archive is rejected");
            assert!(matches!(
                error.code(),
                "runtime_archive_unsafe" | "runtime_archive_invalid"
            ));
            fs::remove_dir_all(root).expect("temporary root removed");
        }
    }
}
