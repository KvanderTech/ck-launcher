use crate::{
    error::LauncherError,
    installer::libraries::{library_allowed, maven_artifact_path, WindowsRuleContext},
    metadata::models::ResolvedVersion,
    paths::AppPaths,
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub(super) fn build_classpath(
    game_root: &Path,
    version: &ResolvedVersion,
) -> Result<(String, Vec<PathBuf>), LauncherError> {
    let safety = AppPaths::new(game_root.to_path_buf());
    let context = WindowsRuleContext::default();
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for library in &version.libraries {
        if !library_allowed(library, &context)? {
            continue;
        }
        let relative = match library
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
        {
            Some(artifact) => artifact
                .path
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or(maven_artifact_path(&library.name)?),
            None if library.natives.is_none() => maven_artifact_path(&library.name)?,
            None => continue,
        };
        let relative = Path::new("libraries").join(relative);
        let validated = safety.safe_join(game_root, &relative)?;
        validate_regular_file(&validated)?;
        push_unique(&mut paths, &mut seen, game_root.join(relative));
    }
    let client_relative = Path::new("versions")
        .join(&version.id)
        .join(format!("{}.jar", version.id));
    let validated_client = safety.safe_join(game_root, &client_relative)?;
    validate_regular_file(&validated_client)?;
    push_unique(&mut paths, &mut seen, game_root.join(client_relative));
    let classpath = paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(";");
    Ok((classpath, paths))
}

pub(super) fn validate_regular_file(path: &Path) -> Result<(), LauncherError> {
    validate_components(path)?;
    let metadata = fs::metadata(path).map_err(|_| invalid_launch_path())?;
    if !metadata.is_file() {
        return Err(invalid_launch_path());
    }
    Ok(())
}

pub(super) fn validate_directory(path: &Path) -> Result<(), LauncherError> {
    validate_components(path)?;
    let metadata = fs::metadata(path).map_err(|_| invalid_launch_path())?;
    if !metadata.is_dir() {
        return Err(invalid_launch_path());
    }
    Ok(())
}

fn validate_components(path: &Path) -> Result<(), LauncherError> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for current in ancestors
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
    {
        let metadata = match fs::symlink_metadata(current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(invalid_launch_path()),
        };
        if is_reparse(&metadata) {
            return Err(invalid_launch_path());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn push_unique(paths: &mut Vec<PathBuf>, seen: &mut HashSet<String>, path: PathBuf) {
    let key = path.to_string_lossy().to_ascii_lowercase();
    if seen.insert(key) {
        paths.push(path);
    }
}

fn invalid_launch_path() -> LauncherError {
    LauncherError::new(
        "invalid_launch_path",
        "A required Minecraft launch path is missing or unsafe.",
        None,
        false,
    )
}
