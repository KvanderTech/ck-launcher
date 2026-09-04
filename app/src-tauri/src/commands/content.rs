use crate::{
    error::LauncherError,
    metadata::{models::VersionJson, resolver::MetadataService},
    paths::AppPaths,
    storage::{BuildSummary, InstalledContent, OfflineSkin, Storage},
};
use base64::Engine;
use futures_util::{stream, StreamExt};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::State;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

#[derive(Default)]
pub struct PendingMrpackPath(pub Mutex<Option<String>>);

impl PendingMrpackPath {
    pub fn replace(&self, path: String) {
        if let Ok(mut pending) = self.0.lock() {
            *pending = Some(path);
        }
    }
}

#[derive(Clone)]
pub struct ContentService {
    client: Client,
    paths: AppPaths,
    storage: Storage,
    metadata: Arc<MetadataService>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModrinthSearchResult {
    pub hits: Vec<ModrinthProject>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthProject {
    pub project_id: String,
    pub project_type: String,
    pub title: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub date_modified: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDetails {
    id: String,
    title: String,
    project_type: String,
    icon_url: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    followers: u64,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectVersion {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    dependencies: Vec<ProjectDependency>,
    #[serde(default)]
    files: Vec<ProjectFile>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    game_versions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ModrinthVersionView {
    id: String,
    name: String,
    version_number: String,
    version_type: String,
    date_published: String,
    downloads: u64,
    loaders: Vec<String>,
    game_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectDependency {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectFile {
    hashes: FileHashes,
    url: String,
    filename: String,
    primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct FileHashes {
    sha512: Option<String>,
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderEntry {
    loader: FabricLoaderVersion,
}
#[derive(Debug, Deserialize)]
struct FabricLoaderVersion {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderEntry {
    loader: QuiltLoaderVersion,
}
#[derive(Debug, Deserialize)]
struct QuiltLoaderVersion {
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrpackIndex {
    format_version: u32,
    game: String,
    version_id: String,
    name: String,
    summary: Option<String>,
    files: Vec<MrpackFile>,
    dependencies: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MrpackFile {
    path: String,
    hashes: FileHashes,
    downloads: Vec<String>,
    env: Option<MrpackEnvironment>,
}

#[derive(Debug, Deserialize)]
struct MrpackEnvironment {
    client: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineSkinView {
    id: String,
    account_id: String,
    name: String,
    data_url: String,
    is_active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildFileEntry {
    name: String,
    relative_path: String,
    kind: String,
    size: u64,
    modified_at: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildWorldSummary {
    name: String,
    relative_path: String,
    size: u64,
    modified_at: u64,
}

pub type BuildLogSummary = BuildFileEntry;

impl ContentService {
    pub fn new(
        paths: AppPaths,
        storage: Storage,
        metadata: Arc<MetadataService>,
    ) -> Result<Self, LauncherError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(90))
            .default_headers({
                let mut headers = header::HeaderMap::new();
                headers.insert(
                    header::USER_AGENT,
                    header::HeaderValue::from_static("CKLauncher/0.1.0 (desktop launcher)"),
                );
                headers
            })
            .build()
            .map_err(|_| network_error())?;
        fs::create_dir_all(paths.root.join("instances"))
            .map_err(|_| LauncherError::storage_unavailable())?;
        fs::create_dir_all(paths.root.join("skins"))
            .map_err(|_| LauncherError::storage_unavailable())?;
        Ok(Self {
            client,
            paths,
            storage,
            metadata,
        })
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, LauncherError> {
        self.client
            .get(url)
            .send()
            .await
            .map_err(|_| network_error())?
            .error_for_status()
            .map_err(|_| network_error())?
            .json()
            .await
            .map_err(|_| network_error())
    }

    pub async fn search(
        &self,
        query: String,
        project_type: String,
        game_version: Option<String>,
        loader: Option<String>,
        category: Option<String>,
        environment: Option<String>,
        index: Option<String>,
        offset: u32,
    ) -> Result<ModrinthSearchResult, LauncherError> {
        let mut facets = vec![vec![format!("project_type:{project_type}")]];
        if let Some(version) = game_version.filter(|value| !value.is_empty()) {
            facets.push(vec![format!("versions:{version}")]);
        }
        if let Some(loader) = loader.filter(|value| !value.is_empty() && value != "vanilla") {
            facets.push(vec![format!("categories:{loader}")]);
        }
        if let Some(category) = category.filter(|value| !value.is_empty()) {
            facets.push(vec![format!("categories:{category}")]);
        }
        if let Some(environment) = environment.filter(|value| !value.is_empty()) {
            facets.push(vec![
                format!("{environment}_side:required"),
                format!("{environment}_side:optional"),
            ]);
        }
        let index = index
            .filter(|value| {
                ["relevance", "downloads", "follows", "newest", "updated"].contains(&value.as_str())
            })
            .unwrap_or_else(|| "relevance".to_owned());
        self.client
            .get(format!("{MODRINTH_API}/search"))
            .query(&[
                ("query", query),
                ("facets", serde_json::to_string(&facets).unwrap_or_default()),
                ("index", index),
                ("offset", offset.to_string()),
                ("limit", "20".to_owned()),
            ])
            .send()
            .await
            .map_err(|_| network_error())?
            .error_for_status()
            .map_err(|_| network_error())?
            .json()
            .await
            .map_err(|_| network_error())
    }

    pub async fn create_build(
        &self,
        name: String,
        game_version: String,
        loader: String,
    ) -> Result<BuildSummary, LauncherError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 48 {
            return Err(input_error(
                "build_name_invalid",
                "Введите название сборки до 48 символов.",
            ));
        }
        if !["vanilla", "fabric", "quilt"].contains(&loader.as_str()) {
            return Err(input_error(
                "loader_not_supported",
                "Сейчас поддерживаются Vanilla, Fabric и Quilt.",
            ));
        }
        let id = format!(
            "build-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let game_dir = self.paths.root.join("instances").join(&id);
        fs::create_dir_all(&game_dir).map_err(|_| LauncherError::storage_unavailable())?;
        let loader_version = match loader.as_str() {
            "fabric" => Some(self.install_fabric_profile(&game_version, None).await?),
            "quilt" => Some(self.install_quilt_profile(&game_version, None).await?),
            _ => None,
        };
        let version_id = loader_version
            .as_ref()
            .map(|value| format!("{loader}-loader-{value}-{game_version}"))
            .unwrap_or_else(|| game_version.clone());
        let build = BuildSummary {
            id,
            name: name.to_owned(),
            game_version: version_id,
            loader,
            loader_version,
            game_dir: game_dir.to_string_lossy().into_owned(),
            icon_url: None,
            is_active: true,
        };
        self.storage.upsert_build(&build).await?;
        self.storage.select_build(&build.id).await
    }

    async fn install_fabric_profile(
        &self,
        game_version: &str,
        requested: Option<&str>,
    ) -> Result<String, LauncherError> {
        let versions: Vec<FabricLoaderEntry> = self
            .json(&format!(
                "https://meta.fabricmc.net/v2/versions/loader/{game_version}"
            ))
            .await?;
        let loader = requested
            .map(str::to_owned)
            .or_else(|| {
                versions
                    .iter()
                    .find(|entry| entry.loader.stable)
                    .map(|entry| entry.loader.version.clone())
            })
            .or_else(|| versions.first().map(|entry| entry.loader.version.clone()))
            .ok_or_else(|| {
                input_error(
                    "fabric_unavailable",
                    "Для этой версии Minecraft не найден Fabric Loader.",
                )
            })?;
        let mut profile: VersionJson = self
            .json(&format!(
                "https://meta.fabricmc.net/v2/versions/loader/{game_version}/{loader}/profile/json"
            ))
            .await?;
        profile.id = format!("fabric-loader-{loader}-{game_version}");
        self.metadata.register_custom_version(&profile)?;
        Ok(loader)
    }

    async fn install_quilt_profile(
        &self,
        game_version: &str,
        requested: Option<&str>,
    ) -> Result<String, LauncherError> {
        let versions: Vec<QuiltLoaderEntry> = self
            .json(&format!(
                "https://meta.quiltmc.org/v3/versions/loader/{game_version}"
            ))
            .await?;
        let loader = requested
            .map(str::to_owned)
            .or_else(|| versions.first().map(|entry| entry.loader.version.clone()))
            .ok_or_else(|| {
                input_error(
                    "quilt_unavailable",
                    "Для этой версии Minecraft не найден Quilt Loader.",
                )
            })?;
        let mut profile: VersionJson = self
            .json(&format!(
                "https://meta.quiltmc.org/v3/versions/loader/{game_version}/{loader}/profile/json"
            ))
            .await?;
        profile.id = format!("quilt-loader-{loader}-{game_version}");
        self.metadata.register_custom_version(&profile)?;
        Ok(loader)
    }

    pub async fn install_project(
        &self,
        project_id: String,
        build_id: String,
        version_id: Option<String>,
    ) -> Result<InstalledContent, LauncherError> {
        let mut visiting = HashSet::new();
        self.install_project_inner(project_id, build_id, version_id, &mut visiting)
            .await
    }

    async fn install_project_inner(
        &self,
        project_id: String,
        build_id: String,
        requested_version_id: Option<String>,
        visiting: &mut HashSet<String>,
    ) -> Result<InstalledContent, LauncherError> {
        if !visiting.insert(project_id.clone()) {
            return Err(input_error(
                "dependency_cycle",
                "Modrinth вернул циклическую зависимость. Установка остановлена безопасно.",
            ));
        }
        let mut build = self
            .storage
            .list_builds()
            .await?
            .into_iter()
            .find(|item| item.id == build_id)
            .ok_or_else(|| input_error("build_not_found", "Выбранная сборка не найдена."))?;
        let project: ProjectDetails = self
            .json(&format!("{MODRINTH_API}/project/{project_id}"))
            .await?;
        let base_game_version = base_game_version(&build);
        let mut request = self
            .client
            .get(format!("{MODRINTH_API}/project/{}/version", project.id));
        let mut query = vec![
            (
                "game_versions",
                serde_json::to_string(&vec![base_game_version]).unwrap_or_default(),
            ),
            ("include_changelog", "false".to_owned()),
        ];
        if project.project_type != "resourcepack"
            && project.project_type != "shader"
            && build.loader != "vanilla"
        {
            query.push((
                "loaders",
                serde_json::to_string(&vec![&build.loader]).unwrap_or_default(),
            ));
        }
        request = request.query(&query);
        let versions: Vec<ProjectVersion> = request
            .send()
            .await
            .map_err(|_| network_error())?
            .error_for_status()
            .map_err(|_| network_error())?
            .json()
            .await
            .map_err(|_| network_error())?;
        let version = match requested_version_id.as_deref() {
            Some(id) => versions.iter().find(|version| version.id == id).cloned(),
            None => versions.into_iter().next(),
        }
        .ok_or_else(|| {
            input_error(
                "compatible_version_not_found",
                "Совместимая версия проекта не найдена.",
            )
        })?;
        if project.project_type == "modpack" {
            visiting.remove(&project_id);
            return self.install_mrpack(project, version, build).await;
        }
        if project.project_type == "mod" && build.loader == "vanilla" {
            let minecraft = base_game_version.to_owned();
            let installed = self.install_fabric_profile(&minecraft, None).await?;
            build.loader = "fabric".to_owned();
            build.loader_version = Some(installed.clone());
            build.game_version = format!("fabric-loader-{installed}-{minecraft}");
            self.storage.upsert_build(&build).await?;
        }
        let installed = self.storage.list_installed_content(&build.id).await?;
        for dependency in &version.dependencies {
            let dependency_project = dependency
                .project_id
                .as_deref()
                .or_else(|| dependency.version_id.as_deref());
            let Some(dependency_project) = dependency_project else {
                continue;
            };
            if dependency.dependency_type == "incompatible"
                && installed.iter().any(|item| {
                    item.project_id == dependency_project || item.version_id == dependency_project
                })
            {
                visiting.remove(&project_id);
                return Err(input_error(
                    "incompatible_content",
                    "Установка заблокирована: в сборке найден несовместимый проект.",
                ));
            }
        }
        for dependency in version
            .dependencies
            .iter()
            .filter(|item| item.dependency_type == "required")
        {
            let Some(dependency_project) = dependency.project_id.clone() else {
                continue;
            };
            if installed
                .iter()
                .any(|item| item.project_id == dependency_project)
            {
                continue;
            }
            Box::pin(self.install_project_inner(
                dependency_project,
                build.id.clone(),
                None,
                visiting,
            ))
            .await?;
        }
        let result = self.install_regular(project, version, build).await;
        visiting.remove(&project_id);
        result
    }

    pub async fn install_modpack_as_build(
        &self,
        project_id: String,
        version_id: Option<String>,
    ) -> Result<InstalledContent, LauncherError> {
        let project: ProjectDetails = self
            .json(&format!("{MODRINTH_API}/project/{project_id}"))
            .await?;
        if project.project_type != "modpack" {
            return Err(input_error(
                "modpack_required",
                "Этот проект не является готовой сборкой Modrinth.",
            ));
        }
        let versions: Vec<ProjectVersion> = self
            .client
            .get(format!("{MODRINTH_API}/project/{}/version", project.id))
            .query(&[("include_changelog", "false")])
            .send()
            .await
            .map_err(|_| network_error())?
            .error_for_status()
            .map_err(|_| network_error())?
            .json()
            .await
            .map_err(|_| network_error())?;
        let version = match version_id.as_deref() {
            Some(id) => versions.iter().find(|version| version.id == id).cloned(),
            None => versions.into_iter().next(),
        }
        .ok_or_else(|| {
            input_error(
                "compatible_version_not_found",
                "У этой сборки нет доступной версии для установки.",
            )
        })?;
        let id = format!(
            "build-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let game_dir = self.paths.root.join("instances").join(&id);
        fs::create_dir_all(&game_dir).map_err(|_| LauncherError::storage_unavailable())?;
        let build = BuildSummary {
            id: id.clone(),
            name: project.title.clone(),
            game_version: "pending".to_owned(),
            loader: "vanilla".to_owned(),
            loader_version: None,
            game_dir: game_dir.to_string_lossy().into_owned(),
            icon_url: project.icon_url.clone(),
            is_active: true,
        };
        self.storage.upsert_build(&build).await?;
        match self.install_mrpack(project, version, build).await {
            Ok(item) => Ok(item),
            Err(error) => {
                let _ = self.storage.delete_build(&id).await;
                let _ = fs::remove_dir_all(game_dir);
                Err(error)
            }
        }
    }

    pub async fn repair_build(&self, build_id: String) -> Result<BuildSummary, LauncherError> {
        let build = self
            .storage
            .list_builds()
            .await?
            .into_iter()
            .find(|item| item.id == build_id)
            .ok_or_else(|| input_error("build_not_found", "Выбранная сборка не найдена."))?;
        fs::create_dir_all(&build.game_dir).map_err(|_| LauncherError::storage_unavailable())?;

        let minecraft = base_game_version(&build).to_owned();
        match build.loader.as_str() {
            "fabric" => {
                self.install_fabric_profile(&minecraft, build.loader_version.as_deref())
                    .await?;
            }
            "quilt" => {
                self.install_quilt_profile(&minecraft, build.loader_version.as_deref())
                    .await?;
            }
            _ => {}
        }

        let installed = self.storage.list_installed_content(&build.id).await?;
        if let Some(modpack) = installed.iter().find(|item| item.project_type == "modpack") {
            let project: ProjectDetails = self
                .json(&format!("{MODRINTH_API}/project/{}", modpack.project_id))
                .await?;
            let version: ProjectVersion = self
                .json(&format!("{MODRINTH_API}/version/{}", modpack.version_id))
                .await?;
            self.install_mrpack(project, version, build.clone()).await?;
        } else {
            for item in installed
                .into_iter()
                .filter(|item| item.version_id != "local")
            {
                let project: ProjectDetails = self
                    .json(&format!("{MODRINTH_API}/project/{}", item.project_id))
                    .await?;
                let version: ProjectVersion = self
                    .json(&format!("{MODRINTH_API}/version/{}", item.version_id))
                    .await?;
                self.install_regular(project, version, build.clone())
                    .await?;
            }
        }
        self.storage.select_build(&build.id).await
    }

    async fn install_regular(
        &self,
        project: ProjectDetails,
        version: ProjectVersion,
        build: BuildSummary,
    ) -> Result<InstalledContent, LauncherError> {
        let file = version
            .files
            .iter()
            .find(|file| file.primary)
            .or_else(|| version.files.first())
            .ok_or_else(|| network_error())?
            .clone();
        let folder = match project.project_type.as_str() {
            "mod" => "mods",
            "resourcepack" => "resourcepacks",
            "shader" => "shaderpacks",
            _ => {
                return Err(input_error(
                    "project_type_unsupported",
                    "Этот тип проекта пока не поддерживается.",
                ))
            }
        };
        let target_dir = PathBuf::from(&build.game_dir).join(folder);
        fs::create_dir_all(&target_dir).map_err(|_| LauncherError::storage_unavailable())?;
        let target = safe_child(&target_dir, &file.filename)?;
        self.download_verified(
            &file.url,
            &target,
            file.hashes.sha512.as_deref(),
            file.hashes.sha1.as_deref(),
        )
        .await?;
        let item = InstalledContent {
            id: format!("{}:{}", build.id, project.id),
            build_id: build.id,
            project_id: project.id,
            version_id: version.id,
            project_type: project.project_type,
            title: project.title,
            filename: file.filename,
            icon_url: project.icon_url,
            enabled: true,
        };
        self.storage.upsert_installed_content(&item).await?;
        Ok(item)
    }

    async fn install_mrpack(
        &self,
        project: ProjectDetails,
        version: ProjectVersion,
        build: BuildSummary,
    ) -> Result<InstalledContent, LauncherError> {
        let file = version
            .files
            .iter()
            .find(|file| file.primary)
            .or_else(|| version.files.first())
            .ok_or_else(network_error)?
            .clone();
        let bytes = self
            .download_bytes(
                &file.url,
                file.hashes.sha512.as_deref(),
                file.hashes.sha1.as_deref(),
            )
            .await?;
        self.install_mrpack_bytes(project, version.id, file.filename, bytes, build)
            .await
    }

    async fn install_mrpack_bytes(
        &self,
        project: ProjectDetails,
        pack_version_id: String,
        pack_filename: String,
        bytes: Vec<u8>,
        mut build: BuildSummary,
    ) -> Result<InstalledContent, LauncherError> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| input_error("mrpack_invalid", "Файл сборки Modrinth повреждён."))?;
        let index: MrpackIndex = {
            let mut entry = archive
                .by_name("modrinth.index.json")
                .map_err(|_| input_error("mrpack_invalid", "В сборке нет modrinth.index.json."))?;
            let mut body = String::new();
            entry.read_to_string(&mut body).map_err(|_| {
                input_error("mrpack_invalid", "Не удалось прочитать индекс сборки.")
            })?;
            serde_json::from_str(&body).map_err(|_| {
                input_error("mrpack_invalid", "Индекс сборки имеет неверный формат.")
            })?
        };
        if index.format_version != 1 || index.game != "minecraft" {
            return Err(input_error(
                "mrpack_invalid",
                "Неподдерживаемый формат сборки.",
            ));
        }
        let minecraft = index
            .dependencies
            .get("minecraft")
            .cloned()
            .ok_or_else(|| {
                input_error("mrpack_invalid", "В сборке не указана версия Minecraft.")
            })?;
        let (loader, loader_version, version_id) =
            if let Some(fabric) = index.dependencies.get("fabric-loader") {
                let installed = self
                    .install_fabric_profile(&minecraft, Some(fabric))
                    .await?;
                (
                    "fabric".to_owned(),
                    Some(installed.clone()),
                    format!("fabric-loader-{installed}-{minecraft}"),
                )
            } else if let Some(quilt) = index.dependencies.get("quilt-loader") {
                let installed = self.install_quilt_profile(&minecraft, Some(quilt)).await?;
                (
                    "quilt".to_owned(),
                    Some(installed.clone()),
                    format!("quilt-loader-{installed}-{minecraft}"),
                )
            } else {
                ("vanilla".to_owned(), None, minecraft.clone())
            };
        build.name = index.name.clone();
        build.game_version = version_id;
        build.loader = loader;
        build.loader_version = loader_version;
        build.icon_url = project.icon_url.clone();
        self.storage.upsert_build(&build).await?;
        let mut referenced_versions = Vec::new();
        for entry in index.files {
            if entry.env.as_ref().and_then(|env| env.client.as_deref()) == Some("unsupported") {
                continue;
            }
            let url = entry.downloads.first().ok_or_else(network_error)?;
            let target = safe_relative(Path::new(&build.game_dir), &entry.path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|_| LauncherError::storage_unavailable())?;
            }
            self.download_verified(
                url,
                &target,
                entry.hashes.sha512.as_deref(),
                entry.hashes.sha1.as_deref(),
            )
            .await?;
            if let Some((project_id, version_id)) = modrinth_ids_from_download(url) {
                let project_type = if entry.path.starts_with("mods/") {
                    "mod"
                } else if entry.path.starts_with("resourcepacks/") {
                    "resourcepack"
                } else if entry.path.starts_with("shaderpacks/") {
                    "shader"
                } else {
                    continue;
                };
                let filename = Path::new(&entry.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("content");
                self.storage
                    .upsert_installed_content(&InstalledContent {
                        id: format!("{}:{}", build.id, project_id),
                        build_id: build.id.clone(),
                        project_id: project_id.clone(),
                        version_id: version_id.clone(),
                        project_type: project_type.to_owned(),
                        title: filename
                            .trim_end_matches(".jar")
                            .trim_end_matches(".zip")
                            .to_owned(),
                        filename: filename.to_owned(),
                        icon_url: None,
                        enabled: true,
                    })
                    .await?;
                if project_type == "mod" {
                    referenced_versions.push(version_id);
                }
            }
        }
        // mrpack indexes may omit files that are declared as required dependencies
        // by an exact project version. Resolve those edges after the indexed files
        // are present so a partially specified pack cannot fail at Fabric startup.
        for version_id in referenced_versions {
            let exact: ProjectVersion = self
                .json(&format!("{MODRINTH_API}/version/{version_id}"))
                .await?;
            for dependency in exact
                .dependencies
                .into_iter()
                .filter(|item| item.dependency_type == "required")
            {
                let Some(dependency_project) = dependency.project_id else {
                    continue;
                };
                if self
                    .storage
                    .list_installed_content(&build.id)
                    .await?
                    .iter()
                    .any(|item| item.project_id == dependency_project)
                {
                    continue;
                }
                let mut visiting = HashSet::new();
                Box::pin(self.install_project_inner(
                    dependency_project,
                    build.id.clone(),
                    None,
                    &mut visiting,
                ))
                .await?;
            }
        }
        for prefix in ["overrides/", "client-overrides/"] {
            for index in 0..archive.len() {
                let mut entry = archive
                    .by_index(index)
                    .map_err(|_| input_error("mrpack_invalid", "Не удалось распаковать сборку."))?;
                let name = entry.name().replace('\\', "/");
                let Some(relative) = name.strip_prefix(prefix) else {
                    continue;
                };
                if relative.is_empty() || entry.is_dir() {
                    continue;
                }
                let target = safe_relative(Path::new(&build.game_dir), relative)?;
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|_| LauncherError::storage_unavailable())?;
                }
                let mut output =
                    fs::File::create(target).map_err(|_| LauncherError::storage_unavailable())?;
                std::io::copy(&mut entry, &mut output)
                    .map_err(|_| LauncherError::storage_unavailable())?;
            }
        }
        let item = InstalledContent {
            id: format!("{}:{}", build.id, project.id),
            build_id: build.id.clone(),
            project_id: project.id,
            version_id: pack_version_id,
            project_type: project.project_type,
            title: index.name,
            filename: pack_filename,
            icon_url: project.icon_url,
            enabled: true,
        };
        self.storage.upsert_installed_content(&item).await?;
        self.storage.select_build(&build.id).await?;
        Ok(item)
    }

    pub async fn install_local_mrpack(
        &self,
        source: PathBuf,
    ) -> Result<InstalledContent, LauncherError> {
        if source
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("mrpack"))
            != Some(true)
        {
            return Err(input_error(
                "mrpack_extension_invalid",
                "Выберите файл сборки с расширением .mrpack.",
            ));
        }
        let bytes = fs::read(&source)
            .map_err(|_| input_error("mrpack_read_failed", "Не удалось прочитать файл сборки."))?;
        if bytes.len() > 1_500_000_000 {
            return Err(input_error(
                "mrpack_too_large",
                "Файл сборки слишком большой.",
            ));
        }
        let seed = format!("{}:{}", source.to_string_lossy(), bytes.len());
        let project_id = format!("local-mrpack-{:x}", Sha256::digest(seed.as_bytes()));
        let id = format!(
            "build-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let game_dir = self.paths.root.join("instances").join(&id);
        fs::create_dir_all(&game_dir).map_err(|_| LauncherError::storage_unavailable())?;
        let filename = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("local.mrpack")
            .to_owned();
        let project = ProjectDetails {
            id: project_id,
            title: filename.trim_end_matches(".mrpack").to_owned(),
            project_type: "modpack".to_owned(),
            icon_url: None,
            description: String::new(),
            body: String::new(),
            downloads: 0,
            followers: 0,
            categories: Vec::new(),
        };
        let build = BuildSummary {
            id: id.clone(),
            name: project.title.clone(),
            game_version: "pending".to_owned(),
            loader: "vanilla".to_owned(),
            loader_version: None,
            game_dir: game_dir.to_string_lossy().into_owned(),
            icon_url: None,
            is_active: true,
        };
        self.storage.upsert_build(&build).await?;
        match self
            .install_mrpack_bytes(project, "local".to_owned(), filename, bytes, build)
            .await
        {
            Ok(item) => Ok(item),
            Err(error) => {
                let _ = self.storage.delete_build(&id).await;
                let _ = fs::remove_dir_all(game_dir);
                Err(error)
            }
        }
    }

    async fn download_verified(
        &self,
        url: &str,
        target: &Path,
        sha512: Option<&str>,
        sha1: Option<&str>,
    ) -> Result<(), LauncherError> {
        let bytes = self.download_bytes(url, sha512, sha1).await?;
        let temporary = target.with_extension(format!(
            "{}.part",
            target
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("download")
        ));
        fs::write(&temporary, bytes).map_err(|_| LauncherError::storage_unavailable())?;
        fs::rename(&temporary, target).map_err(|_| {
            let _ = fs::remove_file(&temporary);
            LauncherError::storage_unavailable()
        })
    }

    async fn download_bytes(
        &self,
        url: &str,
        sha512: Option<&str>,
        sha1: Option<&str>,
    ) -> Result<Vec<u8>, LauncherError> {
        let bytes = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| network_error())?
            .error_for_status()
            .map_err(|_| network_error())?
            .bytes()
            .await
            .map_err(|_| network_error())?
            .to_vec();
        if let Some(expected) = sha512 {
            if format!("{:x}", Sha512::digest(&bytes)) != expected.to_ascii_lowercase() {
                return Err(input_error(
                    "download_hash_mismatch",
                    "Контрольная сумма файла не совпала.",
                ));
            }
        } else if let Some(expected) = sha1 {
            if format!("{:x}", Sha1::digest(&bytes)) != expected.to_ascii_lowercase() {
                return Err(input_error(
                    "download_hash_mismatch",
                    "Контрольная сумма файла не совпала.",
                ));
            }
        }
        Ok(bytes)
    }

    fn skin_view(&self, skin: OfflineSkin) -> Result<OfflineSkinView, LauncherError> {
        let bytes = fs::read(&skin.file_path).map_err(|_| LauncherError::storage_unavailable())?;
        Ok(OfflineSkinView {
            id: skin.id,
            account_id: skin.account_id,
            name: skin.name,
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ),
            is_active: skin.is_active,
        })
    }
}

fn modrinth_ids_from_download(download: &str) -> Option<(String, String)> {
    let url = url::Url::parse(download).ok()?;
    let parts: Vec<_> = url.path_segments()?.collect();
    let data = parts.iter().position(|part| *part == "data")?;
    if parts.get(data + 2)? != &"versions" {
        return None;
    }
    Some((
        parts.get(data + 1)?.to_string(),
        parts.get(data + 3)?.to_string(),
    ))
}

fn base_game_version(build: &BuildSummary) -> &str {
    if build.loader == "fabric" || build.loader == "quilt" {
        if let Some(loader_version) = build.loader_version.as_deref() {
            let prefix = format!("{}-loader-{}-", build.loader, loader_version);
            return build
                .game_version
                .strip_prefix(&prefix)
                .unwrap_or(&build.game_version);
        }
        &build.game_version
    } else {
        &build.game_version
    }
}

fn safe_child(root: &Path, filename: &str) -> Result<PathBuf, LauncherError> {
    safe_relative(root, filename)
}
fn safe_relative(root: &Path, relative: impl AsRef<Path>) -> Result<PathBuf, LauncherError> {
    let relative = relative.as_ref();
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(LauncherError::invalid_path());
    }
    Ok(root.join(relative))
}
fn network_error() -> LauncherError {
    LauncherError::new(
        "modrinth_unavailable",
        "Modrinth сейчас недоступен. Попробуйте ещё раз.",
        None,
        true,
    )
}
fn input_error(code: &'static str, message: &'static str) -> LauncherError {
    LauncherError::new(code, message, None, true)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn search_modrinth(
    query: String,
    project_type: String,
    game_version: Option<String>,
    loader: Option<String>,
    category: Option<String>,
    environment: Option<String>,
    index: Option<String>,
    offset: u32,
    service: State<'_, ContentService>,
) -> Result<ModrinthSearchResult, LauncherError> {
    service
        .search(
            query,
            project_type,
            game_version,
            loader,
            category,
            environment,
            index,
            offset,
        )
        .await
}
#[tauri::command(rename_all = "camelCase")]
pub async fn modrinth_project(
    project_id: String,
    service: State<'_, ContentService>,
) -> Result<ProjectDetails, LauncherError> {
    service
        .json(&format!("{MODRINTH_API}/project/{project_id}"))
        .await
}
#[tauri::command(rename_all = "camelCase")]
pub async fn modrinth_project_versions(
    project_id: String,
    service: State<'_, ContentService>,
) -> Result<Vec<ModrinthVersionView>, LauncherError> {
    let versions: Vec<ProjectVersion> = service
        .json(&format!("{MODRINTH_API}/project/{project_id}/version"))
        .await?;
    Ok(versions
        .into_iter()
        .map(|version| ModrinthVersionView {
            id: version.id,
            name: version.name,
            version_number: version.version_number,
            version_type: version.version_type,
            date_published: version.date_published,
            downloads: version.downloads,
            loaders: version.loaders,
            game_versions: version.game_versions,
        })
        .collect())
}
#[tauri::command(rename_all = "camelCase")]
pub async fn create_build(
    name: String,
    game_version: String,
    loader: String,
    service: State<'_, ContentService>,
) -> Result<BuildSummary, LauncherError> {
    service.create_build(name, game_version, loader).await
}
#[tauri::command]
pub async fn list_builds(storage: State<'_, Storage>) -> Result<Vec<BuildSummary>, LauncherError> {
    storage.list_builds().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn repair_build(
    build_id: String,
    service: State<'_, ContentService>,
) -> Result<BuildSummary, LauncherError> {
    service.repair_build(build_id).await
}
#[tauri::command(rename_all = "camelCase")]
pub async fn select_build(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<BuildSummary, LauncherError> {
    storage.select_build(&build_id).await
}
#[tauri::command(rename_all = "camelCase")]
pub async fn rename_build(
    build_id: String,
    name: String,
    storage: State<'_, Storage>,
) -> Result<BuildSummary, LauncherError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(input_error(
            "build_name_invalid",
            "Название должно содержать от 1 до 80 символов.",
        ));
    }
    storage
        .update_build_identity(&build_id, Some(name), None)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn choose_build_icon(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<Option<BuildSummary>, LauncherError> {
    let Some(source) = choose_build_icon_file()? else {
        return Ok(None);
    };
    let bytes = fs::read(source).map_err(|_| {
        input_error(
            "build_icon_read_failed",
            "Не удалось прочитать изображение.",
        )
    })?;
    if bytes.len() > 2_000_000 {
        return Err(input_error(
            "build_icon_too_large",
            "Размер изображения не должен превышать 2 МБ.",
        ));
    }
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return Err(input_error(
            "build_icon_invalid",
            "Выберите изображение PNG, JPG или WebP.",
        ));
    };
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    storage
        .update_build_identity(&build_id, None, Some(&data_url))
        .await
        .map(Some)
}
#[tauri::command(rename_all = "camelCase")]
pub async fn delete_build(
    build_id: String,
    service: State<'_, ContentService>,
) -> Result<(), LauncherError> {
    let build = service
        .storage
        .list_builds()
        .await?
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
    let source = PathBuf::from(&build.game_dir);
    let trash = service.paths.root.join("trash");
    fs::create_dir_all(&trash).map_err(|_| LauncherError::storage_unavailable())?;
    let destination = trash.join(format!(
        "{}-{}",
        build.id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    if source.exists() {
        fs::rename(&source, &destination).map_err(|_| {
            LauncherError::new(
                "build_trash_failed",
                "Не удалось переместить сборку в корзину.",
                None,
                true,
            )
        })?;
    }
    if let Err(error) = service.storage.delete_build(&build.id).await {
        if destination.exists() {
            let _ = fs::rename(&destination, &source);
        }
        return Err(error);
    }
    Ok(())
}
#[tauri::command(rename_all = "camelCase")]
pub async fn install_modrinth_project(
    project_id: String,
    build_id: String,
    version_id: Option<String>,
    service: State<'_, ContentService>,
) -> Result<InstalledContent, LauncherError> {
    service
        .install_project(project_id, build_id, version_id)
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_modrinth_modpack(
    project_id: String,
    version_id: Option<String>,
    service: State<'_, ContentService>,
) -> Result<InstalledContent, LauncherError> {
    service
        .install_modpack_as_build(project_id, version_id)
        .await
}
#[tauri::command(rename_all = "camelCase")]
pub async fn import_mrpack(
    source_path: Option<String>,
    service: State<'_, ContentService>,
) -> Result<Option<InstalledContent>, LauncherError> {
    let source = match source_path {
        Some(path) => Some(PathBuf::from(path)),
        None => choose_mrpack_file()?,
    };
    match source {
        Some(path) => service.install_local_mrpack(path).await.map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn pending_mrpack_path(pending: State<'_, PendingMrpackPath>) -> Option<String> {
    pending.0.lock().ok()?.take()
}
#[tauri::command(rename_all = "camelCase")]
pub async fn list_installed_content(
    build_id: String,
    service: State<'_, ContentService>,
) -> Result<Vec<InstalledContent>, LauncherError> {
    let mut items = service.storage.list_installed_content(&build_id).await?;
    let missing: Vec<(usize, String)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.icon_url.is_none() && item.version_id != "local")
        .map(|(index, item)| (index, item.project_id.clone()))
        .collect();
    let client = service.inner().clone();
    let details: Vec<_> = stream::iter(missing.into_iter().map(|(index, project_id)| {
        let client = client.clone();
        async move {
            let result = client
                .json::<ProjectDetails>(&format!("{MODRINTH_API}/project/{project_id}"))
                .await
                .ok();
            (index, result)
        }
    }))
    .buffer_unordered(8)
    .collect()
    .await;
    for (index, details) in details {
        if let Some(details) = details {
            items[index].icon_url = details.icon_url;
            if !details.title.trim().is_empty() {
                items[index].title = details.title;
            }
            service
                .storage
                .upsert_installed_content(&items[index])
                .await?;
        }
    }
    Ok(items)
}
#[tauri::command(rename_all = "camelCase")]
pub async fn remove_installed_content(
    build_id: String,
    project_id: String,
    storage: State<'_, Storage>,
) -> Result<(), LauncherError> {
    let build = storage
        .list_builds()
        .await?
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
    if let Some(item) = storage
        .remove_installed_content(&build.id, &project_id)
        .await?
    {
        let folder = match item.project_type.as_str() {
            "mod" => "mods",
            "resourcepack" => "resourcepacks",
            "shader" => "shaderpacks",
            _ => "",
        };
        if !folder.is_empty() {
            let target = safe_child(&PathBuf::from(build.game_dir).join(folder), &item.filename)?;
            let _ = fs::remove_file(target);
        }
    }
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_installed_content_enabled(
    build_id: String,
    project_id: String,
    enabled: bool,
    storage: State<'_, Storage>,
) -> Result<InstalledContent, LauncherError> {
    let build = storage
        .list_builds()
        .await?
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
    let mut item = storage
        .list_installed_content(&build.id)
        .await?
        .into_iter()
        .find(|item| item.project_id == project_id)
        .ok_or_else(|| input_error("content_not_found", "Элемент сборки не найден."))?;
    if item.enabled == enabled {
        return Ok(item);
    }
    let folder = match item.project_type.as_str() {
        "mod" => "mods",
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        _ => {
            return Err(input_error(
                "content_toggle_unsupported",
                "Этот элемент нельзя отключить отдельно.",
            ))
        }
    };
    let root = PathBuf::from(build.game_dir).join(folder);
    let normal = safe_child(&root, &item.filename)?;
    let disabled = safe_child(&root, &format!("{}.disabled", item.filename))?;
    let (source, destination) = if enabled {
        (&disabled, &normal)
    } else {
        (&normal, &disabled)
    };
    fs::rename(source, destination).map_err(|_| {
        LauncherError::new(
            "content_toggle_failed",
            "Не удалось изменить состояние файла. Возможно, он был перемещён вручную.",
            None,
            true,
        )
    })?;
    item.enabled = enabled;
    service_upsert_installed(&storage, &item).await?;
    Ok(item)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn import_local_content(
    build_id: String,
    project_type: String,
    storage: State<'_, Storage>,
) -> Result<Vec<InstalledContent>, LauncherError> {
    let build = storage
        .list_builds()
        .await?
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
    let (folder, extension) = match project_type.as_str() {
        "mod" => ("mods", "jar"),
        "resourcepack" => ("resourcepacks", "zip"),
        "shader" => ("shaderpacks", "zip"),
        _ => {
            return Err(input_error(
                "local_content_unsupported",
                "Этот тип локального контента не поддерживается.",
            ))
        }
    };
    let sources = choose_content_files(extension)?;
    let target_dir = PathBuf::from(&build.game_dir).join(folder);
    fs::create_dir_all(&target_dir).map_err(|_| LauncherError::storage_unavailable())?;
    let mut imported = Vec::new();
    for source in sources {
        let filename = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                input_error(
                    "local_content_invalid",
                    "Имя выбранного файла не поддерживается.",
                )
            })?;
        if source
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some(extension)
        {
            return Err(input_error(
                "local_content_invalid",
                "Выбран файл неподдерживаемого формата.",
            ));
        }
        let target = safe_child(&target_dir, filename)?;
        fs::copy(&source, &target).map_err(|_| LauncherError::storage_unavailable())?;
        let project_id = format!(
            "local-{:x}",
            Sha256::digest(format!("{}:{}", build.id, filename).as_bytes())
        );
        let item = InstalledContent {
            id: format!("{}:{}", build.id, project_id),
            build_id: build.id.clone(),
            project_id,
            version_id: "local".to_owned(),
            project_type: project_type.clone(),
            title: source
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(filename)
                .to_owned(),
            filename: filename.to_owned(),
            icon_url: None,
            enabled: true,
        };
        storage.upsert_installed_content(&item).await?;
        imported.push(item);
    }
    Ok(imported)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn open_build_folder(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<(), LauncherError> {
    let build = storage
        .list_builds()
        .await?
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
    std::process::Command::new("explorer.exe")
        .arg(&build.game_dir)
        .spawn()
        .map_err(|_| input_error("folder_open_failed", "Не удалось открыть папку сборки."))?;
    Ok(())
}

fn find_build(
    storage_builds: Vec<BuildSummary>,
    build_id: &str,
) -> Result<BuildSummary, LauncherError> {
    storage_builds
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))
}

fn metadata_time(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let Ok(metadata) = entry.metadata() else {
                return 0;
            };
            if metadata.is_dir() {
                directory_size(&entry.path())
            } else {
                metadata.len()
            }
        })
        .sum()
}

fn relative_target(root: &Path, relative: &str) -> Result<PathBuf, LauncherError> {
    if relative.trim().is_empty() {
        Ok(root.to_path_buf())
    } else {
        safe_relative(root, relative)
    }
}

fn file_entry(root: &Path, path: &Path) -> Result<BuildFileEntry, LauncherError> {
    let metadata = path
        .metadata()
        .map_err(|_| LauncherError::storage_unavailable())?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| LauncherError::invalid_path())?;
    Ok(BuildFileEntry {
        name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Файл")
            .to_owned(),
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        kind: if metadata.is_dir() {
            "directory"
        } else {
            "file"
        }
        .to_owned(),
        size: if metadata.is_file() {
            metadata.len()
        } else {
            directory_size(path)
        },
        modified_at: metadata_time(&metadata),
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_build_files(
    build_id: String,
    relative_path: String,
    storage: State<'_, Storage>,
) -> Result<Vec<BuildFileEntry>, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let target = relative_target(&root, &relative_path)?;
    if !target.is_dir() {
        return Err(input_error("folder_not_found", "Папка сборки не найдена."));
    }
    let mut result = fs::read_dir(&target)
        .map_err(|_| LauncherError::storage_unavailable())?
        .flatten()
        .filter_map(|entry| file_entry(&root, &entry.path()).ok())
        .collect::<Vec<_>>();
    result.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_build_worlds(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<Vec<BuildWorldSummary>, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let saves = PathBuf::from(build.game_dir).join("saves");
    if !saves.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(&saves)
        .map_err(|_| LauncherError::storage_unavailable())?
        .flatten()
    {
        let metadata = match entry.metadata() {
            Ok(value) if value.is_dir() => value,
            _ => continue,
        };
        result.push(BuildWorldSummary {
            name: entry.file_name().to_string_lossy().into_owned(),
            relative_path: format!("saves/{}", entry.file_name().to_string_lossy()),
            size: directory_size(&entry.path()),
            modified_at: metadata_time(&metadata),
        });
    }
    result.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_build_logs(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<Vec<BuildLogSummary>, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let logs = root.join("logs");
    if !logs.exists() {
        return Ok(Vec::new());
    }
    let mut result = fs::read_dir(logs)
        .map_err(|_| LauncherError::storage_unavailable())?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_file() {
                file_entry(&root, &path).ok()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn read_build_log(
    build_id: String,
    relative_path: String,
    storage: State<'_, Storage>,
) -> Result<String, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let target = safe_relative(&root, &relative_path)?;
    if target.parent() != Some(&root.join("logs")) || !target.is_file() {
        return Err(LauncherError::invalid_path());
    }
    let bytes = fs::read(target).map_err(|_| LauncherError::storage_unavailable())?;
    let start = bytes.len().saturating_sub(2 * 1024 * 1024);
    Ok(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn open_build_path(
    build_id: String,
    relative_path: String,
    storage: State<'_, Storage>,
) -> Result<(), LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let target = relative_target(&root, &relative_path)?;
    if !target.exists() {
        return Err(input_error("path_not_found", "Файл или папка не найдены."));
    }
    std::process::Command::new("explorer.exe")
        .arg(target)
        .spawn()
        .map_err(|_| input_error("folder_open_failed", "Не удалось открыть файл или папку."))?;
    Ok(())
}

#[cfg(windows)]
fn choose_content_files(extension: &str) -> Result<Vec<PathBuf>, LauncherError> {
    use std::{mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };
    let mut file = vec![0_u16; 65_536];
    let label = if extension == "jar" {
        "Моды Minecraft (*.jar)\0*.jar\0\0"
    } else {
        "Архивы Minecraft (*.zip)\0*.zip\0\0"
    };
    let filter: Vec<u16> = label.encode_utf16().collect();
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.Flags = OFN_ALLOWMULTISELECT | OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return Ok(Vec::new());
    }
    let values: Vec<_> = file
        .split(|unit| *unit == 0)
        .take_while(|part| !part.is_empty())
        .map(|part| PathBuf::from(std::ffi::OsString::from_wide(part)))
        .collect();
    if values.len() <= 1 {
        return Ok(values);
    }
    Ok(values[1..]
        .iter()
        .map(|name| values[0].join(name))
        .collect())
}

#[cfg(windows)]
fn choose_mrpack_file() -> Result<Option<PathBuf>, LauncherError> {
    use std::{mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut file = vec![0_u16; 32_768];
    let filter: Vec<u16> = "Сборки Modrinth (*.mrpack)\0*.mrpack\0\0"
        .encode_utf16()
        .collect();
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return Ok(None);
    }
    let length = file
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..length],
    ))))
}

#[cfg(not(windows))]
fn choose_mrpack_file() -> Result<Option<PathBuf>, LauncherError> {
    Err(input_error(
        "mrpack_picker_unavailable",
        "Выбор .mrpack доступен только в Windows.",
    ))
}

#[cfg(not(windows))]
fn choose_content_files(_extension: &str) -> Result<Vec<PathBuf>, LauncherError> {
    Err(input_error(
        "content_picker_unavailable",
        "Локальный импорт доступен только в Windows.",
    ))
}

async fn service_upsert_installed(
    storage: &Storage,
    item: &InstalledContent,
) -> Result<(), LauncherError> {
    storage.upsert_installed_content(item).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_offline_skins(
    account_id: String,
    service: State<'_, ContentService>,
) -> Result<Vec<OfflineSkinView>, LauncherError> {
    service
        .storage
        .list_offline_skins(&account_id)
        .await?
        .into_iter()
        .map(|skin| service.skin_view(skin))
        .collect()
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_offline_skin(
    account_id: String,
    service: State<'_, ContentService>,
) -> Result<Option<OfflineSkinView>, LauncherError> {
    let Some(source) = choose_png_file()? else {
        return Ok(None);
    };
    let bytes = fs::read(&source).map_err(|_| LauncherError::storage_unavailable())?;
    validate_skin_png(&bytes)?;
    let account_key = format!("{:x}", Sha256::digest(account_id.as_bytes()));
    let directory = service.paths.root.join("skins").join(account_key);
    fs::create_dir_all(&directory).map_err(|_| LauncherError::storage_unavailable())?;
    let id = format!(
        "skin-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let target = directory.join(format!("{id}.png"));
    fs::write(&target, &bytes).map_err(|_| LauncherError::storage_unavailable())?;
    let skin = OfflineSkin {
        id,
        account_id,
        name: source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Скин")
            .to_owned(),
        file_path: target.to_string_lossy().into_owned(),
        is_active: true,
    };
    service.storage.add_offline_skin(&skin).await?;
    service.skin_view(skin).map(Some)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_offline_skin(
    account_id: String,
    skin_id: String,
    service: State<'_, ContentService>,
) -> Result<(), LauncherError> {
    let skin = service
        .storage
        .list_offline_skins(&account_id)
        .await?
        .into_iter()
        .find(|skin| skin.id == skin_id)
        .ok_or_else(|| input_error("skin_not_found", "Скин не найден в библиотеке."))?;
    let account_key = format!("{:x}", Sha256::digest(account_id.as_bytes()));
    let expected_path = service
        .paths
        .root
        .join("skins")
        .join(account_key)
        .join(format!("{skin_id}.png"));
    if PathBuf::from(&skin.file_path) != expected_path {
        return Err(LauncherError::storage_unavailable());
    }
    match fs::remove_file(&expected_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(LauncherError::storage_unavailable()),
    }
    service
        .storage
        .delete_offline_skin(&account_id, &skin_id)
        .await?;
    Ok(())
}

const MINECRAFT_PROFILE_ENDPOINT: &str = "https://api.minecraftservices.com/minecraft/profile";
const MINECRAFT_SKINS_ENDPOINT: &str = "https://api.minecraftservices.com/minecraft/profile/skins";
const MINECRAFT_CAPE_ENDPOINT: &str =
    "https://api.minecraftservices.com/minecraft/profile/capes/active";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftCosmetics {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<MinecraftSkin>,
    #[serde(default)]
    capes: Vec<MinecraftCape>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftSkin {
    id: String,
    state: String,
    url: String,
    variant: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftCape {
    id: String,
    state: String,
    url: String,
    alias: String,
}

async fn online_account_token(
    account_id: &str,
    auth: &crate::auth::AuthService,
) -> Result<String, LauncherError> {
    if account_id.starts_with("offline:") {
        return Err(input_error(
            "minecraft_account_required",
            "Для изменения официального скина войдите через Microsoft.",
        ));
    }
    let refreshed = auth.refresh_active_minecraft_account().await?;
    let (account, token) = refreshed.into_parts();
    if account.id != account_id {
        return Err(input_error(
            "account_not_active",
            "Выберите этот Minecraft-аккаунт в меню профиля.",
        ));
    }
    Ok(token)
}

async fn cosmetics_response(
    response: reqwest::Response,
) -> Result<MinecraftCosmetics, LauncherError> {
    if !response.status().is_success() {
        return Err(LauncherError::new(
            "minecraft_cosmetics_failed",
            "Minecraft Services не удалось изменить внешний вид профиля.",
            Some(format!("HTTP {}", response.status().as_u16())),
            true,
        ));
    }
    response.json().await.map_err(|_| {
        LauncherError::new(
            "minecraft_cosmetics_invalid",
            "Minecraft Services вернул некорректные данные профиля.",
            None,
            true,
        )
    })
}

fn cosmetics_network_error() -> LauncherError {
    LauncherError::new(
        "minecraft_cosmetics_unavailable",
        "Minecraft Services сейчас недоступен. Попробуйте ещё раз.",
        None,
        true,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn minecraft_cosmetics(
    account_id: String,
    auth: State<'_, std::sync::Arc<crate::auth::AuthService>>,
) -> Result<MinecraftCosmetics, LauncherError> {
    let token = online_account_token(&account_id, auth.inner().as_ref()).await?;
    let response = reqwest::Client::new()
        .get(MINECRAFT_PROFILE_ENDPOINT)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| cosmetics_network_error())?;
    cosmetics_response(response).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn apply_minecraft_skin(
    account_id: String,
    skin_id: String,
    variant: String,
    service: State<'_, ContentService>,
    auth: State<'_, std::sync::Arc<crate::auth::AuthService>>,
) -> Result<MinecraftCosmetics, LauncherError> {
    let variant = match variant.to_ascii_lowercase().as_str() {
        "slim" => "slim",
        "classic" => "classic",
        _ => {
            return Err(input_error(
                "skin_variant_invalid",
                "Выберите модель Классическая или Тонкая.",
            ))
        }
    };
    let skin = service
        .storage
        .list_offline_skins(&account_id)
        .await?
        .into_iter()
        .find(|skin| skin.id == skin_id)
        .ok_or_else(|| input_error("skin_not_found", "Скин не найден в библиотеке."))?;
    let bytes = fs::read(&skin.file_path).map_err(|_| LauncherError::storage_unavailable())?;
    validate_skin_png(&bytes)?;
    let token = online_account_token(&account_id, auth.inner().as_ref()).await?;
    let boundary = format!(
        "----CKLauncher{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{variant}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = reqwest::Client::new()
        .post(MINECRAFT_SKINS_ENDPOINT)
        .bearer_auth(token)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .map_err(|_| cosmetics_network_error())?;
    let profile = cosmetics_response(response).await?;
    service
        .storage
        .select_offline_skin(&account_id, &skin_id)
        .await?;
    Ok(profile)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn activate_minecraft_cape(
    account_id: String,
    cape_id: Option<String>,
    auth: State<'_, std::sync::Arc<crate::auth::AuthService>>,
) -> Result<MinecraftCosmetics, LauncherError> {
    let token = online_account_token(&account_id, auth.inner().as_ref()).await?;
    let client = reqwest::Client::new();
    let request = if let Some(cape_id) = cape_id {
        client
            .put(MINECRAFT_CAPE_ENDPOINT)
            .bearer_auth(&token)
            .json(&serde_json::json!({ "capeId": cape_id }))
    } else {
        client.delete(MINECRAFT_CAPE_ENDPOINT).bearer_auth(&token)
    };
    let response = request
        .send()
        .await
        .map_err(|_| cosmetics_network_error())?;
    if response.status().is_success() && response.status() == reqwest::StatusCode::NO_CONTENT {
        let refreshed = client
            .get(MINECRAFT_PROFILE_ENDPOINT)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| cosmetics_network_error())?;
        cosmetics_response(refreshed).await
    } else {
        cosmetics_response(response).await
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn select_offline_skin(
    account_id: String,
    skin_id: String,
    service: State<'_, ContentService>,
) -> Result<OfflineSkinView, LauncherError> {
    service
        .storage
        .select_offline_skin(&account_id, &skin_id)
        .await?;
    let skin = service
        .storage
        .list_offline_skins(&account_id)
        .await?
        .into_iter()
        .find(|skin| skin.id == skin_id)
        .ok_or_else(|| input_error("skin_not_found", "Скин не найден."))?;
    service.skin_view(skin)
}

fn validate_skin_png(bytes: &[u8]) -> Result<(), LauncherError> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(input_error("skin_invalid", "Выберите PNG-скин Minecraft."));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width != 64 || ![32, 64].contains(&height) || bytes.len() > 2_000_000 {
        return Err(input_error(
            "skin_invalid",
            "Поддерживаются PNG-скины 64×64 или 64×32.",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn choose_png_file() -> Result<Option<PathBuf>, LauncherError> {
    use std::{mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut file = vec![0_u16; 32_768];
    let filter: Vec<u16> = "PNG Minecraft (*.png)\0*.png\0\0".encode_utf16().collect();
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return Ok(None);
    }
    let length = file
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..length],
    ))))
}

#[cfg(not(windows))]
fn choose_png_file() -> Result<Option<PathBuf>, LauncherError> {
    Err(input_error(
        "skin_picker_unavailable",
        "Выбор скина доступен только в Windows.",
    ))
}

#[cfg(windows)]
fn choose_build_icon_file() -> Result<Option<PathBuf>, LauncherError> {
    use std::{mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut file = vec![0_u16; 32_768];
    let filter: Vec<u16> = "Изображения (*.png;*.jpg;*.jpeg;*.webp)\0*.png;*.jpg;*.jpeg;*.webp\0\0"
        .encode_utf16()
        .collect();
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return Ok(None);
    }
    let length = file
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..length],
    ))))
}

#[cfg(not(windows))]
fn choose_build_icon_file() -> Result<Option<PathBuf>, LauncherError> {
    Err(input_error(
        "build_icon_picker_unavailable",
        "Выбор иконки доступен только в Windows.",
    ))
}
