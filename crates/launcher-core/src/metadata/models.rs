use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameVersionSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub release_date: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub sha1: Option<String>,
    #[serde(rename = "releaseTime")]
    pub release_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub downloads: VersionDownloads,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub logging: Option<Value>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub arguments: VersionArguments,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VersionDownloads {
    #[serde(default)]
    pub client: Option<Download>,
    #[serde(default)]
    pub server: Option<Download>,
    #[serde(flatten)]
    pub additional: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Download {
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub natives: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub extract: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Download>,
    #[serde(default)]
    pub classifiers: std::collections::BTreeMap<String, Download>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<std::collections::BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VersionArguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Argument {
    Literal(String),
    Conditional { rules: Vec<Rule>, value: Value },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedVersion {
    pub id: String,
    pub main_class: Option<String>,
    pub assets: Option<String>,
    pub asset_index: Option<AssetIndex>,
    pub downloads: VersionDownloads,
    pub libraries: Vec<Library>,
    pub logging: Option<Value>,
    pub java_version: Option<JavaVersion>,
    pub arguments: VersionArguments,
    pub minecraft_arguments: Option<String>,
}

impl From<VersionJson> for ResolvedVersion {
    fn from(version: VersionJson) -> Self {
        Self {
            id: version.id,
            main_class: version.main_class,
            assets: version.assets,
            asset_index: version.asset_index,
            downloads: version.downloads,
            libraries: version.libraries,
            logging: version.logging,
            java_version: version.java_version,
            arguments: version.arguments,
            minecraft_arguments: version.minecraft_arguments,
        }
    }
}

#[cfg(test)]
mod current_mojang_shape_tests {
    use super::VersionJson;

    #[test]
    fn accepts_camel_case_java_major_version() {
        let version: VersionJson = serde_json::from_str(
            r#"{"id":"26.2","javaVersion":{"component":"java-runtime-epsilon","majorVersion":25}}"#,
        )
        .expect("current Mojang javaVersion shape is supported");
        assert_eq!(
            version.java_version.expect("java version").major_version,
            25
        );
    }
}
