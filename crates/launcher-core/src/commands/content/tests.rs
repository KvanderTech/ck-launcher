use super::*;
use std::io::Write;

async fn service(root: &Path) -> ContentService {
    let paths = AppPaths::new(root.to_owned());
    paths.create_directories().unwrap();
    let storage = Storage::connect_file(&paths.database).await.unwrap();
    let metadata = Arc::new(MetadataService::production(root.join("metadata-cache")).unwrap());
    let runtimes =
        Arc::new(crate::runtime::RuntimeManager::production(paths.runtime.clone()).unwrap());
    ContentService::new(paths, storage, metadata, runtimes).unwrap()
}

fn build(root: &Path, id: &str) -> BuildSummary {
    let game_dir = root.join("instances").join(id);
    fs::create_dir_all(&game_dir).unwrap();
    BuildSummary {
        id: id.to_owned(),
        name: "Test build".to_owned(),
        game_version: "1.20.1".to_owned(),
        loader: "vanilla".to_owned(),
        loader_version: None,
        game_dir: game_dir.to_string_lossy().into_owned(),
        icon_url: None,
        is_active: true,
    }
}

fn content(build: &BuildSummary, id: &str, filename: &str, enabled: bool) -> InstalledContent {
    InstalledContent {
        id: format!("{}:{id}", build.id),
        build_id: build.id.clone(),
        project_id: id.to_owned(),
        version_id: "version-one".to_owned(),
        project_type: "mod".to_owned(),
        title: id.to_owned(),
        filename: filename.to_owned(),
        icon_url: None,
        enabled,
    }
}

fn pack(root: &Path, entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
    pack_with_index(
        root,
        entries,
        serde_json::json!({"formatVersion":1,"game":"minecraft","versionId":"fixture","name":"Original pack","dependencies":{"minecraft":"1.20.1"},"files":[]}),
    )
}

fn pack_with_index(
    root: &Path,
    entries: &[(&str, &[u8])],
    index: serde_json::Value,
) -> tempfile::NamedTempFile {
    let mut output = tempfile::NamedTempFile::new_in(root).unwrap();
    let mut zip = zip::ZipWriter::new(&mut output);
    zip.start_file(
        "modrinth.index.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(&serde_json::to_vec(&index).unwrap()).unwrap();
    for (path, bytes) in entries {
        zip.start_file(*path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    output.seek(SeekFrom::Start(0)).unwrap();
    output
}

fn project() -> ProjectDetails {
    ProjectDetails {
        id: "fixture-pack".to_owned(),
        title: "Original pack".to_owned(),
        project_type: "modpack".to_owned(),
        icon_url: Some("https://cdn.modrinth.com/original.png".to_owned()),
        description: String::new(),
        body: String::new(),
        downloads: 0,
        followers: 0,
        categories: vec![],
    }
}

#[test]
fn required_dependency_is_enabled_and_missing_or_wrong_versions_are_not_skipped() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let build = build(temp.path(), "test");
        service.storage.upsert_build(&build).await.unwrap();
        let dependency = content(&build, "dependency", "dependency.jar", false);
        service
            .storage
            .upsert_installed_content(&dependency)
            .await
            .unwrap();
        let mods = Path::new(&build.game_dir).join("mods");
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("dependency.jar.disabled"), b"fixture").unwrap();
        assert!(!enable_required_dependency(
            &service.storage,
            &build,
            "dependency",
            Some("different")
        )
        .await
        .unwrap());
        assert!(enable_required_dependency(
            &service.storage,
            &build,
            "dependency",
            Some("version-one")
        )
        .await
        .unwrap());
        assert!(mods.join("dependency.jar").is_file());
        assert!(!mods.join("dependency.jar.disabled").exists());
        assert!(
            service
                .storage
                .list_installed_content(&build.id)
                .await
                .unwrap()[0]
                .enabled
        );
        fs::remove_file(mods.join("dependency.jar")).unwrap();
        assert!(
            !enable_required_dependency(&service.storage, &build, "dependency", None)
                .await
                .unwrap()
        );
    });
}

#[test]
fn incompatible_dependency_honors_exact_version_and_disabled_state() {
    let temp = tempfile::tempdir().unwrap();
    let build = build(temp.path(), "test");
    let mut item = content(&build, "dependency", "dependency.jar", true);
    let mut dependency = ProjectDependency {
        project_id: Some("dependency".to_owned()),
        version_id: Some("different".to_owned()),
        dependency_type: "incompatible".to_owned(),
    };
    assert!(!incompatible_dependency_matches(&dependency, &item));
    dependency.version_id = Some(item.version_id.clone());
    assert!(incompatible_dependency_matches(&dependency, &item));
    item.enabled = false;
    assert!(!incompatible_dependency_matches(&dependency, &item));
    item.enabled = true;
    dependency.version_id = None;
    assert!(incompatible_dependency_matches(&dependency, &item));
    dependency.project_id = None;
    assert!(!incompatible_dependency_matches(&dependency, &item));
}

#[test]
fn local_import_does_not_overwrite_modrinth_or_untracked_files() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let build = build(temp.path(), "test");
        service.storage.upsert_build(&build).await.unwrap();
        let item = content(&build, "modrinth-project", "existing.jar", true);
        service
            .storage
            .upsert_installed_content(&item)
            .await
            .unwrap();
        let mods = Path::new(&build.game_dir).join("mods");
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("existing.jar"), b"original").unwrap();
        let source = temp.path().join("existing.jar");
        fs::write(&source, b"replacement").unwrap();
        assert_eq!(
            import_local_file(&build, "mod", &source, &service.storage)
                .await
                .unwrap_err()
                .code(),
            "content_file_conflict"
        );
        assert_eq!(fs::read(mods.join("existing.jar")).unwrap(), b"original");
        assert_eq!(
            service
                .storage
                .list_installed_content(&build.id)
                .await
                .unwrap(),
            vec![item]
        );

        let source = temp.path().join("untracked.jar");
        fs::write(&source, b"replacement").unwrap();
        fs::write(mods.join("untracked.jar"), b"user file").unwrap();
        assert_eq!(
            import_local_file(&build, "mod", &source, &service.storage)
                .await
                .unwrap_err()
                .code(),
            "content_file_conflict"
        );
        assert_eq!(fs::read(mods.join("untracked.jar")).unwrap(), b"user file");
    });
}

#[test]
fn local_reimport_preserves_disabled_state_and_does_not_duplicate_records() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let build = build(temp.path(), "test");
        service.storage.upsert_build(&build).await.unwrap();
        let source = temp.path().join("local.jar");
        fs::write(&source, b"first").unwrap();
        let original = import_local_file(&build, "mod", &source, &service.storage)
            .await
            .unwrap();
        set_installed_content_enabled(
            build.id.clone(),
            original.project_id.clone(),
            false,
            &service.storage,
        )
        .await
        .unwrap();
        fs::write(&source, b"second").unwrap();
        let changed = import_local_file(&build, "mod", &source, &service.storage)
            .await
            .unwrap();
        assert_eq!(changed.project_id, original.project_id);
        assert!(!changed.enabled);
        let mods = Path::new(&build.game_dir).join("mods");
        assert!(!mods.join("local.jar").exists());
        assert_eq!(
            fs::read(mods.join("local.jar.disabled")).unwrap(),
            b"second"
        );
        assert_eq!(
            service
                .storage
                .list_installed_content(&build.id)
                .await
                .unwrap()
                .len(),
            1
        );
    });
}

#[test]
fn local_resourcepack_and_shader_with_same_name_have_separate_records() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let build = build(temp.path(), "test");
        service.storage.upsert_build(&build).await.unwrap();
        let source = temp.path().join("pack.zip");
        fs::write(&source, b"fixture").unwrap();
        let resource = import_local_file(&build, "resourcepack", &source, &service.storage)
            .await
            .unwrap();
        let shader = import_local_file(&build, "shader", &source, &service.storage)
            .await
            .unwrap();
        assert_ne!(resource.project_id, shader.project_id);
        assert_eq!(
            service
                .storage
                .list_installed_content(&build.id)
                .await
                .unwrap()
                .len(),
            2
        );
        remove_installed_content(build.id.clone(), shader.project_id, &service.storage)
            .await
            .unwrap();
        assert!(Path::new(&build.game_dir)
            .join("resourcepacks/pack.zip")
            .is_file());
        assert!(!Path::new(&build.game_dir)
            .join("shaderpacks/pack.zip")
            .exists());
    });
}

#[test]
fn pack_version_replacement_removes_only_old_tracked_files_and_rolls_back() {
    let temp = tempfile::tempdir().unwrap();
    let build = build(temp.path(), "test");
    let root = Path::new(&build.game_dir);
    fs::create_dir_all(root.join("mods")).unwrap();
    fs::write(root.join("mods/old.jar.disabled"), b"old").unwrap();
    fs::write(root.join("mods/user.jar"), b"user").unwrap();
    let previous = [content(&build, "project", "old.jar", false)];
    let next = [content(&build, "project", "new.jar", false)];
    {
        let mut transaction = FileTransaction::new(root).unwrap();
        remove_replaced_content_files(&mut transaction, &previous, &next).unwrap();
        assert!(!root.join("mods/old.jar.disabled").exists());
    }
    assert_eq!(
        fs::read(root.join("mods/old.jar.disabled")).unwrap(),
        b"old"
    );
    let mut transaction = FileTransaction::new(root).unwrap();
    remove_replaced_content_files(&mut transaction, &previous, &next).unwrap();
    transaction.commit();
    assert!(!root.join("mods/old.jar.disabled").exists());
    assert_eq!(fs::read(root.join("mods/user.jar")).unwrap(), b"user");
}

#[test]
fn pack_repair_preserves_custom_name_icon_configs_worlds_and_disabled_overrides() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let build = build(temp.path(), "test");
        let entries = [
            ("overrides/config/options.txt", b"default".as_slice()),
            ("overrides/saves/world/level.dat", b"template".as_slice()),
            ("overrides/mods/local.jar", b"mod".as_slice()),
        ];
        service
            .install_mrpack_archive(
                project(),
                "version-one".to_owned(),
                "test.mrpack".to_owned(),
                pack(temp.path(), &entries),
                build.clone(),
                false,
            )
            .await
            .unwrap();
        let root = Path::new(&build.game_dir);
        fs::write(root.join("config/options.txt"), b"my config").unwrap();
        fs::write(root.join("saves/world/level.dat"), b"my world").unwrap();
        fs::rename(
            root.join("mods/local.jar"),
            root.join("mods/local.jar.disabled"),
        )
        .unwrap();
        service
            .storage
            .update_build_identity(
                &build.id,
                Some("My renamed pack"),
                Some("data:image/png;base64,custom"),
            )
            .await
            .unwrap();
        let repaired = service.repair_build(build.id.clone()).await.unwrap();
        assert_eq!(repaired.name, "My renamed pack");
        assert_eq!(
            repaired.icon_url.as_deref(),
            Some("data:image/png;base64,custom")
        );
        assert_eq!(
            fs::read(root.join("config/options.txt")).unwrap(),
            b"my config"
        );
        assert_eq!(
            fs::read(root.join("saves/world/level.dat")).unwrap(),
            b"my world"
        );
        assert!(!root.join("mods/local.jar").exists());
        assert_eq!(
            fs::read(root.join("mods/local.jar.disabled")).unwrap(),
            b"mod"
        );
    });
}

#[test]
fn pack_repair_preserves_downloaded_user_files_without_network_or_overwriting() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let mut service = service(temp.path()).await;
        // A regression must fail locally rather than reach an external server.
        service.client = Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap();
        let build = build(temp.path(), "test");
        service.storage.upsert_build(&build).await.unwrap();
        let root = Path::new(&build.game_dir);
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/settings.json"), b"user settings").unwrap();
        let index = serde_json::json!({"formatVersion":1,"game":"minecraft","versionId":"fixture","name":"Original pack","dependencies":{"minecraft":"1.20.1"},"files":[{"path":"config/settings.json","fileSize":1,"hashes":{"sha1":"ab".repeat(20)},"downloads":["https://cdn.modrinth.com/fixture"]}]});
        service
            .install_mrpack_archive(
                project(),
                "version-one".to_owned(),
                "test.mrpack".to_owned(),
                pack_with_index(temp.path(), &[], index),
                build.clone(),
                true,
            )
            .await
            .unwrap();
        assert_eq!(
            fs::read(root.join("config/settings.json")).unwrap(),
            b"user settings"
        );
    });
}

#[test]
fn failed_new_modpack_download_keeps_previous_active_build_and_profile() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let active = build(temp.path(), "active");
        service
            .storage
            .upsert_profile(&crate::storage::LauncherProfile {
                id: "default".to_owned(),
                name: active.name.clone(),
                version_id: Some(active.game_version.clone()),
                memory_mb: 4096,
                game_dir: active.game_dir.clone(),
                java_override: None,
            })
            .await
            .unwrap();
        service.storage.upsert_build(&active).await.unwrap();
        let profile_before = service.storage.active_profile().await.unwrap();
        let version: ProjectVersion = serde_json::from_value(serde_json::json!({
            "id":"broken-version","project_id":"fixture-pack","files":[{
                "hashes":{"sha1":"ab".repeat(20)},"url":"http://untrusted.invalid/test.mrpack",
                "filename":"test.mrpack","primary":true,"size":1
            }]
        }))
        .unwrap();
        let error = service
            .install_modpack_version(project(), version)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "download_url_denied");
        assert_eq!(service.storage.list_builds().await.unwrap(), vec![active]);
        assert_eq!(
            service.storage.active_profile().await.unwrap(),
            profile_before
        );
        assert_eq!(
            fs::read_dir(temp.path().join("instances")).unwrap().count(),
            1
        );
    });
}

#[test]
fn cancelled_pack_install_does_not_register_or_change_active_build() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let active = build(temp.path(), "active");
        service.storage.upsert_build(&active).await.unwrap();
        let pending = build(temp.path(), "pending");
        service.cancel();
        let error = service
            .install_mrpack_archive(
                project(),
                "version-one".to_owned(),
                "test.mrpack".to_owned(),
                pack(temp.path(), &[]),
                pending.clone(),
                false,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "operation_cancelled");
        assert_eq!(service.storage.list_builds().await.unwrap(), vec![active]);
        assert!(service
            .storage
            .list_installed_content(&pending.id)
            .await
            .unwrap()
            .is_empty());
    });
}

#[test]
fn skin_delete_keeps_file_when_database_delete_fails() {
    crate::tasks::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let service = service(temp.path()).await;
        let account_id = "offline:fixture";
        service
            .storage
            .upsert_account(&crate::storage::AccountSummary {
                id: account_id.to_owned(),
                minecraft_name: "Fixture".to_owned(),
                minecraft_uuid: "fixture".to_owned(),
                head_url: None,
                is_active: true,
            })
            .await
            .unwrap();
        let directory = temp
            .path()
            .join("skins")
            .join(format!("{:x}", Sha256::digest(account_id.as_bytes())));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("skin-one.png");
        fs::write(&file, b"skin fixture").unwrap();
        let skin = OfflineSkin {
            id: "skin-one".to_owned(),
            account_id: account_id.to_owned(),
            name: "Fixture".to_owned(),
            file_path: file.to_string_lossy().into_owned(),
            is_active: true,
            is_favorite: true,
        };
        service.storage.add_offline_skin(&skin).await.unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new().filename(&service.paths.database),
            )
            .await
            .unwrap();
        sqlx::query("CREATE TRIGGER reject_skin_delete BEFORE DELETE ON offline_skins BEGIN SELECT RAISE(ABORT,'fixture'); END").execute(&pool).await.unwrap();
        assert!(
            delete_offline_skin(account_id.to_owned(), skin.id.clone(), &service)
                .await
                .is_err()
        );
        assert_eq!(fs::read(&file).unwrap(), b"skin fixture");
        assert_eq!(
            service
                .storage
                .list_offline_skins(account_id)
                .await
                .unwrap(),
            vec![skin]
        );
        pool.close().await;
    });
}
