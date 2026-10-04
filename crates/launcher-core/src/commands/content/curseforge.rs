//! CurseForge Core integration. The key is supplied only to the native build.
use super::*;

const API: &str = "https://api.curseforge.com/v1";
const KEY: Option<&str> = option_env!("CK_CURSEFORGE_API_KEY");
static CF_CLIENT: std::sync::LazyLock<Client> = std::sync::LazyLock::new(|| {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("CurseForge HTTP client")
});
static CF_DOWNLOAD_CLIENT: std::sync::LazyLock<Option<Client>> = std::sync::LazyLock::new(|| {
    let value = KEY?.trim();
    let mut headers = header::HeaderMap::new();
    headers.insert("x-api-key", header::HeaderValue::from_str(value).ok()?);
    Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5
                || url.scheme() != "https"
                || url.port().is_some_and(|p| p != 443)
                || !url.username().is_empty()
                || url.password().is_some()
                || !security::is_curseforge_cdn(url.host_str().unwrap_or(""))
            {
                attempt.error("CurseForge CDN redirect denied")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .ok()
});

fn key() -> Result<&'static str, LauncherError> {
    KEY.filter(|value| !value.trim().is_empty()).ok_or_else(|| input_error(
        "curseforge_key_missing",
        "Каталог CurseForge недоступен в этой сборке лаунчера: при сборке не был добавлен ключ API.",
    ))
}

fn unavailable() -> LauncherError {
    LauncherError::new(
        "curseforge_unavailable",
        "CurseForge сейчас недоступен. Попробуйте позже.",
        None,
        true,
    )
}

fn class_id(kind: &str) -> Result<u32, LauncherError> {
    match kind {
        "modpack" => Ok(4471),
        "mod" => Ok(6),
        "resourcepack" => Ok(12),
        "shader" => Ok(6552),
        _ => Err(input_error(
            "project_type_unsupported",
            "Тип контента не поддерживается.",
        )),
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
    #[serde(default)]
    pagination: Option<Pagination>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total_count: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mod {
    id: u64,
    name: String,
    summary: String,
    download_count: u64,
    class_id: Option<u32>,
    logo: Option<Logo>,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    is_available: bool,
    #[serde(default)]
    allow_mod_distribution: Option<bool>,
    date_modified: Option<String>,
}
#[derive(Deserialize)]
struct Logo {
    url: String,
}
#[derive(Deserialize)]
struct Author {
    name: String,
}
#[derive(Deserialize)]
struct Category {
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    id: u64,
    mod_id: u64,
    display_name: String,
    file_name: String,
    release_type: u32,
    file_length: u64,
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<Hash>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    is_available: bool,
    #[serde(default)]
    download_count: u64,
    file_date: Option<String>,
    #[serde(default)]
    is_server_pack: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackManifest {
    manifest_type: String,
    manifest_version: u32,
    name: String,
    minecraft: PackMinecraft,
    files: Vec<PackReference>,
    overrides: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackMinecraft {
    version: String,
    mod_loaders: Vec<PackLoader>,
}
#[derive(Deserialize)]
struct PackLoader {
    id: String,
    primary: bool,
}
#[derive(Deserialize)]
struct PackReference {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    required: bool,
}
#[derive(Deserialize)]
struct Hash {
    value: String,
    algo: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dependency {
    mod_id: u64,
    relation_type: u32,
}

impl ContentService {
    async fn cf_json<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Envelope<T>, LauncherError> {
        let request = request
            .header("x-api-key", key()?)
            .build()
            .map_err(|_| unavailable())?;
        if request.url().host_str() != Some("api.curseforge.com")
            || request.url().scheme() != "https"
        {
            return Err(unavailable());
        }
        // Never forward the key on a redirect, even if the API changes behavior.
        let response = CF_CLIENT
            .execute(request)
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(unavailable());
        }
        response
            .json::<Envelope<T>>()
            .await
            .map_err(|_| unavailable())
    }
    pub async fn search_curseforge(
        &self,
        query: String,
        kind: String,
        game_version: Option<String>,
        loader: Option<String>,
        offset: u32,
    ) -> Result<ModrinthSearchResult, LauncherError> {
        let mut params = vec![
            ("gameId", "432".to_owned()),
            ("classId", class_id(&kind)?.to_string()),
            ("index", offset.min(10_000).to_string()),
            ("pageSize", "20".to_owned()),
        ];
        if !query.trim().is_empty() {
            params.push(("searchFilter", query.trim().to_owned()));
        }
        if let Some(version) = game_version.filter(|v| !v.trim().is_empty()) {
            params.push(("gameVersion", version));
            if kind == "mod" {
                let loader_id = match loader.as_deref() {
                    Some("forge") => Some(1),
                    Some("fabric") => Some(4),
                    Some("quilt") => Some(5),
                    Some("neoforge") => Some(6),
                    _ => None,
                };
                if let Some(id) = loader_id {
                    params.push(("modLoaderType", id.to_string()));
                }
            }
        }
        let response: Envelope<Vec<Mod>> = self
            .cf_json(self.client.get(format!("{API}/mods/search")).query(&params))
            .await?;
        Ok(ModrinthSearchResult {
            hits: response
                .data
                .into_iter()
                .filter(|item| item.is_available)
                .map(|item| ModrinthProject {
                    project_id: format!("curseforge:{}", item.id),
                    project_type: kind.clone(),
                    title: item.name,
                    description: item.summary,
                    author: item
                        .authors
                        .first()
                        .map(|a| a.name.clone())
                        .unwrap_or_default(),
                    categories: item.categories.into_iter().map(|c| c.name).collect(),
                    versions: vec![],
                    downloads: item.download_count,
                    follows: 0,
                    icon_url: item.logo.map(|l| l.url),
                    date_modified: item.date_modified.unwrap_or_default(),
                })
                .collect(),
            offset,
            limit: 20,
            total_hits: response.pagination.map_or(0, |p| p.total_count),
        })
    }
    async fn cf_mod(&self, id: u64) -> Result<Mod, LauncherError> {
        Ok(self
            .cf_json::<Mod>(self.client.get(format!("{API}/mods/{id}")))
            .await?
            .data)
    }
    async fn cf_files(
        &self,
        id: u64,
        game_version: Option<&str>,
        loader: Option<&str>,
    ) -> Result<Vec<File>, LauncherError> {
        let mut request = self
            .client
            .get(format!("{API}/mods/{id}/files"))
            .query(&[("pageSize", "50")]);
        if let Some(version) = game_version {
            request = request.query(&[("gameVersion", version)]);
        }
        if let (Some(_), Some(loader)) = (game_version, loader) {
            let loader_id = match loader {
                "forge" => Some(1),
                "fabric" => Some(4),
                "quilt" => Some(5),
                "neoforge" => Some(6),
                _ => None,
            };
            if let Some(id) = loader_id {
                request = request.query(&[("modLoaderType", id)]);
            }
        }
        Ok(self.cf_json::<Vec<File>>(request).await?.data)
    }
    async fn cf_file(&self, mod_id: u64, file_id: u64) -> Result<File, LauncherError> {
        Ok(self
            .cf_json::<File>(
                self.client
                    .get(format!("{API}/mods/{mod_id}/files/{file_id}")),
            )
            .await?
            .data)
    }
    pub async fn curseforge_versions(
        &self,
        id: u64,
        game_version: Option<String>,
        loader: Option<String>,
    ) -> Result<Vec<ModrinthVersionView>, LauncherError> {
        Ok(self
            .cf_files(id, game_version.as_deref(), loader.as_deref())
            .await?
            .into_iter()
            .filter(|f| f.is_available)
            .map(|f| ModrinthVersionView {
                id: f.id.to_string(),
                name: f.display_name,
                version_number: f.file_name,
                version_type: match f.release_type {
                    2 => "beta",
                    3 => "alpha",
                    _ => "release",
                }
                .to_owned(),
                date_published: f.file_date.unwrap_or_default(),
                downloads: f.download_count,
                loaders: vec![],
                game_versions: f.game_versions,
            })
            .collect())
    }
    pub async fn install_curseforge_project(
        &self,
        id: u64,
        build_id: String,
        kind: String,
        version_id: Option<u64>,
    ) -> Result<InstalledContent, LauncherError> {
        if kind == "modpack" {
            return Err(input_error(
                "project_type_unsupported",
                "Используйте установку сборок CurseForge.",
            ));
        }
        let build = self
            .storage
            .list_builds()
            .await?
            .into_iter()
            .find(|b| b.id == build_id)
            .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
        let existing = self.storage.list_installed_content(&build.id).await?;
        let mut completed: HashSet<u64> = existing
            .iter()
            .filter_map(|item| item.project_id.strip_prefix("curseforge:")?.parse().ok())
            .collect();
        let mut queued = HashSet::new();
        let mut pending = Vec::new();
        let mut queue = vec![(id, version_id, kind)];
        while let Some((project_id, selected_file, project_kind)) = queue.pop() {
            if !queued.insert(project_id) {
                continue;
            }
            if queued.len() > 64 {
                return Err(input_error(
                    "dependency_limit",
                    "У проекта слишком много обязательных зависимостей.",
                ));
            }
            let project = self.cf_mod(project_id).await?;
            if project.class_id != Some(class_id(&project_kind)?) {
                return Err(input_error(
                    "project_type_unsupported",
                    "Тип проекта CurseForge не совпадает с выбранным разделом.",
                ));
            }
            let file = self
                .cf_files(
                    project_id,
                    Some(base_game_version(&build)),
                    if project_kind == "mod" {
                        Some(&build.loader)
                    } else {
                        None
                    },
                )
                .await?
                .into_iter()
                .find(|f| f.is_available && selected_file.is_none_or(|v| f.id == v))
                .ok_or_else(|| {
                    input_error(
                        "compatible_version_not_found",
                        "Совместимая версия зависимости не найдена.",
                    )
                })?;
            let dependencies: Vec<u64> = file
                .dependencies
                .iter()
                .filter(|d| d.relation_type == 3)
                .map(|d| d.mod_id)
                .collect();
            for dependency in &dependencies {
                if !completed.contains(dependency) {
                    queue.push((*dependency, None, "mod".to_owned()));
                }
            }
            pending.push((project_id, project_kind, Some(file.id), dependencies));
        }
        let mut main = None;
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|(_, _, _, dependencies)| {
                dependencies.iter().all(|d| completed.contains(d))
            }) else {
                return Err(input_error(
                    "dependency_cycle",
                    "В обязательных зависимостях CurseForge обнаружен цикл.",
                ));
            };
            let (project_id, project_kind, selected_file, _) = pending.remove(index);
            let installed = self
                .install_cf_single(project_id, build.id.clone(), project_kind, selected_file)
                .await?;
            completed.insert(project_id);
            if project_id == id {
                main = Some(installed);
            }
        }
        main.ok_or_else(|| {
            input_error(
                "curseforge_install_incomplete",
                "Установка CurseForge не завершена.",
            )
        })
    }

    async fn install_cf_single(
        &self,
        id: u64,
        build_id: String,
        kind: String,
        version_id: Option<u64>,
    ) -> Result<InstalledContent, LauncherError> {
        if kind == "modpack" {
            return Err(input_error(
                "curseforge_modpack_unsupported",
                "Установка сборок CurseForge пока не готова.",
            ));
        }
        let folder = match kind.as_str() {
            "mod" => "mods",
            "resourcepack" => "resourcepacks",
            "shader" => "shaderpacks",
            _ => {
                class_id(&kind)?;
                unreachable!()
            }
        };
        let build = self
            .storage
            .list_builds()
            .await?
            .into_iter()
            .find(|b| b.id == build_id)
            .ok_or_else(|| input_error("build_not_found", "Сборка не найдена."))?;
        let project = self.cf_mod(id).await?;
        if !project.is_available || project.allow_mod_distribution == Some(false) {
            return Err(input_error(
                "curseforge_distribution_denied",
                "Автор проекта запретил распространение через сторонние лаунчеры.",
            ));
        }
        let files = self
            .cf_files(
                id,
                Some(&base_game_version(&build)),
                if kind == "mod" {
                    Some(&build.loader)
                } else {
                    None
                },
            )
            .await?;
        let file = files
            .into_iter()
            .find(|f| f.is_available && version_id.is_none_or(|v| v == f.id))
            .ok_or_else(|| {
                input_error(
                    "compatible_version_not_found",
                    "Совместимый файл CurseForge не найден.",
                )
            })?;
        if file.mod_id != id || file.file_length == 0 || file.file_length > 512 * 1024 * 1024 {
            return Err(input_error(
                "curseforge_file_invalid",
                "Некорректный файл CurseForge.",
            ));
        }
        let filename = security::relative_path(&file.file_name)?;
        if filename.components().count() != 1 {
            return Err(LauncherError::invalid_path());
        }
        let root = Path::new(&build.game_dir);
        let previous = self.storage.list_installed_content(&build.id).await?;
        if file.dependencies.iter().any(|dependency| {
            dependency.relation_type == 3
                && !previous
                    .iter()
                    .any(|item| item.project_id == format!("curseforge:{}", dependency.mod_id))
        }) {
            return Err(input_error("curseforge_dependency_missing", "Для этого файла нужны другие моды CurseForge. Установите обязательные зависимости перед ним."));
        }
        let project_id = format!("curseforge:{id}");
        if previous
            .iter()
            .any(|p| p.project_id != project_id && p.filename.eq_ignore_ascii_case(&file.file_name))
        {
            return Err(input_error(
                "content_file_conflict",
                "Файл с таким именем уже принадлежит другому проекту.",
            ));
        }
        let old = previous.iter().find(|p| p.project_id == project_id);
        let enabled = old.is_none_or(|p| p.enabled);
        let destination = PathBuf::from(folder).join(if enabled {
            file.file_name.clone()
        } else {
            format!("{}.disabled", file.file_name)
        });
        let staged = self.cf_download(&file, root, 0, 1).await?;
        let item = InstalledContent {
            id: format!("{}:{project_id}", build.id),
            build_id: build.id.clone(),
            project_id,
            version_id: file.id.to_string(),
            project_type: kind,
            title: project.name,
            filename: file.file_name,
            icon_url: project.logo.map(|l| l.url),
            enabled,
        };
        let mut transaction = FileTransaction::new(root)?;
        transaction.replace(&destination, staged.path())?;
        if let Some(old) = old {
            if !old.filename.eq_ignore_ascii_case(&item.filename) {
                transaction.remove(&PathBuf::from(folder).join(if old.enabled {
                    old.filename.clone()
                } else {
                    format!("{}.disabled", old.filename)
                }))?;
            }
        }
        self.storage.upsert_installed_content(&item).await?;
        transaction.commit();
        Ok(item)
    }

    pub async fn install_curseforge_modpack(
        &self,
        id: u64,
        version_id: Option<u64>,
    ) -> Result<InstalledContent, LauncherError> {
        let project = self.cf_mod(id).await?;
        if project.class_id != Some(4471)
            || !project.is_available
            || project.allow_mod_distribution == Some(false)
        {
            return Err(input_error(
                "curseforge_distribution_denied",
                "Эту сборку нельзя установить через сторонний лаунчер.",
            ));
        }
        let file = self
            .cf_files(id, None, None)
            .await?
            .into_iter()
            .find(|f| {
                f.is_available
                    && f.is_server_pack != Some(true)
                    && version_id.is_none_or(|wanted| wanted == f.id)
            })
            .ok_or_else(|| {
                input_error(
                    "compatible_version_not_found",
                    "Файл сборки CurseForge не найден.",
                )
            })?;
        let archive = self
            .cf_download(&file, &self.cache_directory()?, 0, 1)
            .await?;
        self.install_curseforge_pack_archive(id, file.id, archive, None)
            .await
    }

    async fn cf_download(
        &self,
        file: &File,
        root: &Path,
        step: u64,
        total_files: u64,
    ) -> Result<tempfile::NamedTempFile, LauncherError> {
        if !file.is_available || file.file_length == 0 || file.file_length > security::MAX_ARCHIVE {
            return Err(security::limit_error());
        }
        let hashes = FileHashes {
            sha512: None,
            sha1: file
                .hashes
                .iter()
                .find(|h| h.algo == 1)
                .map(|h| h.value.clone()),
        };
        security::validate_hashes(None, hashes.sha1.as_deref())?;
        let url = if let Some(url) = &file.download_url {
            url.clone()
        } else {
            self.cf_json::<Option<String>>(self.client.get(format!(
                "{API}/mods/{}/files/{}/download-url",
                file.mod_id, file.id
            )))
            .await?
            .data
            .ok_or_else(|| {
                input_error(
                    "curseforge_download_restricted",
                    "CurseForge не разрешил автоматическую загрузку одного из файлов.",
                )
            })?
        };
        let parsed = security::validate_url(&url)?;
        if !security::is_curseforge_cdn(parsed.host_str().unwrap_or("")) {
            return Err(input_error(
                "curseforge_download_restricted",
                "Файл CurseForge перенаправлен на неподдерживаемый сервер загрузки.",
            ));
        }
        key()?;
        let client = CF_DOWNLOAD_CLIENT.as_ref().ok_or_else(unavailable)?;
        security::download_with_progress(
            client,
            root,
            &url,
            &hashes,
            Some(file.file_length),
            &self.token(),
            &|done, total| {
                self.events.progress(
                    "content-download",
                    &file.file_name,
                    done,
                    total,
                    step,
                    total_files,
                )
            },
        )
        .await
        .map_err(|error| {
            if error.code() == "modrinth_unavailable" {
                unavailable()
            } else {
                error
            }
        })
    }

    pub(super) async fn repair_curseforge_pack(
        &self,
        id: u64,
        file_id: u64,
        hash: &str,
        build: BuildSummary,
    ) -> Result<InstalledContent, LauncherError> {
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(security::invalid_archive());
        }
        let cached = self.paths.safe_join(
            &self.cache_directory()?,
            Path::new(&format!("{hash}.cfpack")),
        )?;
        let mut archive = tempfile::NamedTempFile::new_in(self.cache_directory()?)
            .map_err(|_| LauncherError::storage_unavailable())?;
        let source = fs::File::open(cached).map_err(|_| {
            input_error(
                "pack_source_missing",
                "Исходный архив CurseForge отсутствует.",
            )
        })?;
        if std::io::copy(&mut source.take(MAX_ARCHIVE + 1), &mut archive)
            .map_err(|_| security::invalid_archive())?
            > MAX_ARCHIVE
            || hash_archive(archive.as_file_mut())? != hash
        {
            return Err(security::invalid_archive());
        }
        self.install_curseforge_pack_archive(id, file_id, archive, Some(build))
            .await
    }

    async fn install_curseforge_pack_archive(
        &self,
        id: u64,
        file_id: u64,
        mut source: tempfile::NamedTempFile,
        existing: Option<BuildSummary>,
    ) -> Result<InstalledContent, LauncherError> {
        let fingerprint = hash_archive(source.as_file_mut())?;
        let manifest: PackManifest = {
            let mut archive =
                zip::ZipArchive::new(source.reopen().map_err(|_| security::invalid_archive())?)
                    .map_err(|_| security::invalid_archive())?;
            if archive.len() > 20_000 {
                return Err(security::limit_error());
            }
            let mut names = HashSet::new();
            let mut expanded = 0u64;
            for index in 0..archive.len() {
                let entry = archive
                    .by_index(index)
                    .map_err(|_| security::invalid_archive())?;
                let name = entry.name().replace('\\', "/");
                security::relative_path(name.trim_end_matches('/'))?;
                if !names.insert(name.to_ascii_lowercase())
                    || entry.unix_mode().is_some_and(|mode| {
                        let kind = mode & 0o170000;
                        kind != 0 && kind != 0o100000 && kind != 0o040000
                    })
                {
                    return Err(security::invalid_archive());
                }
                expanded = expanded
                    .checked_add(entry.size())
                    .ok_or_else(security::limit_error)?;
                if expanded > 4 * 1024 * 1024 * 1024
                    || entry.size() > 512 * 1024 * 1024
                    || (entry.size() > 1024 * 1024
                        && entry.size() / entry.compressed_size().max(1) > 200)
                {
                    return Err(security::limit_error());
                }
            }
            let manifest_entry = archive
                .by_name("manifest.json")
                .map_err(|_| security::invalid_archive())?;
            if manifest_entry.size() > 4 * 1024 * 1024 {
                return Err(security::limit_error());
            }
            let mut manifest_bytes = Vec::new();
            manifest_entry
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut manifest_bytes)
                .map_err(|_| security::invalid_archive())?;
            serde_json::from_slice(&manifest_bytes).map_err(|_| security::invalid_archive())?
        };
        if manifest.manifest_type != "minecraftModpack"
            || manifest.manifest_version != 1
            || manifest.files.len() > 10_000
            || manifest.name.trim().is_empty()
            || manifest.name.chars().count() > 48
        {
            return Err(security::invalid_archive());
        }
        if !manifest
            .minecraft
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
            || manifest.minecraft.version.len() > 128
        {
            return Err(security::invalid_archive());
        }
        let override_root = security::relative_path(&manifest.overrides)?;
        if override_root.components().count() != 1 {
            return Err(security::invalid_archive());
        }
        let loader = manifest
            .minecraft
            .mod_loaders
            .iter()
            .find(|l| l.primary)
            .or_else(|| manifest.minecraft.mod_loaders.first());
        if manifest.minecraft.mod_loaders.len() > 1 {
            return Err(input_error(
                "loader_not_supported",
                "Сборка требует несколько загрузчиков Minecraft.",
            ));
        }
        let (loader_name, loader_version) = if let Some(loader) = loader {
            let (name, version) = loader
                .id
                .split_once('-')
                .ok_or_else(|| security::invalid_archive())?;
            if !["fabric", "quilt", "forge"].contains(&name)
                || version.is_empty()
                || version.len() > 128
                || !version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
            {
                return Err(input_error(
                    "loader_not_supported",
                    "Загрузчик сборки не поддерживается.",
                ));
            }
            (name.to_owned(), Some(version.to_owned()))
        } else {
            ("vanilla".to_owned(), None)
        };
        let cache = self.cache_directory()?;
        let staging = tempfile::Builder::new()
            .prefix(".ck-cf-staging-")
            .tempdir_in(&cache)
            .map_err(|_| LauncherError::storage_unavailable())?;
        let mut downloaded = Vec::new();
        let mut targets = HashSet::new();
        let mut total_download_bytes = 0u64;
        let required_ids: HashSet<u64> = manifest
            .files
            .iter()
            .filter(|file| file.required)
            .map(|file| file.project_id)
            .collect();
        for (position, reference) in manifest
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.required)
        {
            if self.token().is_cancelled() {
                return Err(security::cancelled());
            }
            let project = self.cf_mod(reference.project_id).await?;
            if !project.is_available || project.allow_mod_distribution == Some(false) {
                return Err(input_error("curseforge_distribution_denied", "Автор одного из файлов сборки запретил распространение через сторонние лаунчеры."));
            }
            let kind = match project.class_id {
                Some(6) => "mod",
                Some(12) => "resourcepack",
                Some(6552) => "shader",
                _ => {
                    return Err(input_error(
                        "project_type_unsupported",
                        "Сборка содержит неподдерживаемый тип файла.",
                    ))
                }
            };
            let folder = match kind {
                "mod" => "mods",
                "resourcepack" => "resourcepacks",
                _ => "shaderpacks",
            };
            let file = self
                .cf_file(reference.project_id, reference.file_id)
                .await?;
            if file.mod_id != reference.project_id
                || file.id != reference.file_id
                || !file.is_available
            {
                return Err(security::invalid_archive());
            }
            total_download_bytes = total_download_bytes
                .checked_add(file.file_length)
                .ok_or_else(security::limit_error)?;
            if total_download_bytes > 8 * 1024 * 1024 * 1024 {
                return Err(security::limit_error());
            }
            if file.dependencies.iter().any(|dependency| {
                dependency.relation_type == 3 && !required_ids.contains(&dependency.mod_id)
            }) {
                return Err(input_error(
                    "curseforge_dependency_missing",
                    "В сборке не указан один из обязательных модов. Установка остановлена.",
                ));
            }
            if !file
                .game_versions
                .iter()
                .any(|v| v == &manifest.minecraft.version)
            {
                return Err(input_error(
                    "compatible_version_not_found",
                    "Один из файлов сборки несовместим с указанной версией Minecraft.",
                ));
            }
            let filename = security::relative_path(&file.file_name)?;
            if filename.components().count() != 1 {
                return Err(LauncherError::invalid_path());
            }
            let relative = PathBuf::from(folder).join(&filename);
            if !targets.insert(relative.to_string_lossy().to_ascii_lowercase()) {
                return Err(security::invalid_archive());
            }
            self.events.progress(
                "content-download",
                &file.file_name,
                0,
                file.file_length,
                position as u64,
                manifest.files.len() as u64,
            );
            let staged = self
                .cf_download(
                    &file,
                    staging.path(),
                    position as u64,
                    manifest.files.len() as u64,
                )
                .await?;
            downloaded.push((
                relative,
                staged,
                reference.project_id,
                reference.file_id,
                kind.to_owned(),
                project.name,
                project.logo.map(|l| l.url),
                file.file_name,
            ));
        }
        let prefix = format!("{}/", manifest.overrides);
        {
            let mut archive =
                zip::ZipArchive::new(source.reopen().map_err(|_| security::invalid_archive())?)
                    .map_err(|_| security::invalid_archive())?;
            for index in 0..archive.len() {
                if self.token().is_cancelled() {
                    return Err(security::cancelled());
                }
                let entry = archive
                    .by_index(index)
                    .map_err(|_| security::invalid_archive())?;
                let name = entry.name().replace('\\', "/");
                let Some(relative) = name.strip_prefix(&prefix).filter(|_| !entry.is_dir()) else {
                    continue;
                };
                let path = security::relative_path(relative)?;
                if !targets.insert(path.to_string_lossy().to_ascii_lowercase()) {
                    return Err(security::invalid_archive());
                }
                let size = entry.size();
                let mut staged = tempfile::NamedTempFile::new_in(staging.path())
                    .map_err(|_| LauncherError::storage_unavailable())?;
                if std::io::copy(&mut entry.take(size + 1), &mut staged)
                    .map_err(|_| security::invalid_archive())?
                    != size
                {
                    return Err(security::invalid_archive());
                }
                downloaded.push((
                    path,
                    staged,
                    0,
                    0,
                    String::new(),
                    String::new(),
                    None,
                    String::new(),
                ));
            }
        }
        let new_build = existing.is_none();
        let mut build = if let Some(build) = existing {
            build
        } else {
            let instance_id = format!("build-{:032x}", rand::random::<u128>());
            let instances = self
                .paths
                .safe_join(&self.paths.root, Path::new("instances"))?;
            let game_dir = self.paths.safe_join(&instances, Path::new(&instance_id))?;
            fs::create_dir(&game_dir).map_err(|_| LauncherError::storage_unavailable())?;
            BuildSummary {
                id: instance_id,
                name: manifest.name.clone(),
                game_version: manifest.minecraft.version.clone(),
                loader: loader_name.clone(),
                loader_version: loader_version.clone(),
                game_dir: crate::paths::strip_verbatim_prefix(game_dir)
                    .to_string_lossy()
                    .into_owned(),
                icon_url: None,
                is_active: true,
            }
        };
        let installation = async {
            let minecraft = &manifest.minecraft.version;
            let installed_loader = match loader_name.as_str() {
                "fabric" => Some(self.install_fabric_profile(minecraft, loader_version.as_deref()).await?),
                "quilt" => Some(self.install_quilt_profile(minecraft, loader_version.as_deref()).await?),
                "forge" => Some(self.install_forge_profile(minecraft, loader_version.as_deref(), Path::new(&build.game_dir)).await?),
                _ => None,
            };
            build.game_version = installed_loader.as_ref().map(|v| format!("{loader_name}-loader-{v}-{minecraft}")).unwrap_or_else(|| minecraft.clone());
            build.loader = loader_name;
            build.loader_version = installed_loader;
            let pack = self.cf_mod(id).await?;
            if new_build { build.icon_url = pack.logo.map(|l| l.url); }
            let root = Path::new(&build.game_dir);
            let previous = self.storage.list_installed_content(&build.id).await?;
            let mut transaction = FileTransaction::new(root)?;
            let mut records = Vec::new();
            for (relative, staged, project_id, version_id, kind, title, icon, filename) in downloaded {
                if self.token().is_cancelled() { return Err(security::cancelled()); }
                if !new_build && kind.is_empty() && security::safe_destination(root, &relative)?.exists() { continue; }
                let previous_item = previous.iter().find(|item| item.project_id == format!("curseforge:{project_id}"));
                let enabled = previous_item.is_none_or(|item| item.enabled);
                let destination = if enabled { relative.clone() } else { relative.with_file_name(format!("{filename}.disabled")) };
                transaction.replace(&destination, staged.path())?;
                if project_id != 0 {
                    let project_id = format!("curseforge:{project_id}");
                    records.push(InstalledContent { id: format!("{}:{project_id}", build.id), build_id: build.id.clone(), project_id, version_id: version_id.to_string(), project_type: kind, title, filename, icon_url: icon, enabled });
                }
            }
            let item = InstalledContent { id: format!("{}:curseforge:{id}", build.id), build_id: build.id.clone(), project_id: format!("curseforge:{id}"), version_id: file_id.to_string(), project_type: "modpack".to_owned(), title: manifest.name, filename: format!("{id}-{file_id}.zip"), icon_url: build.icon_url.clone(), enabled: true };
            records.push(item.clone());
            let cached = self.paths.safe_join(&cache, Path::new(&format!("{fingerprint}.cfpack")))?;
            if !cached.exists() { fs::copy(source.path(), cached).map_err(|_| LauncherError::storage_unavailable())?; }
            let source_json = serde_json::json!({"kind":"curseforge","sha256":fingerprint,"projectId":id,"versionId":file_id}).to_string();
            self.storage.commit_pack(&build, &records, &source_json).await?;
            transaction.commit();
            Ok(item)
        }.await;
        if installation.is_err() && new_build {
            let _ = fs::remove_dir(&build.game_dir);
        }
        installation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_supported_minecraft_classes() {
        assert_eq!(class_id("modpack").unwrap(), 4471);
        assert_eq!(class_id("mod").unwrap(), 6);
        assert_eq!(class_id("resourcepack").unwrap(), 12);
        assert_eq!(class_id("shader").unwrap(), 6552);
        assert!(class_id("world").is_err());
    }

    #[test]
    fn key_bearing_downloads_stay_on_curseforge_cdn() {
        assert!(security::is_curseforge_cdn("edge.forgecdn.net"));
        assert!(security::is_curseforge_cdn("123.mediafilez.forgecdn.net"));
        assert!(!security::is_curseforge_cdn(
            "edge.forgecdn.net.attacker.example"
        ));
        assert!(!security::is_curseforge_cdn("evil.mediafilez.forgecdn.net"));
        assert!(security::validate_url("https://123.mediafilez.forgecdn.net/files/a.jar").is_ok());
    }

    #[test]
    fn reads_official_pack_references_without_following_embedded_urls() {
        let manifest: PackManifest = serde_json::from_value(serde_json::json!({
            "manifestType":"minecraftModpack", "manifestVersion":1, "name":"Example",
            "minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.3.0","primary":true}]},
            "files":[{"projectID":123,"fileID":456,"required":true}], "overrides":"overrides"
        }))
        .unwrap();
        assert_eq!(manifest.files[0].project_id, 123);
        assert_eq!(manifest.files[0].file_id, 456);
        assert!(manifest.files[0].required);
        assert_eq!(manifest.minecraft.mod_loaders[0].id, "forge-47.3.0");
    }
}
