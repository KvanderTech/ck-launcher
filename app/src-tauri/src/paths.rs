use crate::error::LauncherError;
use std::fs;
use std::path::{Component, Path, PathBuf};
use url::Url;

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
        for path in [&self.root, &self.game, &self.runtime, &self.logs] {
            fs::create_dir_all(path).map_err(|_| LauncherError::storage_unavailable())?;
        }

        Ok(())
    }

    pub fn database_url(&self) -> Result<String, LauncherError> {
        let mut url = Url::from_file_path(&self.database)
            .map_err(|_| LauncherError::storage_unavailable())?;
        url.set_scheme("sqlite")
            .map_err(|_| LauncherError::storage_unavailable())?;
        url.set_query(Some("mode=rwc"));

        Ok(url.into())
    }

    pub fn safe_join(&self, root: &Path, relative: &Path) -> Result<PathBuf, LauncherError> {
        if relative.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        }) {
            return Err(LauncherError::invalid_path());
        }

        let canonical_root = root
            .canonicalize()
            .map_err(|_| LauncherError::invalid_path())?;
        let candidate = canonical_root.join(relative);
        let existing_ancestor = existing_ancestor(&candidate)?;
        let canonical_ancestor = existing_ancestor
            .canonicalize()
            .map_err(|_| LauncherError::invalid_path())?;

        if !canonical_ancestor.starts_with(&canonical_root) {
            return Err(LauncherError::invalid_path());
        }

        Ok(candidate)
    }
}

fn existing_ancestor(path: &Path) -> Result<&Path, LauncherError> {
    let mut ancestor = path;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or_else(LauncherError::invalid_path)?;
    }

    Ok(ancestor)
}

#[cfg(test)]
mod tests {
    use super::AppPaths;
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
            assert_eq!(error.code, "invalid_path");
        }

        fs::remove_dir_all(root).expect("temporary root is removed");
    }
}
