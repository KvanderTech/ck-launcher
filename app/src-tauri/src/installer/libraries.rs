use super::path_safety::is_strict_windows_relative_path;
use crate::{
    error::LauncherError,
    metadata::models::{Library, Rule},
};
use regex::Regex;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const DEFAULT_LIBRARY_BASE: &str = "https://libraries.minecraft.net/";

#[derive(Debug, Clone)]
pub struct WindowsRuleContext {
    pub os_name: &'static str,
    pub architecture: &'static str,
    pub os_version: &'static str,
    pub features: BTreeMap<String, bool>,
}

impl Default for WindowsRuleContext {
    fn default() -> Self {
        Self {
            os_name: "windows",
            architecture: "x86_64",
            os_version: "10.0",
            features: BTreeMap::from([
                ("has_custom_resolution".to_owned(), false),
                ("is_demo_user".to_owned(), false),
                ("has_quick_plays_support".to_owned(), false),
                ("is_quick_play_singleplayer".to_owned(), false),
                ("is_quick_play_multiplayer".to_owned(), false),
                ("is_quick_play_realms".to_owned(), false),
            ]),
        }
    }
}

pub fn library_allowed(
    library: &Library,
    context: &WindowsRuleContext,
) -> Result<bool, LauncherError> {
    rules_allowed(&library.rules, context)
}

pub fn rules_allowed(rules: &[Rule], context: &WindowsRuleContext) -> Result<bool, LauncherError> {
    if rules.is_empty() {
        return Ok(true);
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, context)? {
            allowed = match rule.action.as_str() {
                "allow" => true,
                "disallow" => false,
                _ => return Err(metadata_invalid()),
            };
        }
    }
    Ok(allowed)
}

fn rule_matches(rule: &Rule, context: &WindowsRuleContext) -> Result<bool, LauncherError> {
    if let Some(os) = &rule.os {
        if os
            .name
            .as_deref()
            .is_some_and(|name| name != context.os_name)
        {
            return Ok(false);
        }
        if let Some(arch) = os.arch.as_deref() {
            let matches = match arch {
                "x86_64" | "amd64" => context.architecture == "x86_64",
                "x86" => context.architecture == "x86",
                other => other == context.architecture,
            };
            if !matches {
                return Ok(false);
            }
        }
        if let Some(pattern) = os.version.as_deref() {
            let regex = Regex::new(pattern).map_err(|_| metadata_invalid())?;
            if !regex.is_match(context.os_version) {
                return Ok(false);
            }
        }
    }
    Ok(rule.features.as_ref().is_none_or(|features| {
        features.iter().all(|(name, required)| {
            context.features.get(name).copied().unwrap_or(false) == *required
        })
    }))
}

pub fn maven_artifact_path(coordinate: &str) -> Result<PathBuf, LauncherError> {
    let mut extension_split = coordinate.split('@');
    let coordinate = extension_split.next().ok_or_else(metadata_invalid)?;
    let extension = extension_split.next().unwrap_or("jar");
    if extension_split.next().is_some() || !safe_token(extension) {
        return Err(metadata_invalid());
    }
    let parts = coordinate.split(':').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| !safe_token(part)) {
        return Err(metadata_invalid());
    }
    let group = parts[0];
    if group.split('.').any(|part| !safe_token(part)) {
        return Err(metadata_invalid());
    }
    let name = parts[1];
    let version = parts[2];
    let classifier = parts
        .get(3)
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    let mut path = PathBuf::new();
    for component in group.split('.') {
        path.push(component);
    }
    path.push(name);
    path.push(version);
    path.push(format!("{name}-{version}{classifier}.{extension}"));
    if !is_strict_windows_relative_path(&path) {
        return Err(metadata_invalid());
    }
    Ok(path)
}

pub(super) fn validate_metadata_path(path: &str) -> Result<PathBuf, LauncherError> {
    let path = Path::new(path);
    if !is_strict_windows_relative_path(path) {
        return Err(metadata_invalid());
    }
    Ok(path.to_path_buf())
}

pub(super) fn library_url(base: Option<&str>, path: &Path) -> Result<String, LauncherError> {
    let base =
        url::Url::parse(base.unwrap_or(DEFAULT_LIBRARY_BASE)).map_err(|_| metadata_invalid())?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err(metadata_invalid());
    }
    base.join(&path.to_string_lossy().replace('\\', "/"))
        .map(String::from)
        .map_err(|_| metadata_invalid())
}

fn safe_token(token: &str) -> bool {
    !token.is_empty()
        && token != "."
        && token != ".."
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn metadata_invalid() -> LauncherError {
    LauncherError::metadata_invalid()
}
