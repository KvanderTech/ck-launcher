//! Prepare the complete new package, then replace files with a rollback journal in memory.
//! Backups are kept on the installation volume, so rollback is a rename, not another copy.
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILES: usize = 4000;
const MAX_PACKAGE_BYTES: u64 = 768 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

trait FileSystem {
    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
        fs::copy(from, to)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

struct RealFileSystem;
impl FileSystem for RealFileSystem {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        // The frontend has exited, but its backend or antivirus may release DLLs slightly later.
        let mut error = None;
        for _ in 0..80 {
            match fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(value) if value.kind() == io::ErrorKind::PermissionDenied => {
                    error = Some(value);
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                Err(value) => return Err(value),
            }
        }
        Err(error.expect("the retry loop records a failure"))
    }
}

pub fn apply(source: &Path, target: &Path) -> io::Result<()> {
    apply_with(source, target, &RealFileSystem)
}

struct Change {
    relative: PathBuf,
    previous: bool,
    installed: bool,
}

fn apply_with(source: &Path, target: &Path, fs_ops: &dyn FileSystem) -> io::Result<()> {
    checked_directory(source)?;
    checked_directory(target)?;
    let source = source.canonicalize()?;
    let target = target.canonicalize()?;
    if source == target || source.starts_with(&target) || target.starts_with(&source) {
        return Err(invalid("Update and installation directories overlap"));
    }
    checked_directory(&source)?;
    checked_directory(&target)?;
    let mut files = Vec::new();
    let mut total = 0;
    collect_files(&source, Path::new(""), &mut files, &mut total)?;
    files.sort();
    for required in ["ck-launcher-qt.exe", "ck-launcher-service.exe"] {
        if !files.iter().any(|path| path == Path::new(required)) {
            return Err(invalid("Incomplete update package"));
        }
    }
    // Reject conflicting directories or links before replacing any installed file.
    for relative in &files {
        check_destination(&target, relative)?;
    }

    let workspace = new_workspace(&target)?;
    let prepared = workspace.join("new");
    let backups = workspace.join("backup");
    let mut changes = Vec::<Change>::new();
    let result = (|| {
        fs::create_dir(&prepared)?;
        fs::create_dir(&backups)?;
        // Disk-full/read errors here cannot affect the working installation.
        for relative in &files {
            let from = source.join(relative);
            let to = prepared.join(relative);
            create_parents(&prepared, relative)?;
            let expected = checked_file(&from)?.len();
            if fs_ops.copy(&from, &to)? != expected || checked_file(&to)?.len() != expected {
                return Err(invalid("Incomplete staged update file"));
            }
        }
        for relative in &files {
            check_destination(&target, relative)?;
            create_parents(&target, relative)?;
            let destination = target.join(relative);
            let previous = destination.try_exists()?;
            if previous {
                create_parents(&backups, relative)?;
                fs_ops.rename(&destination, &backups.join(relative))?;
            }
            changes.push(Change {
                relative: relative.clone(),
                previous,
                installed: false,
            });
            fs_ops.rename(&prepared.join(relative), &destination)?;
            changes.last_mut().expect("current change").installed = true;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_error = None;
        for change in changes.iter().rev() {
            let destination = target.join(&change.relative);
            let restored = (|| {
                check_destination(&target, &change.relative)?;
                if change.installed {
                    fs_ops.remove_file(&destination)?;
                }
                if change.previous {
                    fs_ops.rename(&backups.join(&change.relative), &destination)?;
                }
                Ok::<(), io::Error>(())
            })();
            if let Err(value) = restored {
                rollback_error.get_or_insert(value);
            }
        }
        if let Some(rollback) = rollback_error {
            // Never delete the only remaining old files after an unsuccessful rollback.
            return Err(io::Error::other(format!(
                "Update failed: {error}. Restore failed: {rollback}. Previous files are preserved in {}",
                backups.display()
            )));
        }
        let _ = fs::remove_dir_all(&workspace);
        return Err(io::Error::new(
            error.kind(),
            format!("Update failed; the previous version was restored: {error}"),
        ));
    }
    let _ = fs::remove_dir_all(workspace);
    Ok(())
}

fn new_workspace(target: &Path) -> io::Result<PathBuf> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..16 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = target.join(format!(
            ".ck-update-{}-{time:x}-{sequence:x}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(invalid("Could not reserve update workspace"))
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn checked_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(invalid("Update directory is not a regular directory"));
    }
    Ok(())
}
fn checked_file(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(invalid("Update file is not a regular file"));
    }
    Ok(metadata)
}

fn collect_files(
    root: &Path,
    relative: &Path,
    files: &mut Vec<PathBuf>,
    total: &mut u64,
) -> io::Result<()> {
    if relative.components().count() > 32 {
        return Err(invalid("Update directory nesting is too deep"));
    }
    checked_directory(&root.join(relative))?;
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(".ck-update-") {
            return Err(invalid("Reserved update path"));
        }
        let path = relative.join(name);
        let metadata = fs::symlink_metadata(entry.path())?;
        if is_link(&metadata) {
            return Err(invalid("Links are not allowed in update packages"));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files, total)?;
        } else if metadata.is_file() {
            *total = total
                .checked_add(metadata.len())
                .ok_or_else(|| invalid("Update package is too large"))?;
            if files.len() >= MAX_FILES || *total > MAX_PACKAGE_BYTES {
                return Err(invalid("Update package is too large"));
            }
            files.push(path);
        } else {
            return Err(invalid("Unsupported update file type"));
        }
    }
    Ok(())
}

fn check_destination(root: &Path, relative: &Path) -> io::Result<()> {
    let mut path = root.to_owned();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(invalid("Invalid update path"));
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(invalid("Update target contains a link"))
            }
            Ok(metadata) if index + 1 == components.len() && !metadata.is_file() => {
                return Err(invalid("Update file conflicts with a directory"))
            }
            Ok(metadata) if index + 1 < components.len() && !metadata.is_dir() => {
                return Err(invalid("Update directory conflicts with a file"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn create_parents(root: &Path, relative: &Path) -> io::Result<()> {
    let mut current = root.to_owned();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(name) = component else {
            return Err(invalid("Invalid update directory"));
        };
        current.push(name);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        checked_directory(&current)?;
    }
    Ok(())
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        target: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = new_workspace(&std::env::temp_dir()).unwrap();
            let source = root.join("package");
            let target = root.join("installed");
            fs::create_dir(&source).unwrap();
            fs::create_dir(&target).unwrap();
            for file in [
                "a-new.dll",
                "b.dll",
                "ck-launcher-qt.exe",
                "ck-launcher-service.exe",
                "platforms/qwindows.dll",
            ] {
                create_parents(&source, Path::new(file)).unwrap();
                fs::write(source.join(file), format!("new {file}")).unwrap();
                if file != "a-new.dll" {
                    create_parents(&target, Path::new(file)).unwrap();
                    fs::write(target.join(file), format!("old {file}")).unwrap();
                }
            }
            fs::write(target.join("personal-settings.json"), b"keep this").unwrap();
            Self {
                root,
                source,
                target,
            }
        }
        fn assert_previous(&self) {
            for file in [
                "b.dll",
                "ck-launcher-qt.exe",
                "ck-launcher-service.exe",
                "platforms/qwindows.dll",
            ] {
                assert_eq!(
                    fs::read_to_string(self.target.join(file)).unwrap(),
                    format!("old {file}")
                );
            }
            assert!(!self.target.join("a-new.dll").exists());
            assert_eq!(
                fs::read(self.target.join("personal-settings.json")).unwrap(),
                b"keep this"
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    struct Failures {
        copy: bool,
        rollback: bool,
    }
    impl FileSystem for Failures {
        fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
            if self.copy && from.file_name().unwrap() == "ck-launcher-service.exe" {
                return Err(io::Error::other("disk full"));
            }
            fs::copy(from, to)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let parts: Vec<_> = from.components().collect();
            let prepared = parts.iter().any(|part| part.as_os_str() == "new");
            let backup = parts.iter().any(|part| part.as_os_str() == "backup");
            if !self.copy
                && (prepared && from.file_name().unwrap() == "b.dll" || self.rollback && backup)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "DLL locked",
                ));
            }
            fs::rename(from, to)
        }
    }
    #[test]
    fn complete_package_is_applied_without_removing_unrelated_user_files() {
        let fixture = Fixture::new();
        apply(&fixture.source, &fixture.target).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.target.join("ck-launcher-qt.exe")).unwrap(),
            "new ck-launcher-qt.exe"
        );
        assert!(fixture.target.join("a-new.dll").is_file());
        assert_eq!(
            fs::read(fixture.target.join("personal-settings.json")).unwrap(),
            b"keep this"
        );
        assert!(!fs::read_dir(&fixture.target).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ck-update-")));
    }
    #[test]
    fn disk_full_during_preparation_leaves_every_installed_file_untouched() {
        let fixture = Fixture::new();
        assert!(apply_with(
            &fixture.source,
            &fixture.target,
            &Failures {
                copy: true,
                rollback: false
            }
        )
        .is_err());
        fixture.assert_previous();
    }
    #[test]
    fn failed_activation_restores_replaced_files_and_removes_new_files() {
        let fixture = Fixture::new();
        assert!(apply_with(
            &fixture.source,
            &fixture.target,
            &Failures {
                copy: false,
                rollback: false
            }
        )
        .is_err());
        fixture.assert_previous();
    }
    #[test]
    fn failed_rollback_preserves_the_only_old_copy_in_a_discoverable_backup() {
        let fixture = Fixture::new();
        let error = apply_with(
            &fixture.source,
            &fixture.target,
            &Failures {
                copy: false,
                rollback: true,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("Previous files are preserved"));
        let work = fs::read_dir(&fixture.target)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".ck-update-")
            })
            .unwrap();
        assert_eq!(fs::read(work.join("backup/b.dll")).unwrap(), b"old b.dll");
    }
    #[test]
    fn conflicting_directory_and_incomplete_package_are_rejected_before_replacement() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.target.join("b.dll")).unwrap();
        fs::create_dir(fixture.target.join("b.dll")).unwrap();
        assert!(apply(&fixture.source, &fixture.target).is_err());
        assert_eq!(
            fs::read(fixture.target.join("ck-launcher-qt.exe")).unwrap(),
            b"old ck-launcher-qt.exe"
        );
        fs::remove_file(fixture.source.join("ck-launcher-service.exe")).unwrap();
        assert!(apply(&fixture.source, &fixture.target).is_err());
    }
    #[test]
    fn source_and_target_must_not_overlap() {
        let fixture = Fixture::new();
        assert!(apply(&fixture.source, &fixture.source).is_err());
        assert!(apply(&fixture.root, &fixture.target).is_err());
        fixture.assert_previous();
    }
}
