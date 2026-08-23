use crate::error::LauncherError;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Deserialize)]
pub(super) struct AssetIndexDocument {
    pub objects: BTreeMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AssetObject {
    pub hash: String,
    pub size: u64,
}

pub fn asset_object_path(hash: &str) -> Result<PathBuf, LauncherError> {
    if hash.len() != 40 || !hash.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(asset_index_invalid());
    }
    Ok(PathBuf::from("assets")
        .join("objects")
        .join(&hash[..2])
        .join(hash))
}

pub(super) fn asset_index_invalid() -> LauncherError {
    LauncherError::new(
        "asset_index_invalid",
        "The Minecraft asset index is invalid.",
        None,
        false,
    )
}
