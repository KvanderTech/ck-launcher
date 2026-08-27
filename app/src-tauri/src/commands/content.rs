use crate::{
    error::LauncherError,
    metadata::{models::VersionJson, resolver::MetadataService},
    paths::AppPaths,
    storage::{BuildSummary, InstalledContent, OfflineSkin, Storage},
};
use base64::Engine;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tauri::State;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

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

#[derive(Debug, Deserialize)]
struct ProjectDetails {
    id: String,
    title: String,
    project_type: String,
    icon_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectVersion {
    id: String,
    #[serde(default)]
    dependencies: Vec<ProjectDependency>,
    #[serde(default)]
    files: Vec<ProjectFile>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    game_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectDependency {
    project_id: Option<String>,
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
        offset: u32,
    ) -> Result<ModrinthSearchResult, LauncherError> {
        let mut facets = vec![vec![format!("project_type:{project_type}")]];
        if let Some(version) = game_version.filter(|value| !value.is_empty()) {
            facets.push(vec![format!("versions:{version}")]);
        }
        if let Some(loader) = loader.filter(|value| !value.is_empty() && value != "vanilla") {
            facets.push(vec![format!("categories:{loader}")]);
        }
        self.client
            .get(format!("{MODRINTH_API}/search"))
            .query(&[
                ("query", query),
                ("facets", serde_json::to_string(&facets).unwrap_or_default()),
                ("index", "relevance".to_owned()),
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
        if !["vanilla", "fabric"].contains(&loader.as_str()) {
            return Err(input_error(
                "loader_not_supported",
                "Сейчас поддерживаются Vanilla и Fabric.",
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
        let loader_version = if loader == "fabric" {
            Some(self.install_fabric_profile(&game_version, None).await?)
        } else {
            None
        };
        let version_id = loader_version
            .as_ref()
            .map(|value| format!("fabric-loader-{value}-{game_version}"))
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

    pub async fn install_project(
        &self,
        project_id: String,
        build_id: String,
    ) -> Result<InstalledContent, LauncherError> {
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
        let version = versions.into_iter().next().ok_or_else(|| {
            input_error(
                "compatible_version_not_found",
                "Совместимая версия проекта не найдена.",
            )
        })?;
        if project.project_type == "modpack" {
            return self.install_mrpack(project, version, build).await;
        }
        if project.project_type == "mod" && build.loader == "vanilla" {
            return Err(input_error(
                "mod_loader_required",
                "Для модов создайте сборку Fabric.",
            ));
        }
        self.install_regular(project, version, build).await
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
        self.download_verified(&file.url, &target, file.hashes.sha512.as_deref())
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
        mut build: BuildSummary,
    ) -> Result<InstalledContent, LauncherError> {
        let file = version
            .files
            .iter()
            .find(|file| file.primary)
            .or_else(|| version.files.first())
            .ok_or_else(network_error)?
            .clone();
        let bytes = self
            .download_bytes(&file.url, file.hashes.sha512.as_deref())
            .await?;
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
            } else {
                ("vanilla".to_owned(), None, minecraft.clone())
            };
        build.name = index.name.clone();
        build.game_version = version_id;
        build.loader = loader;
        build.loader_version = loader_version;
        build.icon_url = project.icon_url.clone();
        self.storage.upsert_build(&build).await?;
        for entry in index.files {
            if entry.env.as_ref().and_then(|env| env.client.as_deref()) == Some("unsupported") {
                continue;
            }
            let url = entry.downloads.first().ok_or_else(network_error)?;
            let target = safe_relative(Path::new(&build.game_dir), &entry.path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|_| LauncherError::storage_unavailable())?;
            }
            self.download_verified(url, &target, entry.hashes.sha512.as_deref())
                .await?;
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
            version_id: version.id,
            project_type: project.project_type,
            title: index.name,
            filename: file.filename,
            icon_url: project.icon_url,
            enabled: true,
        };
        self.storage.upsert_installed_content(&item).await?;
        self.storage.select_build(&build.id).await?;
        Ok(item)
    }

    async fn download_verified(
        &self,
        url: &str,
        target: &Path,
        sha512: Option<&str>,
    ) -> Result<(), LauncherError> {
        let bytes = self.download_bytes(url, sha512).await?;
        fs::write(target, bytes).map_err(|_| LauncherError::storage_unavailable())
    }

    async fn download_bytes(
        &self,
        url: &str,
        sha512: Option<&str>,
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

fn base_game_version(build: &BuildSummary) -> &str {
    if build.loader == "fabric" {
        build
            .game_version
            .rsplit('-')
            .next()
            .unwrap_or(&build.game_version)
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
    offset: u32,
    service: State<'_, ContentService>,
) -> Result<ModrinthSearchResult, LauncherError> {
    service
        .search(query, project_type, game_version, loader, offset)
        .await
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
pub async fn select_build(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<BuildSummary, LauncherError> {
    storage.select_build(&build_id).await
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
    service.storage.delete_build(&build.id).await?;
    let _ = fs::remove_dir_all(build.game_dir);
    Ok(())
}
#[tauri::command(rename_all = "camelCase")]
pub async fn install_modrinth_project(
    project_id: String,
    build_id: String,
    service: State<'_, ContentService>,
) -> Result<InstalledContent, LauncherError> {
    service.install_project(project_id, build_id).await
}
#[tauri::command(rename_all = "camelCase")]
pub async fn list_installed_content(
    build_id: String,
    storage: State<'_, Storage>,
) -> Result<Vec<InstalledContent>, LauncherError> {
    storage.list_installed_content(&build_id).await
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
    if !account_id.starts_with("offline:") {
        return Err(input_error(
            "offline_skin_only",
            "Локальная галерея доступна для офлайн-профилей.",
        ));
    }
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
