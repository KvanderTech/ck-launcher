use super::verify::verify_file;
use crate::{error::LauncherError, paths::AppPaths};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    ffi::OsString,
    path::{Component, Path, PathBuf},
};
use url::Url;

const PART_SUFFIX: &str = ".part";
const LOCK_SUFFIX: &str = ".part.lock";

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

#[cfg(test)]
pub(crate) fn build_plan(
    root: &Path,
    specs: Vec<DownloadSpec>,
) -> Result<DownloadPlan, LauncherError> {
    build_plan_with_alias(root, root, specs)
}

pub(crate) fn build_plan_with_alias(
    root: &Path,
    original_root: &Path,
    specs: Vec<DownloadSpec>,
) -> Result<DownloadPlan, LauncherError> {
    let safety = AppPaths::new(root.to_path_buf());
    let mut total_bytes = 0_u64;
    let mut completed_bytes = 0_u64;
    let mut candidates = Vec::new();
    let mut owned_paths = Vec::new();

    for spec in specs {
        validate_spec(&spec)?;
        total_bytes = total_bytes
            .checked_add(spec.expected_size)
            .ok_or_else(invalid_spec)?;
        let relative_destination = destination_relative_to(root, &spec.destination)
            .or_else(|_| destination_relative_to(original_root, &spec.destination))?;
        // These suffixes are exclusively queue-owned. Reserving them in every component of every
        // plan means no final subtree can contain another execution's part or ownership marker.
        if has_internal_component(&relative_destination) {
            return Err(invalid_spec());
        }
        let destination = safety.safe_join(root, &relative_destination)?;
        let relative_part = sibling_with_suffix(&relative_destination, PART_SUFFIX)?;
        let relative_lock = sibling_with_suffix(&relative_destination, LOCK_SUFFIX)?;
        let part = safety.safe_join(root, &relative_part)?;
        let lock = safety.safe_join(root, &relative_lock)?;
        for path in [destination, part, lock] {
            owned_paths.push(path_identity(path));
        }
        candidates.push(PlannedDownload {
            spec,
            relative_destination,
            relative_part,
            relative_lock,
        });
    }

    owned_paths.sort_unstable_by(compare_path_identities);
    if owned_paths
        .windows(2)
        .any(|paths| compare_path_identities(&paths[0], &paths[1]).is_eq())
    {
        return Err(invalid_spec());
    }

    let mut pending = Vec::new();
    for candidate in candidates {
        if verify_file(root, &candidate.relative_destination, &candidate.spec)? {
            completed_bytes = completed_bytes
                .checked_add(candidate.spec.expected_size)
                .ok_or_else(invalid_spec)?;
            continue;
        }
        pending.push(candidate);
    }

    Ok(DownloadPlan {
        total_bytes,
        completed_bytes,
        pending,
    })
}

#[cfg(windows)]
type PathIdentity = Vec<u16>;

#[cfg(not(windows))]
type PathIdentity = PathBuf;

#[cfg(windows)]
fn path_identity(path: PathBuf) -> PathIdentity {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().collect()
}

#[cfg(not(windows))]
fn path_identity(path: PathBuf) -> PathIdentity {
    path
}

#[cfg(windows)]
fn compare_path_identities(left: &PathIdentity, right: &PathIdentity) -> Ordering {
    compare_utf16_case_insensitive(left, right)
}

#[cfg(not(windows))]
fn compare_path_identities(left: &PathIdentity, right: &PathIdentity) -> Ordering {
    left.cmp(right)
}

#[cfg(windows)]
fn compare_utf16_case_insensitive(left: &[u16], right: &[u16]) -> Ordering {
    use windows_sys::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};

    // SAFETY: both slices remain alive for the call and explicit lengths avoid a terminator.
    let result = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    };
    result.cmp(&CSTR_EQUAL)
}

fn has_internal_component(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        filename_ends_with(name, PART_SUFFIX) || filename_ends_with(name, LOCK_SUFFIX)
    })
}

#[cfg(windows)]
fn filename_ends_with(name: &std::ffi::OsStr, suffix: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let name = name.encode_wide().collect::<Vec<_>>();
    let suffix = suffix.encode_utf16().collect::<Vec<_>>();
    name.len() >= suffix.len()
        && compare_utf16_case_insensitive(&name[name.len() - suffix.len()..], &suffix).is_eq()
}

#[cfg(not(windows))]
fn filename_ends_with(name: &std::ffi::OsStr, suffix: &str) -> bool {
    name.to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(suffix)
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
                destination.strip_prefix(crate::paths::strip_verbatim_prefix(root.to_path_buf()))
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
