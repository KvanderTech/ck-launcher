mod security;
use crate::downloads::DownloadCancellationToken;
use crate::{
    error::LauncherError,
    metadata::{models::VersionJson, resolver::MetadataService},
    paths::AppPaths,
    storage::{BuildSummary, InstalledContent, OfflineSkin, Storage},
};
use base64::Engine;
use reqwest::{header, Client};
use security::{inspect_archive, FileTransaction, MAX_ARCHIVE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

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
    cancellation: Arc<Mutex<DownloadCancellationToken>>,
    previews: Arc<
        Mutex<std::collections::HashMap<String, (tempfile::NamedTempFile, std::time::Instant)>>,
    >,
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
    project_id: String,
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
    size: u64,
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
    #[serde(rename = "versionId")]
    _version_id: String,
    name: String,
    #[serde(rename = "summary")]
    _summary: Option<String>,
    files: Vec<MrpackFile>,
    dependencies: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MrpackFile {
    path: String,
    hashes: FileHashes,
    downloads: Vec<String>,
    env: Option<MrpackEnvironment>,
    #[serde(rename = "fileSize")]
    file_size: u64,
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
    is_favorite: bool,
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
            .redirect(security::redirect_policy())
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
        fs::create_dir_all(paths.safe_join(&paths.root, Path::new("instances"))?)
            .map_err(|_| LauncherError::storage_unavailable())?;
        fs::create_dir_all(paths.safe_join(&paths.root, Path::new("skins"))?)
            .map_err(|_| LauncherError::storage_unavailable())?;
        Ok(Self {
            client,
            paths,
            storage,
            metadata,
            cancellation: Arc::new(Mutex::new(DownloadCancellationToken::new())),
            previews: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, LauncherError> {
        Self::response_json(
            self.client
                .get(url)
                .send()
                .await
                .map_err(|_| network_error())?,
        )
        .await
    }

    async fn response_json<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<T, LauncherError> {
        const MAX_JSON: usize = 4 * 1024 * 1024;
        let mut response = response.error_for_status().map_err(|_| network_error())?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_JSON as u64)
        {
            return Err(security::limit_error());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| network_error())? {
            if bytes.len().saturating_add(chunk.len()) > MAX_JSON {
                return Err(security::limit_error());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| network_error())
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
        let response = self
            .client
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
            .map_err(|_| network_error())?;
        Self::response_json(response).await
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
        let game_dir = self
            .paths
            .safe_join(&self.paths.root, Path::new("instances"))?
            .join(&id);
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
            game_dir: crate::paths::strip_verbatim_prefix(game_dir.clone())
                .to_string_lossy()
                .into_owned(),
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
        if visiting.len() >= 64 {
            return Err(input_error(
                "dependency_limit",
                "Слишком глубокое дерево зависимостей.",
            ));
        }
        if !visiting.insert(project_id.clone()) {
            return Err(input_error(
                "dependency_cycle",
                "Modrinth вернул циклическую зависимость. Установка остановлена безопасно.",
            ));
        }
        let build = self
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
        if project.project_type == "mod" {
            if build.loader == "vanilla"
                || !version.loaders.iter().any(|loader| loader == &build.loader)
            {
                return Err(input_error("loader_not_supported", "Этот мод не совместим с загрузчиком сборки. Выберите сборку Fabric или Quilt и подходящую версию мода."));
            }
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
            let dependency_project = match (&dependency.project_id, &dependency.version_id) {
                (Some(project), _) => project.clone(),
                (None, Some(version)) => {
                    self.json::<ProjectVersion>(&format!("{MODRINTH_API}/version/{version}"))
                        .await?
                        .project_id
                }
                _ => {
                    return Err(input_error(
                        "dependency_invalid",
                        "Зависимость не содержит идентификатора.",
                    ))
                }
            };
            if installed.iter().any(|item| {
                item.project_id == dependency_project
                    && item.enabled
                    && dependency
                        .version_id
                        .as_ref()
                        .is_none_or(|v| v == &item.version_id)
            }) {
                continue;
            }
            Box::pin(self.install_project_inner(
                dependency_project,
                build.id.clone(),
                dependency.version_id.clone(),
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
        let game_dir = self
            .paths
            .safe_join(&self.paths.root, Path::new("instances"))?
            .join(&id);
        fs::create_dir_all(&game_dir).map_err(|_| LauncherError::storage_unavailable())?;
        let build = BuildSummary {
            id: id.clone(),
            name: project.title.clone(),
            game_version: "pending".to_owned(),
            loader: "vanilla".to_owned(),
            loader_version: None,
            game_dir: crate::paths::strip_verbatim_prefix(game_dir.clone())
                .to_string_lossy()
                .into_owned(),
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
            if let Some(source) = self.storage.pack_source(&build.id).await? {
                let lock: PackSource =
                    serde_json::from_str(&source).map_err(|_| security::invalid_archive())?;
                if lock.sha256.len() != 64 || !lock.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(security::invalid_archive());
                }
                let cache = self.cache_directory()?;
                let path = self
                    .paths
                    .safe_join(&cache, Path::new(&format!("{}.mrpack", lock.sha256)))?;
                let mut archive = tempfile::NamedTempFile::new_in(&cache)
                    .map_err(|_| LauncherError::storage_unavailable())?;
                let source = fs::File::open(path).map_err(|_| {
                    input_error(
                        "pack_source_missing",
                        "Исходный архив отсутствует. Импортируйте доверенную сборку заново.",
                    )
                })?;
                if std::io::copy(&mut source.take(MAX_ARCHIVE + 1), &mut archive)
                    .map_err(|_| security::invalid_archive())?
                    > MAX_ARCHIVE
                    || hash_archive(archive.as_file_mut())? != lock.sha256
                {
                    return Err(security::invalid_archive());
                }
                self.install_mrpack_archive(
                    lock.project,
                    lock.version_id,
                    lock.filename,
                    archive,
                    build.clone(),
                    true,
                )
                .await?;
            } else if modpack.version_id == "local" || modpack.project_id.starts_with("local-") {
                return Err(input_error("pack_source_missing", "Сборка создана старой версией лаунчера без сохранения исходного архива. Импортируйте её заново для восстановления."));
            } else {
                let project: ProjectDetails = self
                    .json(&format!("{MODRINTH_API}/project/{}", modpack.project_id))
                    .await?;
                let version: ProjectVersion = self
                    .json(&format!("{MODRINTH_API}/version/{}", modpack.version_id))
                    .await?;
                self.install_mrpack(project, version, build.clone()).await?;
            }
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
        let root = Path::new(&build.game_dir);
        let filename = security::relative_path(&file.filename)?;
        if filename.components().count() != 1 {
            return Err(LauncherError::invalid_path());
        }
        let previous = self.storage.list_installed_content(&build.id).await?;
        if previous.iter().any(|p| {
            p.project_id != project.id
                && p.project_type == project.project_type
                && p.filename.eq_ignore_ascii_case(&file.filename)
        }) {
            return Err(input_error(
                "content_file_conflict",
                "Этот файл принадлежит другому установленному проекту.",
            ));
        }
        let old = previous.iter().find(|p| p.project_id == project.id);
        let enabled = old.is_none_or(|p| p.enabled);
        let destination = PathBuf::from(folder).join(if enabled {
            file.filename.clone()
        } else {
            format!("{}.disabled", file.filename)
        });
        security::safe_destination(root, &destination)?;
        let staged = security::download(
            &self.client,
            root,
            &file.url,
            &file.hashes,
            Some(file.size),
            &self.token(),
        )
        .await?;
        let item = InstalledContent {
            id: format!("{}:{}", build.id, project.id),
            build_id: build.id.clone(),
            project_id: project.id,
            version_id: version.id,
            project_type: project.project_type,
            title: project.title,
            filename: file.filename,
            icon_url: project.icon_url,
            enabled,
        };
        let mut tx = FileTransaction::new(root)?;
        tx.replace(&destination, staged.path())?;
        if let Some(old) = old {
            if !old.filename.eq_ignore_ascii_case(&item.filename) {
                tx.remove(&PathBuf::from(folder).join(if old.enabled {
                    old.filename.clone()
                } else {
                    format!("{}.disabled", old.filename)
                }))?;
            }
        }
        self.storage.upsert_installed_content(&item).await?;
        tx.commit();
        Ok(item)
    }

    fn token(&self) -> DownloadCancellationToken {
        self.cancellation
            .lock()
            .expect("content cancellation")
            .clone()
    }
    pub fn cancel(&self) {
        self.token().cancel();
    }
    pub fn reset_cancellation(&self) {
        *self.cancellation.lock().expect("content cancellation") = DownloadCancellationToken::new();
    }
    fn cache_directory(&self) -> Result<PathBuf, LauncherError> {
        let path = self
            .paths
            .safe_join(&self.paths.root, Path::new("pack-cache"))?;
        fs::create_dir_all(&path).map_err(|_| LauncherError::storage_unavailable())?;
        self.paths
            .safe_join(&self.paths.root, Path::new("pack-cache"))
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
            .find(|f| f.primary)
            .or_else(|| version.files.first())
            .ok_or_else(network_error)?;
        let archive = security::download(
            &self.client,
            &self.cache_directory()?,
            &file.url,
            &file.hashes,
            Some(file.size),
            &self.token(),
        )
        .await?;
        self.install_mrpack_archive(
            project,
            version.id,
            file.filename.clone(),
            archive,
            build,
            false,
        )
        .await
    }
    async fn install_mrpack_archive(
        &self,
        project: ProjectDetails,
        pack_version_id: String,
        pack_filename: String,
        mut source: tempfile::NamedTempFile,
        mut build: BuildSummary,
        repairing: bool,
    ) -> Result<InstalledContent, LauncherError> {
        let fingerprint = hash_archive(source.as_file_mut())?;
        source
            .seek(SeekFrom::Start(0))
            .map_err(|_| LauncherError::storage_unavailable())?;
        let mut archive = zip::ZipArchive::new(
            source
                .reopen()
                .map_err(|_| LauncherError::storage_unavailable())?,
        )
        .map_err(|_| security::invalid_archive())?;
        let index = inspect_archive(&mut archive)?;
        let minecraft = &index.dependencies["minecraft"];
        let token = self.token();
        if token.is_cancelled() {
            return Err(security::cancelled());
        }
        let (loader, loader_version) =
            if let Some(version) = index.dependencies.get("fabric-loader") {
                (
                    "fabric",
                    Some(
                        self.install_fabric_profile(minecraft, Some(version))
                            .await?,
                    ),
                )
            } else if let Some(version) = index.dependencies.get("quilt-loader") {
                (
                    "quilt",
                    Some(self.install_quilt_profile(minecraft, Some(version)).await?),
                )
            } else {
                ("vanilla", None)
            };
        if !repairing {
            build.name = index.name.clone();
        }
        build.game_version = loader_version
            .as_ref()
            .map(|v| format!("{loader}-loader-{v}-{minecraft}"))
            .unwrap_or_else(|| minecraft.clone());
        build.loader = loader.to_owned();
        build.loader_version = loader_version;
        build.icon_url = project.icon_url.clone();
        let root = Path::new(&build.game_dir);
        let existing = self.storage.list_installed_content(&build.id).await?;
        let staging = tempfile::Builder::new()
            .prefix(".ck-staging-")
            .tempdir_in(root)
            .map_err(|_| LauncherError::storage_unavailable())?;
        let mut staged = Vec::new();
        let mut records = Vec::new();
        for entry in &index.files {
            if token.is_cancelled() {
                return Err(security::cancelled());
            }
            if entry.env.as_ref().and_then(|env| env.client.as_deref()) == Some("unsupported") {
                continue;
            }
            let relative = security::relative_path(&entry.path)?;
            let filename = relative
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(LauncherError::invalid_path)?
                .to_owned();
            let project_type = match relative
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
            {
                Some("mods") => "mod",
                Some("resourcepacks") => "resourcepack",
                Some("shaderpacks") => "shader",
                _ => "file",
            };
            let (project_id, version_id) = modrinth_ids_from_download(&entry.downloads[0])
                .unwrap_or_else(|| {
                    (
                        format!("pack-file-{:x}", Sha256::digest(entry.path.as_bytes())),
                        pack_version_id.clone(),
                    )
                });
            let previous = existing.iter().find(|i| {
                i.project_id == project_id
                    || (i.filename == filename && i.project_type == project_type)
            });
            let enabled = previous.is_none_or(|p| p.enabled);
            let destination = if enabled {
                relative.clone()
            } else {
                relative.with_file_name(format!("{filename}.disabled"))
            };
            security::safe_destination(root, &destination)?;
            let mut last_error = None;
            let mut downloaded = None;
            for url in &entry.downloads {
                match security::download(
                    &self.client,
                    staging.path(),
                    url,
                    &entry.hashes,
                    Some(entry.file_size),
                    &token,
                )
                .await
                {
                    Ok(file) => {
                        downloaded = Some(file);
                        break;
                    }
                    Err(e) if e.code() == "operation_cancelled" => return Err(e),
                    Err(e) => last_error = Some(e),
                }
            }
            let file = downloaded.ok_or_else(|| last_error.unwrap_or_else(network_error))?;
            staged.push((destination, file));
            if project_type != "file" {
                records.push(InstalledContent {
                    id: format!("{}:{project_id}", build.id),
                    build_id: build.id.clone(),
                    project_id,
                    version_id,
                    project_type: project_type.to_owned(),
                    title: previous
                        .map(|p| p.title.clone())
                        .unwrap_or_else(|| filename.clone()),
                    filename,
                    icon_url: previous.and_then(|p| p.icon_url.clone()),
                    enabled,
                });
            }
        }
        let mut override_paths = std::collections::BTreeMap::new();
        for prefix in ["overrides/", "client-overrides/"] {
            for i in 0..archive.len() {
                let entry = archive
                    .by_index(i)
                    .map_err(|_| security::invalid_archive())?;
                let name = entry.name().replace('\\', "/");
                if let Some(relative) = name
                    .strip_prefix(prefix)
                    .filter(|r| !r.is_empty() && !entry.is_dir())
                {
                    override_paths.insert(relative.to_lowercase(), (relative.to_owned(), i));
                }
            }
        }
        for (_, (relative, index)) in override_paths {
            if token.is_cancelled() {
                return Err(security::cancelled());
            }
            let relative = security::relative_path(&relative)?;
            let destination = security::safe_destination(root, &relative)?;
            // A repair must preserve user-edited configs, worlds, and disabled files.
            if repairing
                && (destination.exists()
                    || destination
                        .with_file_name(format!(
                            "{}.disabled",
                            destination.file_name().unwrap().to_string_lossy()
                        ))
                        .exists())
            {
                continue;
            }
            let entry = archive
                .by_index(index)
                .map_err(|_| security::invalid_archive())?;
            let expected = entry.size();
            let mut output = tempfile::NamedTempFile::new_in(staging.path())
                .map_err(|_| LauncherError::storage_unavailable())?;
            let copied = std::io::copy(&mut entry.take(expected + 1), &mut output)
                .map_err(|_| security::invalid_archive())?;
            if copied != expected {
                return Err(security::invalid_archive());
            }
            output
                .as_file()
                .sync_all()
                .map_err(|_| LauncherError::storage_unavailable())?;
            staged.push((relative, output));
        }
        let cache = self.cache_directory()?;
        let cached = self
            .paths
            .safe_join(&cache, Path::new(&format!("{fingerprint}.mrpack")))?;
        if !cached.exists() {
            source
                .persist_noclobber(&cached)
                .map_err(|_| LauncherError::storage_unavailable())?;
        }
        let source_json = serde_json::json!({"sha256": fingerprint, "project": project, "versionId": pack_version_id, "filename": pack_filename}).to_string();
        let item = InstalledContent {
            id: format!("{}:{}", build.id, project.id),
            build_id: build.id.clone(),
            project_id: project.id,
            version_id: pack_version_id,
            project_type: "modpack".to_owned(),
            title: index.name,
            filename: pack_filename,
            icon_url: project.icon_url,
            enabled: true,
        };
        records.push(item.clone());
        let mut tx = FileTransaction::new(root)?;
        for (relative, file) in staged {
            if token.is_cancelled() {
                return Err(security::cancelled());
            }
            tx.replace(&relative, file.path())?;
        }
        self.storage
            .commit_pack(&build, &records, &source_json)
            .await?;
        tx.commit();
        Ok(item)
    }
    pub fn preview_mrpack(&self, source: PathBuf) -> Result<MrpackPreview, LauncherError> {
        if source
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("mrpack"))
        {
            return Err(security::invalid_archive());
        }
        let original = fs::File::open(&source).map_err(|_| security::invalid_archive())?;
        if original
            .metadata()
            .map_err(|_| security::invalid_archive())?
            .len()
            > MAX_ARCHIVE
        {
            return Err(security::limit_error());
        }
        let mut snapshot = tempfile::NamedTempFile::new_in(self.cache_directory()?)
            .map_err(|_| LauncherError::storage_unavailable())?;
        if std::io::copy(&mut original.take(MAX_ARCHIVE + 1), &mut snapshot)
            .map_err(|_| security::invalid_archive())?
            > MAX_ARCHIVE
        {
            return Err(security::limit_error());
        }
        let fingerprint = hash_archive(snapshot.as_file_mut())?;
        snapshot
            .seek(SeekFrom::Start(0))
            .map_err(|_| security::invalid_archive())?;
        let mut archive =
            zip::ZipArchive::new(snapshot.reopen().map_err(|_| security::invalid_archive())?)
                .map_err(|_| security::invalid_archive())?;
        let index = inspect_archive(&mut archive)?;
        let hosts = index
            .files
            .iter()
            .flat_map(|f| f.downloads.iter())
            .filter_map(|u| {
                url::Url::parse(u)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let overrides = (0..archive.len())
            .filter(|i| {
                archive.by_index(*i).is_ok_and(|e| {
                    !e.is_dir()
                        && (e.name().starts_with("overrides/")
                            || e.name().starts_with("client-overrides/"))
                })
            })
            .count();
        let preview = MrpackPreview { sha256: fingerprint.clone(), name: index.name, minecraft: index.dependencies["minecraft"].clone(), loader: if index.dependencies.contains_key("fabric-loader") { "Fabric" } else if index.dependencies.contains_key("quilt-loader") { "Quilt" } else { "Vanilla" }.to_owned(), download_files: index.files.len(), total_download_bytes: index.files.iter().map(|f| f.file_size).sum(), overrides, hosts, warning: "Моды исполняют код с правами вашей учётной записи Windows. Контрольная сумма подтверждает целостность, но не безопасность. Продолжайте, только если доверяете автору сборки.".to_owned() };
        let mut pending = self
            .previews
            .lock()
            .map_err(|_| LauncherError::storage_unavailable())?;
        pending.retain(|_, (_, time)| time.elapsed() < Duration::from_secs(600));
        if pending.len() >= 4 {
            pending.clear();
        }
        pending.insert(fingerprint, (snapshot, std::time::Instant::now()));
        Ok(preview)
    }
    pub async fn confirm_mrpack(&self, sha256: String) -> Result<InstalledContent, LauncherError> {
        let (mut snapshot, time) = self
            .previews
            .lock()
            .map_err(|_| LauncherError::storage_unavailable())?
            .remove(&sha256)
            .ok_or_else(|| {
                input_error(
                    "preview_required",
                    "Сначала проверьте сборку и подтвердите установку.",
                )
            })?;
        if time.elapsed() > Duration::from_secs(600)
            || hash_archive(snapshot.as_file_mut())? != sha256
        {
            return Err(input_error(
                "preview_expired",
                "Предпросмотр устарел. Проверьте сборку снова.",
            ));
        }
        let id = format!("build-{:032x}", rand::random::<u128>());
        let instances = self
            .paths
            .safe_join(&self.paths.root, Path::new("instances"))?;
        let game_dir = self.paths.safe_join(&instances, Path::new(&id))?;
        fs::create_dir(&game_dir).map_err(|_| LauncherError::storage_unavailable())?;
        let project = ProjectDetails {
            id: format!("local-mrpack-{sha256}"),
            title: "Локальная сборка".to_owned(),
            project_type: "modpack".to_owned(),
            icon_url: None,
            description: String::new(),
            body: String::new(),
            downloads: 0,
            followers: 0,
            categories: vec![],
        };
        let build = BuildSummary {
            id: id.clone(),
            name: project.title.clone(),
            game_version: "pending".to_owned(),
            loader: "vanilla".to_owned(),
            loader_version: None,
            game_dir: crate::paths::strip_verbatim_prefix(game_dir.clone())
                .to_string_lossy()
                .into_owned(),
            icon_url: None,
            is_active: true,
        };
        let result = self
            .install_mrpack_archive(
                project,
                "local".to_owned(),
                "local.mrpack".to_owned(),
                snapshot,
                build,
                false,
            )
            .await;
        if result.is_err() {
            let _ = fs::remove_dir(&game_dir);
        }
        result
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
            is_favorite: skin.is_favorite,
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
    if security::relative_path(filename)?.components().count() != 1 {
        return Err(LauncherError::invalid_path());
    }
    safe_relative(root, filename)
}
fn safe_relative(root: &Path, relative: impl AsRef<Path>) -> Result<PathBuf, LauncherError> {
    security::safe_destination(root, relative.as_ref())
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

pub async fn search_modrinth(
    query: String,
    project_type: String,
    game_version: Option<String>,
    loader: Option<String>,
    category: Option<String>,
    environment: Option<String>,
    index: Option<String>,
    offset: u32,
    service: &ContentService,
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
pub async fn modrinth_project(
    project_id: String,
    service: &ContentService,
) -> Result<ProjectDetails, LauncherError> {
    service
        .json(&format!("{MODRINTH_API}/project/{project_id}"))
        .await
}
pub async fn modrinth_project_versions(
    project_id: String,
    service: &ContentService,
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
pub async fn create_build(
    name: String,
    game_version: String,
    loader: String,
    service: &ContentService,
) -> Result<BuildSummary, LauncherError> {
    service.create_build(name, game_version, loader).await
}
pub async fn list_builds(storage: &Storage) -> Result<Vec<BuildSummary>, LauncherError> {
    storage.list_builds().await
}

pub async fn repair_build(
    build_id: String,
    service: &ContentService,
) -> Result<BuildSummary, LauncherError> {
    service.repair_build(build_id).await
}
pub async fn select_build(
    build_id: String,
    storage: &Storage,
) -> Result<BuildSummary, LauncherError> {
    storage.select_build(&build_id).await
}
pub async fn rename_build(
    build_id: String,
    name: String,
    storage: &Storage,
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

pub async fn choose_build_icon(
    build_id: String,
    storage: &Storage,
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
pub async fn delete_build(build_id: String, service: &ContentService) -> Result<(), LauncherError> {
    let build = service
        .storage
        .list_builds()
        .await?
        .into_iter()
        .find(|item| item.id == build_id)
        .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
    let source = PathBuf::from(&build.game_dir);
    service.paths.validate_absolute_directory(&source)?;
    let trash = service
        .paths
        .safe_join(&service.paths.root, Path::new("trash"))?;
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
pub async fn install_modrinth_project(
    project_id: String,
    build_id: String,
    version_id: Option<String>,
    service: &ContentService,
) -> Result<InstalledContent, LauncherError> {
    service
        .install_project(project_id, build_id, version_id)
        .await
}

pub async fn install_modrinth_modpack(
    project_id: String,
    version_id: Option<String>,
    service: &ContentService,
) -> Result<InstalledContent, LauncherError> {
    service
        .install_modpack_as_build(project_id, version_id)
        .await
}
pub async fn import_mrpack(
    source_path: Option<String>,
    service: &ContentService,
) -> Result<Option<InstalledContent>, LauncherError> {
    let _ = (source_path, service);
    Err(input_error(
        "preview_required",
        "Сначала проверьте архив через предпросмотр и подтвердите установку.",
    ))
}
pub async fn preview_mrpack(
    source_path: Option<String>,
    service: &ContentService,
) -> Result<Option<MrpackPreview>, LauncherError> {
    let source = match source_path {
        Some(path) => Some(PathBuf::from(path)),
        None => choose_mrpack_file()?,
    };
    source.map(|path| service.preview_mrpack(path)).transpose()
}

pub fn pending_mrpack_path(pending: &PendingMrpackPath) -> Option<String> {
    pending.0.lock().ok()?.take()
}
pub async fn list_installed_content(
    build_id: String,
    service: &ContentService,
) -> Result<Vec<InstalledContent>, LauncherError> {
    // Reading the installed library must never wait for Modrinth or mutate its records.
    service.storage.list_installed_content(&build_id).await
}

pub async fn remove_installed_content(
    build_id: String,
    project_id: String,
    storage: &Storage,
) -> Result<(), LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let Some(item) = storage
        .list_installed_content(&build.id)
        .await?
        .into_iter()
        .find(|i| i.project_id == project_id)
    else {
        return Ok(());
    };
    let folder = content_folder(&item.project_type)?;
    let relative = PathBuf::from(folder).join(if item.enabled {
        item.filename
    } else {
        format!("{}.disabled", item.filename)
    });
    let mut tx = FileTransaction::new(Path::new(&build.game_dir))?;
    tx.remove(&relative)?;
    storage
        .remove_installed_content(&build.id, &project_id)
        .await?;
    tx.commit();
    Ok(())
}
fn content_folder(kind: &str) -> Result<&'static str, LauncherError> {
    match kind {
        "mod" => Ok("mods"),
        "resourcepack" => Ok("resourcepacks"),
        "shader" => Ok("shaderpacks"),
        _ => Err(input_error(
            "content_action_unsupported",
            "Для удаления модпака удалите сборку целиком.",
        )),
    }
}

pub async fn set_installed_content_enabled(
    build_id: String,
    project_id: String,
    enabled: bool,
    storage: &Storage,
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
    if destination.exists() {
        return Err(input_error(
            "content_file_conflict",
            "Целевой файл уже существует.",
        ));
    }
    fs::rename(source, destination).map_err(|_| {
        LauncherError::new(
            "content_toggle_failed",
            "Не удалось изменить состояние файла. Возможно, он был перемещён вручную.",
            None,
            true,
        )
    })?;
    item.enabled = enabled;
    if let Err(error) = service_upsert_installed(&storage, &item).await {
        fs::rename(destination, source).map_err(|_| {
            input_error(
                "content_rollback_failed",
                "Не удалось восстановить файл после ошибки сохранения.",
            )
        })?;
        return Err(error);
    }
    Ok(item)
}

pub async fn import_local_content(
    build_id: String,
    project_type: String,
    storage: &Storage,
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
        safe_child(&target_dir, filename)?;
        let input = fs::File::open(&source).map_err(|_| LauncherError::storage_unavailable())?;
        if input
            .metadata()
            .map_err(|_| LauncherError::storage_unavailable())?
            .len()
            > MAX_ARCHIVE
        {
            return Err(security::limit_error());
        }
        let mut staged = tempfile::NamedTempFile::new_in(&target_dir)
            .map_err(|_| LauncherError::storage_unavailable())?;
        if std::io::copy(&mut input.take(MAX_ARCHIVE + 1), &mut staged)
            .map_err(|_| LauncherError::storage_unavailable())?
            > MAX_ARCHIVE
        {
            return Err(security::limit_error());
        }
        let mut transaction = FileTransaction::new(&target_dir)?;
        transaction.replace(Path::new(filename), staged.path())?;
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
        transaction.commit();
        imported.push(item);
    }
    Ok(imported)
}

pub async fn open_build_folder(build_id: String, storage: &Storage) -> Result<(), LauncherError> {
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
    let mut pending = vec![(path.to_owned(), 0usize)];
    let mut count = 0usize;
    let mut size = 0u64;
    while let Some((path, depth)) = pending.pop() {
        if depth > 32 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            count += 1;
            if count > 20_000 {
                return size;
            }
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if crate::paths::is_reparse_point(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if metadata.is_file() {
                size = size.saturating_add(metadata.len());
            }
        }
    }
    size
}
fn relative_target(root: &Path, relative: &str) -> Result<PathBuf, LauncherError> {
    if relative.trim().is_empty() {
        AppPaths::new(root.to_owned()).validate_absolute_directory(root)?;
        Ok(root.to_path_buf())
    } else {
        safe_relative(root, relative)
    }
}

fn file_entry(root: &Path, path: &Path) -> Result<BuildFileEntry, LauncherError> {
    security::safe_destination(
        root,
        path.strip_prefix(root)
            .map_err(|_| LauncherError::invalid_path())?,
    )?;
    let metadata = path
        .symlink_metadata()
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
            0
        },
        modified_at: metadata_time(&metadata),
    })
}

pub async fn list_build_files(
    build_id: String,
    relative_path: String,
    storage: &Storage,
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
        .take(4097)
        .filter_map(|entry| file_entry(&root, &entry.path()).ok())
        .collect::<Vec<_>>();
    if result.len() > 4096 {
        return Err(input_error(
            "directory_limit",
            "В папке слишком много файлов для просмотра. Откройте её в Проводнике.",
        ));
    }
    result.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(result)
}

pub async fn list_build_worlds(
    build_id: String,
    storage: &Storage,
) -> Result<Vec<BuildWorldSummary>, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let saves = safe_child(Path::new(&build.game_dir), "saves")?;
    if !saves.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(&saves)
        .map_err(|_| LauncherError::storage_unavailable())?
        .flatten()
        .take(512)
    {
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(value) if value.is_dir() && !crate::paths::is_reparse_point(&value) => value,
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

pub async fn list_build_logs(
    build_id: String,
    storage: &Storage,
) -> Result<Vec<BuildLogSummary>, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let logs = safe_child(&root, "logs")?;
    if !logs.exists() {
        return Ok(Vec::new());
    }
    let mut result = fs::read_dir(logs)
        .map_err(|_| LauncherError::storage_unavailable())?
        .flatten()
        .take(1024)
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

pub async fn read_build_log(
    build_id: String,
    relative_path: String,
    storage: &Storage,
) -> Result<String, LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let target = safe_relative(&root, &relative_path)?;
    if target.parent() != Some(&root.join("logs")) || !target.is_file() {
        return Err(LauncherError::invalid_path());
    }
    crate::logs::read_tail(&target)
}

pub async fn open_build_path(
    build_id: String,
    relative_path: String,
    storage: &Storage,
) -> Result<(), LauncherError> {
    let build = find_build(storage.list_builds().await?, &build_id)?;
    let root = PathBuf::from(build.game_dir);
    let target = relative_target(&root, &relative_path)?;
    if !target.exists() {
        return Err(input_error("path_not_found", "Файл или папка не найдены."));
    }
    crate::platform::show_in_folder(&target)
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

pub async fn list_offline_skins(
    account_id: String,
    service: &ContentService,
) -> Result<Vec<OfflineSkinView>, LauncherError> {
    service
        .storage
        .list_offline_skins(&account_id)
        .await?
        .into_iter()
        .map(|skin| service.skin_view(skin))
        .collect()
}

pub async fn add_offline_skin(
    account_id: String,
    service: &ContentService,
) -> Result<Option<OfflineSkinView>, LauncherError> {
    let Some(source) = choose_png_file()? else {
        return Ok(None);
    };
    let bytes = fs::read(&source).map_err(|_| LauncherError::storage_unavailable())?;
    validate_skin_png(&bytes)?;
    let account_key = format!("{:x}", Sha256::digest(account_id.as_bytes()));
    let directory = service
        .paths
        .safe_join(&service.paths.root, Path::new("skins"))?
        .join(account_key);
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
        is_favorite: false,
    };
    service.storage.add_offline_skin(&skin).await?;
    service.skin_view(skin).map(Some)
}

pub async fn delete_offline_skin(
    account_id: String,
    skin_id: String,
    service: &ContentService,
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

pub async fn rename_offline_skin(
    account_id: String,
    skin_id: String,
    name: String,
    service: &ContentService,
) -> Result<OfflineSkinView, LauncherError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(input_error(
            "skin_name_invalid",
            "Название должно содержать от 1 до 60 символов.",
        ));
    }
    let skin = service
        .storage
        .update_offline_skin(&account_id, &skin_id, Some(name), None)
        .await?;
    service.skin_view(skin)
}

pub async fn set_offline_skin_favorite(
    account_id: String,
    skin_id: String,
    is_favorite: bool,
    service: &ContentService,
) -> Result<OfflineSkinView, LauncherError> {
    let skin = service
        .storage
        .update_offline_skin(&account_id, &skin_id, None, Some(is_favorite))
        .await?;
    service.skin_view(skin)
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

pub async fn minecraft_cosmetics(
    account_id: String,
    auth: &std::sync::Arc<crate::auth::AuthService>,
) -> Result<MinecraftCosmetics, LauncherError> {
    let token = online_account_token(&account_id, auth.as_ref()).await?;
    let response = reqwest::Client::new()
        .get(MINECRAFT_PROFILE_ENDPOINT)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| cosmetics_network_error())?;
    cosmetics_response(response).await
}

pub async fn apply_minecraft_skin(
    account_id: String,
    skin_id: String,
    variant: String,
    service: &ContentService,
    auth: &std::sync::Arc<crate::auth::AuthService>,
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
    let token = online_account_token(&account_id, auth.as_ref()).await?;
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

pub async fn activate_minecraft_cape(
    account_id: String,
    cape_id: Option<String>,
    auth: &std::sync::Arc<crate::auth::AuthService>,
) -> Result<MinecraftCosmetics, LauncherError> {
    let token = online_account_token(&account_id, auth.as_ref()).await?;
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

pub async fn select_offline_skin(
    account_id: String,
    skin_id: String,
    service: &ContentService,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackPreview {
    pub sha256: String,
    pub name: String,
    pub minecraft: String,
    pub loader: String,
    pub download_files: usize,
    pub total_download_bytes: u64,
    pub overrides: usize,
    pub hosts: Vec<String>,
    pub warning: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackSource {
    sha256: String,
    project: ProjectDetails,
    version_id: String,
    filename: String,
}
fn hash_archive(file: &mut fs::File) -> Result<String, LauncherError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| security::invalid_archive())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| security::invalid_archive())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_ARCHIVE {
            return Err(security::limit_error());
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
