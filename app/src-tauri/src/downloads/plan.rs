use super::verify::verify_file;
use crate::{error::LauncherError, paths::AppPaths};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSpec {
    pub url: String,
    pub destination: PathBuf,
    pub expected_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedDownload {
    pub spec: DownloadSpec,
    pub relative_destination: PathBuf,
    pub relative_part: PathBuf,
    pub relative_lock: PathBuf,
}

pub(crate) struct DownloadPlan {
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub pending: Vec<PlannedDownload>,
}

pub(crate) fn build_plan(
    root: &Path,
    specs: Vec<DownloadSpec>,
) -> Result<DownloadPlan, LauncherError> {
    let safety = AppPaths::new(root.to_path_buf());
    let mut total_bytes = 0_u64;
    let mut completed_bytes = 0_u64;
    let mut pending = Vec::new();
    let mut destinations = HashSet::new();

    for spec in specs {
        validate_spec(&spec)?;
        total_bytes = total_bytes
            .checked_add(spec.expected_size)
            .ok_or_else(invalid_spec)?;
        let relative_destination = destination_relative_to(root, &spec.destination)?;
        let destination = safety.safe_join(root, &relative_destination)?;
        if !destinations.insert(destination) {
            return Err(invalid_spec());
        }
        if verify_file(root, &relative_destination, &spec)? {
            completed_bytes = completed_bytes
                .checked_add(spec.expected_size)
                .ok_or_else(invalid_spec)?;
            continue;
        }
        let relative_part = sibling_with_suffix(&relative_destination, ".part")?;
        let relative_lock = sibling_with_suffix(&relative_destination, ".part.lock")?;
        safety.safe_join(root, &relative_part)?;
        safety.safe_join(root, &relative_lock)?;
        pending.push(PlannedDownload {
            spec,
            relative_destination,
            relative_part,
            relative_lock,
        });
    }

    Ok(DownloadPlan {
        total_bytes,
        completed_bytes,
        pending,
    })
}

fn validate_spec(spec: &DownloadSpec) -> Result<(), LauncherError> {
    let url = Url::parse(&spec.url).map_err(|_| invalid_spec())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid_spec());
    }
    if !hash_is_valid(spec.sha1.as_deref(), 40) || !hash_is_valid(spec.sha256.as_deref(), 64) {
        return Err(invalid_spec());
    }
    Ok(())
}

fn hash_is_valid(hash: Option<&str>, length: usize) -> bool {
    hash.is_none_or(|hash| {
        hash.len() == length && hash.as_bytes().iter().all(u8::is_ascii_hexdigit)
    })
}

fn destination_relative_to(root: &Path, destination: &Path) -> Result<PathBuf, LauncherError> {
    let relative = if destination.is_absolute() {
        destination
            .strip_prefix(root)
            .or_else(|_| {
                let root = root.to_string_lossy();
                let root = root.strip_prefix(r"\\?\").unwrap_or(&root);
                destination.strip_prefix(Path::new(root))
            })
            .map_err(|_| LauncherError::invalid_path())?
            .to_path_buf()
    } else {
        destination.to_path_buf()
    };
    if relative.as_os_str().is_empty() || relative.file_name().is_none() {
        return Err(LauncherError::invalid_path());
    }
    Ok(relative)
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, LauncherError> {
    let name = path.file_name().ok_or_else(LauncherError::invalid_path)?;
    let mut suffixed = OsString::from(name);
    suffixed.push(suffix);
    Ok(path.with_file_name(suffixed))
}

fn invalid_spec() -> LauncherError {
    LauncherError::new(
        "download_spec_invalid",
        "Download metadata is invalid.",
        None,
        false,
    )
}
