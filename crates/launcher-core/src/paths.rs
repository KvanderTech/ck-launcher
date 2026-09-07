use crate::error::LauncherError;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathKind {
    Existing,
    Missing,
    ReparsePoint,
}

trait PathInspector {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    fn kind(&self, path: &Path) -> io::Result<PathKind>;
}

struct FileSystemInspector;

impl PathInspector for FileSystemInspector {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }

    fn kind(&self, path: &Path) -> io::Result<PathKind> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if is_reparse_point(&metadata) => Ok(PathKind::ReparsePoint),
            Ok(_) => Ok(PathKind::Existing),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PathKind::Missing),
            Err(error) => Err(error),
        }
    }
}

#[cfg(windows)]
pub(crate) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
    pub game: PathBuf,
    pub runtime: PathBuf,
    pub logs: PathBuf,
    pub database: PathBuf,
}

impl AppPaths {
    pub fn new(base: PathBuf) -> Self {
        Self {
            game: base.join("game"),
            runtime: base.join("runtime"),
            logs: base.join("logs"),
            database: base.join("launcher.sqlite3"),
            root: base,
        }
    }

    pub fn windows_default() -> Result<Self, LauncherError> {
        let app_data = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| LauncherError::internal("APPDATA is unavailable"))?;

        Ok(Self::new(app_data.join("CKLauncher")))
    }

    pub fn create_directories(&self) -> Result<(), LauncherError> {
        fs::create_dir_all(&self.root).map_err(|_| LauncherError::storage_unavailable())?;
        if FileSystemInspector
            .kind(&self.root)
            .map_err(|_| LauncherError::invalid_path())?
            != PathKind::Existing
        {
            return Err(LauncherError::invalid_path());
        }
        for relative in ["game", "runtime", "logs"] {
            let path = self.safe_join(&self.root, Path::new(relative))?;
            fs::create_dir_all(&path).map_err(|_| LauncherError::storage_unavailable())?;
            self.safe_join(&self.root, Path::new(relative))?;
        }

        Ok(())
    }

    pub fn database_url(&self) -> Result<String, LauncherError> {
        let database = self.safe_join(&self.root, Path::new("launcher.sqlite3"))?;
        let mut url =
            Url::from_file_path(database).map_err(|_| LauncherError::storage_unavailable())?;
        url.set_scheme("sqlite")
            .map_err(|_| LauncherError::storage_unavailable())?;
        url.set_query(Some("mode=rwc"));

        Ok(url.into())
    }

    /// Creates and canonicalizes a directory chosen by the user in a backend-owned picker.
    /// Every existing component is rejected if it is a link or Windows reparse point.
    pub fn prepare_user_selected_directory(
        &self,
        selected: &Path,
    ) -> Result<PathBuf, LauncherError> {
        if !selected.is_absolute()
            || selected.file_name().is_none()
            || selected
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(LauncherError::invalid_path());
        }

        let inspector = FileSystemInspector;
        let mut current = PathBuf::new();
        for component in selected.components() {
            current.push(component.as_os_str());
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            match inspector
                .kind(&current)
                .map_err(|_| LauncherError::invalid_path())?
            {
                PathKind::Existing => {}
                PathKind::ReparsePoint => return Err(LauncherError::invalid_path()),
                PathKind::Missing => {
                    fs::create_dir(&current).map_err(|_| LauncherError::storage_unavailable())?;
                    if inspector
                        .kind(&current)
                        .map_err(|_| LauncherError::invalid_path())?
                        != PathKind::Existing
                    {
                        return Err(LauncherError::invalid_path());
                    }
                }
            }
        }

        let canonical = inspector
            .canonicalize(selected)
            .map_err(|_| LauncherError::invalid_path())?;
        self.validate_absolute_directory(&canonical)?;
        Ok(canonical)
    }

    pub fn validate_absolute_directory(&self, directory: &Path) -> Result<(), LauncherError> {
        if !directory.is_absolute() || !directory.is_dir() || directory.file_name().is_none() {
            return Err(LauncherError::invalid_path());
        }
        let inspector = FileSystemInspector;
        let mut current = PathBuf::new();
        for component in directory.components() {
            current.push(component.as_os_str());
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            if inspector
                .kind(&current)
                .map_err(|_| LauncherError::invalid_path())?
                != PathKind::Existing
            {
                return Err(LauncherError::invalid_path());
            }
        }
        Ok(())
    }

    /// Validates a path immediately before a sensitive write.
    ///
    /// Existing links and Windows reparse points (including junctions and mount points) are
    /// rejected component by component, including dangling links. The filesystem can still
    /// change after this check, so callers that write sensitive files must keep the trusted root
    /// private and use platform no-follow/open-by-handle APIs where available in addition to
    /// validating as close to the write as possible.
    pub fn safe_join(&self, root: &Path, relative: &Path) -> Result<PathBuf, LauncherError> {
        self.safe_join_with(root, relative, &FileSystemInspector)
    }

    fn safe_join_with(
        &self,
        root: &Path,
        relative: &Path,
        inspector: &impl PathInspector,
    ) -> Result<PathBuf, LauncherError> {
        if relative.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        }) {
            return Err(LauncherError::invalid_path());
        }

        if inspector
            .kind(root)
            .map_err(|_| LauncherError::invalid_path())?
            != PathKind::Existing
        {
            return Err(LauncherError::invalid_path());
        }

        let canonical_root = inspector
            .canonicalize(root)
            .map_err(|_| LauncherError::invalid_path())?;
        let mut candidate = canonical_root.clone();

        for component in relative.components() {
            let Component::Normal(component) = component else {
                continue;
            };
            candidate.push(component);

            match inspector
                .kind(&candidate)
                .map_err(|_| LauncherError::invalid_path())?
            {
                PathKind::Existing => {}
                PathKind::Missing => {}
                PathKind::ReparsePoint => return Err(LauncherError::invalid_path()),
            }
        }

        Ok(candidate)
    }
}

/// Removes the `\\?\` verbatim prefix Windows `canonicalize()` adds.
/// Java (and most child processes) cannot resolve verbatim paths passed via argv.
/// ponytail: non-UTF-16-representable paths are returned unchanged; game roots are always UTF-8.
pub(crate) fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::{strip_verbatim_prefix, AppPaths, PathInspector, PathKind};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ck-launcher-paths-{unique}"));
        fs::create_dir_all(&root).expect("temporary root is created");
        root
    }

    #[test]
    fn verbatim_prefix_is_stripped_so_child_processes_can_read_the_path() {
        assert_eq!(
            strip_verbatim_prefix(PathBuf::from(r"\\?\C:\game\mods\a.jar")),
            PathBuf::from(r"C:\game\mods\a.jar")
        );
        assert_eq!(
            strip_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\a.jar")),
            PathBuf::from(r"\\server\share\a.jar")
        );
        assert_eq!(
            strip_verbatim_prefix(PathBuf::from(r"C:\game\mods\a.jar")),
            PathBuf::from(r"C:\game\mods\a.jar")
        );
    }

    #[test]
    fn safe_join_keeps_valid_nested_paths_below_the_root() {
        let root = temporary_root();
        let paths = AppPaths::new(root.clone());

        let joined = paths
            .safe_join(&root, Path::new("assets\\objects\\index.json"))
            .expect("nested relative path is accepted");

        assert!(joined.starts_with(root.canonicalize().expect("root canonicalizes")));
        assert_eq!(
            joined.file_name().and_then(|name| name.to_str()),
            Some("index.json")
        );
        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    #[test]
    fn safe_join_rejects_traversal_absolute_prefix_and_mixed_escape_paths() {
        let root = temporary_root();
        let paths = AppPaths::new(root.clone());

        for path in [
            Path::new("..\\secret.txt"),
            Path::new("C:\\secret.txt"),
            Path::new("\\\\?\\C:\\secret.txt"),
            Path::new("assets\\..\\..\\secret.txt"),
        ] {
            let error = paths
                .safe_join(&root, path)
                .expect_err("unsafe relative path is rejected");
            assert_eq!(error.code(), "invalid_path");
        }

        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    #[test]
    fn selected_game_directory_must_be_an_absolute_backend_picker_result() {
        let root = temporary_root();
        let paths = AppPaths::new(root.clone());

        let error = paths
            .prepare_user_selected_directory(Path::new("relative-game"))
            .expect_err("relative frontend-injected path is rejected");

        assert_eq!(error.code(), "invalid_path");
        assert!(!root.join("relative-game").exists());
        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    struct DanglingSymlinkInspector;

    impl PathInspector for DanglingSymlinkInspector {
        fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }

        fn kind(&self, path: &Path) -> std::io::Result<PathKind> {
            if path.ends_with("linked") {
                Ok(PathKind::ReparsePoint)
            } else {
                Ok(PathKind::Existing)
            }
        }
    }

    #[test]
    fn safe_join_rejects_paths_beneath_a_dangling_symlink_deterministically() {
        let root = temporary_root();
        let paths = AppPaths::new(root.clone());

        let error = paths
            .safe_join_with(
                &root,
                Path::new("linked\\new-file.txt"),
                &DanglingSymlinkInspector,
            )
            .expect_err("paths below a dangling symlink are rejected");

        assert_eq!(error.code(), "invalid_path");
        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    struct JunctionInspector;

    impl PathInspector for JunctionInspector {
        fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }

        fn kind(&self, path: &Path) -> std::io::Result<PathKind> {
            if path.ends_with("junction") {
                Ok(PathKind::ReparsePoint)
            } else {
                Ok(PathKind::Existing)
            }
        }
    }

    #[test]
    fn safe_join_rejects_all_reparse_points_not_only_symbolic_links() {
        let root = temporary_root();
        let paths = AppPaths::new(root.clone());

        let error = paths
            .safe_join_with(
                &root,
                Path::new("junction\\new-file.txt"),
                &JunctionInspector,
            )
            .expect_err("junction reparse point is rejected");

        assert_eq!(error.code(), "invalid_path");
        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    #[cfg(windows)]
    #[test]
    fn safe_join_rejects_a_real_dangling_directory_symlink_when_permitted() {
        use std::os::windows::fs::symlink_dir;

        let root = temporary_root();
        let link = root.join("linked");
        let missing_target = root.join("missing-target");
        match symlink_dir(&missing_target, &link) {
            Ok(()) => {
                let paths = AppPaths::new(root.clone());
                assert_eq!(
                    paths
                        .safe_join(&root, Path::new("linked\\new-file.txt"))
                        .expect_err("dangling symlink is rejected")
                        .code(),
                    "invalid_path"
                );
                fs::remove_dir_all(root).expect("temporary root is removed");
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                fs::remove_dir_all(root).expect("temporary root is removed");
            }
            Err(error) => panic!("unexpected symlink creation failure: {error}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn safe_join_rejects_a_real_windows_junction_when_creation_is_available() {
        use std::process::Command;

        let root = temporary_root();
        let target = root.join("junction-target");
        let junction = root.join("junction");
        fs::create_dir(&target).expect("junction target is created");
        let output = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("junction command starts");

        if output.status.success() {
            let paths = AppPaths::new(root.clone());
            assert_eq!(
                paths
                    .safe_join(&root, Path::new("junction\\new-file.txt"))
                    .expect_err("junction is rejected")
                    .code(),
                "invalid_path"
            );
            fs::remove_dir(&junction).expect("junction is removed without following it");
        }
        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    #[cfg(windows)]
    #[test]
    fn selected_game_directory_rejects_a_real_windows_junction() {
        use std::process::Command;

        let root = temporary_root();
        let target = root.join("outside-target");
        let junction = root.join("selected-junction");
        fs::create_dir(&target).expect("junction target is created");
        let output = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("junction command starts");

        if output.status.success() {
            let error = AppPaths::new(root.clone())
                .prepare_user_selected_directory(&junction)
                .expect_err("selected junction is rejected");
            assert_eq!(error.code(), "invalid_path");
            fs::remove_dir(&junction).expect("junction is removed without following it");
        }
        fs::remove_dir_all(root).expect("temporary root is removed");
    }

    #[cfg(windows)]
    #[test]
    fn directory_creation_refuses_an_existing_reparse_point_child() {
        use std::process::Command;

        let root = temporary_root();
        let target = root.join("external-game");
        let junction = root.join("game");
        fs::create_dir(&target).expect("junction target is created");
        let output = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("junction command starts");

        if output.status.success() {
            let error = AppPaths::new(root.clone())
                .create_directories()
                .expect_err("launcher directory junction is rejected before writes");
            assert_eq!(error.code(), "invalid_path");
            fs::remove_dir(&junction).expect("junction is removed without following it");
        }
        fs::remove_dir_all(root).expect("temporary root is removed");
    }
}
