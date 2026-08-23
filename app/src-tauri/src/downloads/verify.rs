use super::plan::DownloadSpec;
use crate::{error::LauncherError, paths::AppPaths};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use std::{
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

pub(crate) fn verify_file(
    root: &Path,
    relative: &Path,
    spec: &DownloadSpec,
) -> Result<bool, LauncherError> {
    let path = validated_path(root, relative)?;
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(storage_error()),
    };
    if !metadata.file_type().is_file() || metadata.len() != spec.expected_size {
        return Ok(false);
    }
    if spec.sha1.is_none() && spec.sha256.is_none() {
        return Ok(true);
    }

    let file = File::open(&path).map_err(|_| storage_error())?;
    let mut reader = BufReader::new(file);
    let mut sha1 = spec.sha1.as_ref().map(|_| Sha1::new());
    let mut sha256 = spec.sha256.as_ref().map(|_| Sha256::new());
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|_| storage_error())?;
        if read == 0 {
            break;
        }
        if let Some(digest) = &mut sha1 {
            digest.update(&buffer[..read]);
        }
        if let Some(digest) = &mut sha256 {
            digest.update(&buffer[..read]);
        }
    }
    if let (Some(expected), Some(actual)) = (&spec.sha1, sha1) {
        if !format!("{:x}", actual.finalize()).eq_ignore_ascii_case(expected) {
            return Ok(false);
        }
    }
    if let (Some(expected), Some(actual)) = (&spec.sha256, sha256) {
        if !format!("{:x}", actual.finalize()).eq_ignore_ascii_case(expected) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn validated_path(root: &Path, relative: &Path) -> Result<PathBuf, LauncherError> {
    AppPaths::new(root.to_path_buf()).safe_join(root, relative)
}

pub(crate) fn storage_error() -> LauncherError {
    LauncherError::new(
        "download_storage_failed",
        "The downloaded file could not be stored safely.",
        None,
        true,
    )
}
