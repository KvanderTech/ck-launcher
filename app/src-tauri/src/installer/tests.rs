use super::{
    assets::asset_object_path,
    libraries::{library_allowed, maven_artifact_path, validate_metadata_path, WindowsRuleContext},
    natives::{
        activate_staging_with_cleanup, extract_natives_transactional, recover_interrupted,
        validate_native_budget, MAX_NATIVE_ENTRIES, MAX_TOTAL_NATIVE_BYTES,
    },
    plan_installation, InstallFileKind, InstallationStore, Installer, NativeArchive,
    OperationRegistry, OperationState, PhaseProgressSink, VerifiedDownloader, VersionProvider,
    MAX_TERMINAL_OPERATIONS,
};
use crate::metadata::models::{ResolvedVersion, VersionJson};
use crate::{
    downloads::{
        DownloadCancellationToken, DownloadProgress, DownloadService, DownloadSpec, ProgressSink,
    },
    error::LauncherError,
    metadata::models::AssetIndex,
};
use async_trait::async_trait;
use sha1::{Digest, Sha1};
use std::io::{Cursor, Read, Write};
use std::{
    fs,
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use zip::{write::SimpleFileOptions, ZipWriter};

fn temporary_game(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ck-launcher-installer-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("temporary game root");
    root
}

fn resolved_fixture() -> ResolvedVersion {
    let version: VersionJson = serde_json::from_str(include_str!(
        "../tests/fixtures/installer_plan_version.json"
    ))
    .expect("installer version fixture");
    ResolvedVersion::from(version)
}

#[test]
fn fixture_plan_contains_verified_vanilla_files_and_only_windows_native() {
    let game = temporary_game("plan");
    let index_body = include_bytes!("../tests/fixtures/installer_asset_index.json");
    let index_path = game.join("assets/indexes/fixture-assets.json");
    fs::create_dir_all(index_path.parent().expect("asset index parent")).expect("parent");
    fs::write(&index_path, index_body).expect("verified fixture index");

    let plan = plan_installation(&game, &resolved_fixture()).expect("fixture plan");
    let kinds = plan.files.iter().map(|file| file.kind).collect::<Vec<_>>();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::VersionJson)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::Client)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::Logging)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::AssetIndex)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::AssetObject)
            .count(),
        2
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::Library)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == InstallFileKind::Native)
            .count(),
        1
    );
    assert!(plan
        .files
        .iter()
        .all(|file| !file.destination.to_string_lossy().contains("linux")));
    assert!(plan
        .files
        .iter()
        .all(|file| !file.destination.to_string_lossy().contains("osx")));
    let ordinary = plan
        .files
        .iter()
        .find(|file| file.kind == InstallFileKind::Library)
        .expect("ordinary library");
    assert_eq!(
        ordinary.url.as_deref(),
        Some("https://example.test/ordinary.jar")
    );
    assert!(ordinary
        .destination
        .ends_with("org/example/ordinary/1.0/ordinary-1.0.jar"));
    let logging = plan
        .files
        .iter()
        .find(|file| file.kind == InstallFileKind::Logging)
        .expect("logging config");
    assert!(logging
        .destination
        .ends_with("assets/log_configs/log4j.xml"));

    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn rules_use_last_matching_action_for_windows_x64_and_features() {
    let context = WindowsRuleContext::default();
    let no_rules = serde_json::from_str(r#"{"name":"a:b:1"}"#).expect("library");
    assert!(library_allowed(&no_rules, &context).expect("rules"));

    let allow_then_disallow = serde_json::from_str(
        r#"{"name":"a:b:1","rules":[{"action":"allow","os":{"name":"windows"}},{"action":"disallow","os":{"name":"windows","arch":"x86_64"}}]}"#,
    ).expect("library");
    assert!(!library_allowed(&allow_then_disallow, &context).expect("rules"));

    let feature_rule = serde_json::from_str(
        r#"{"name":"a:b:1","rules":[{"action":"disallow"},{"action":"allow","features":{"has_custom_resolution":false}}]}"#,
    ).expect("library");
    assert!(library_allowed(&feature_rule, &context).expect("rules"));
}

#[test]
fn maven_coordinates_and_metadata_paths_are_safe_and_deterministic() {
    assert_eq!(
        maven_artifact_path("org.example:demo:1.2.3").expect("plain coordinate"),
        PathBuf::from("org/example/demo/1.2.3/demo-1.2.3.jar")
    );
    assert_eq!(
        maven_artifact_path("org.example:demo:1.2.3:natives-windows@zip")
            .expect("classified extension"),
        PathBuf::from("org/example/demo/1.2.3/demo-1.2.3-natives-windows.zip")
    );
    for unsafe_coordinate in ["../evil:x:1", "a:b", "a:b:1@../zip", "a::1"] {
        assert!(
            maven_artifact_path(unsafe_coordinate).is_err(),
            "{unsafe_coordinate}"
        );
    }
}

#[test]
fn asset_hashes_map_to_content_addressed_paths_and_malformed_hashes_fail() {
    let hash = "00112233445566778899aabbccddeeff00112233";
    assert_eq!(
        asset_object_path(hash).expect("valid SHA-1"),
        PathBuf::from("assets/objects/00/00112233445566778899aabbccddeeff00112233")
    );
    for malformed in [
        "",
        "0",
        "gg112233445566778899aabbccddeeff00112233",
        "../0011",
    ] {
        assert!(asset_object_path(malformed).is_err(), "{malformed}");
    }
}

#[test]
fn asset_index_must_match_hash_before_object_plan_expands() {
    let game = temporary_game("bad-index");
    let index_path = game.join("assets/indexes/fixture-assets.json");
    fs::create_dir_all(index_path.parent().expect("asset index parent")).expect("parent");
    fs::write(index_path, b"{}").expect("corrupt cached index");

    let error = plan_installation(&game, &resolved_fixture()).expect_err("bad index is rejected");
    assert_eq!(error.code(), "asset_index_invalid");
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn fixture_asset_index_hash_is_pinned_to_its_exact_bytes() {
    let body = include_bytes!("../tests/fixtures/installer_asset_index.json");
    assert_eq!(
        format!("{:x}", Sha1::digest(body)),
        "f12c6162a8b2dfc72557cbcf499f3dcffd772df0"
    );
}

fn zip_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in entries {
        zip.start_file(*name, SimpleFileOptions::default())
            .expect("zip entry");
        zip.write_all(body).expect("zip body");
    }
    zip.finish().expect("zip closes").into_inner()
}

#[test]
fn native_extraction_excludes_metadata_and_configured_prefixes() {
    let game = temporary_game("native-excludes");
    let archive = game.join("libraries/native.jar");
    fs::create_dir_all(archive.parent().expect("library parent")).expect("parent");
    fs::write(
        &archive,
        zip_archive(&[
            ("good.dll", b"good"),
            ("META-INF/MANIFEST.MF", b"meta"),
            ("skip/ignored.dll", b"skip"),
        ]),
    )
    .expect("archive");

    let destination = extract_natives_transactional(
        &game,
        "fixture-1.0",
        &[NativeArchive {
            archive,
            excludes: vec!["skip/".to_owned()],
        }],
        &DownloadCancellationToken::new(),
    )
    .expect("safe extraction");

    assert_eq!(
        fs::read(destination.join("good.dll")).expect("native"),
        b"good"
    );
    assert!(!destination.join("META-INF").exists());
    assert!(!destination.join("skip").exists());
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn unsafe_native_entry_rolls_back_existing_destination() {
    let game = temporary_game("native-rollback");
    let destination = game.join("versions/fixture-1.0/natives");
    fs::create_dir_all(&destination).expect("old destination");
    fs::write(destination.join("old.dll"), b"old").expect("old native");
    let archive = game.join("libraries/native.jar");
    fs::create_dir_all(archive.parent().expect("library parent")).expect("parent");
    fs::write(
        &archive,
        zip_archive(&[("new.dll", b"new"), ("../escape.dll", b"escape")]),
    )
    .expect("archive");

    let error = extract_natives_transactional(
        &game,
        "fixture-1.0",
        &[NativeArchive {
            archive,
            excludes: Vec::new(),
        }],
        &DownloadCancellationToken::new(),
    )
    .expect_err("traversal is rejected");

    assert_eq!(error.code(), "native_archive_invalid");
    assert_eq!(
        fs::read(destination.join("old.dll")).expect("old retained"),
        b"old"
    );
    assert!(!game.join("escape.dll").exists());
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn native_archive_symbolic_links_are_rejected() {
    let game = temporary_game("native-link");
    let archive = game.join("libraries/native.jar");
    fs::create_dir_all(archive.parent().expect("library parent")).expect("parent");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink("linked.dll", "outside.dll", SimpleFileOptions::default())
        .expect("symlink entry");
    fs::write(&archive, zip.finish().expect("zip closes").into_inner()).expect("archive");

    let error = extract_natives_transactional(
        &game,
        "fixture-1.0",
        &[NativeArchive {
            archive,
            excludes: Vec::new(),
        }],
        &DownloadCancellationToken::new(),
    )
    .expect_err("archive link is rejected");

    assert_eq!(error.code(), "native_archive_invalid");
    assert!(!game.join("versions/fixture-1.0/natives").exists());
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn native_extraction_recovers_stale_staging_and_honors_cancellation() {
    let game = temporary_game("native-recovery");
    let version_root = game.join("versions/fixture-1.0");
    let stale = version_root.join("natives.installing-stale");
    fs::create_dir_all(&stale).expect("stale staging");
    fs::write(stale.join("partial.dll"), b"partial").expect("partial");
    let archive = game.join("libraries/native.jar");
    fs::create_dir_all(archive.parent().expect("library parent")).expect("parent");
    fs::write(&archive, zip_archive(&[("good.dll", b"good")])).expect("archive");
    let token = DownloadCancellationToken::new();
    token.cancel();

    let error = extract_natives_transactional(
        &game,
        "fixture-1.0",
        &[NativeArchive {
            archive,
            excludes: Vec::new(),
        }],
        &token,
    )
    .expect_err("cancelled extraction stops");

    assert_eq!(error.code(), "download_cancelled");
    assert!(!stale.exists());
    assert!(!version_root.join("natives").exists());
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn registry_rejects_duplicate_version_and_cancellation_is_idempotent() {
    let registry = OperationRegistry::default();
    let first = registry.begin("fixture-1.0").expect("first operation");
    let duplicate = registry
        .begin("fixture-1.0")
        .expect_err("duplicate blocked");
    assert_eq!(duplicate.code(), "installation_in_progress");

    registry
        .cancel(&first.operation_id)
        .expect("first cancellation");
    registry
        .cancel(&first.operation_id)
        .expect("second cancellation");
    assert!(first.cancel_token.is_cancelled());
    assert_eq!(
        registry.status(&first.operation_id).expect("status").state,
        OperationState::Cancelling
    );

    registry
        .finish(&first.operation_id, OperationState::Cancelled, None, None)
        .expect("finish");
    assert!(registry.begin("fixture-1.0").is_ok());
}

struct FixtureVersions(ResolvedVersion);

#[async_trait]
impl VersionProvider for FixtureVersions {
    async fn resolved_version(&self, _id: &str) -> Result<ResolvedVersion, LauncherError> {
        Ok(self.0.clone())
    }
}

struct FailingDownloads;

#[async_trait]
impl VerifiedDownloader for FailingDownloads {
    async fn execute(
        &self,
        _operation_id: String,
        _specs: Vec<DownloadSpec>,
        _cancel: DownloadCancellationToken,
        _progress: Arc<dyn ProgressSink>,
    ) -> Result<(), LauncherError> {
        Err(LauncherError::new(
            "download_http_status",
            "retry exhausted",
            None,
            true,
        ))
    }
}

#[derive(Default)]
struct RecordingInstallations(Mutex<Vec<(String, String)>>);

#[async_trait]
impl InstallationStore for RecordingInstallations {
    async fn set_state(&self, version_id: &str, state: &str) -> Result<(), LauncherError> {
        self.0
            .lock()
            .expect("states")
            .push((version_id.to_owned(), state.to_owned()));
        Ok(())
    }
}

#[test]
fn download_failure_propagates_and_never_persists_verified_state() {
    tauri::async_runtime::block_on(async {
        let game = temporary_game("install-failure");
        let store = Arc::new(RecordingInstallations::default());
        let installer = Installer::with_dependencies(
            game.clone(),
            Arc::new(FixtureVersions(resolved_fixture())),
            Arc::new(FailingDownloads),
            store.clone(),
        )
        .expect("installer");

        let error = installer
            .install(
                "operation-one".to_owned(),
                "fixture-1.0".to_owned(),
                DownloadCancellationToken::new(),
            )
            .await
            .expect_err("download failure propagates");

        assert_eq!(error.code(), "download_http_status");
        let states = store.0.lock().expect("states").clone();
        assert_eq!(
            states.first().map(|entry| entry.1.as_str()),
            Some("installing")
        );
        assert_eq!(states.last().map(|entry| entry.1.as_str()), Some("failed"));
        assert!(states.iter().all(|entry| entry.1 != "verified"));
        fs::remove_dir_all(game).expect("cleanup");
    });
}

#[test]
fn pre_cancelled_install_records_cancelled_without_marking_installed() {
    tauri::async_runtime::block_on(async {
        let game = temporary_game("install-cancelled");
        let store = Arc::new(RecordingInstallations::default());
        let installer = Installer::with_dependencies(
            game.clone(),
            Arc::new(FixtureVersions(resolved_fixture())),
            Arc::new(FailingDownloads),
            store.clone(),
        )
        .expect("installer");
        let token = DownloadCancellationToken::new();
        token.cancel();

        let error = installer
            .install("cancelled".to_owned(), "fixture-1.0".to_owned(), token)
            .await
            .expect_err("cancelled install stops");

        assert_eq!(error.code(), "download_cancelled");
        let states = store.0.lock().expect("states").clone();
        assert_eq!(
            states.last().map(|entry| entry.1.as_str()),
            Some("cancelled")
        );
        assert!(states.iter().all(|entry| entry.1 != "verified"));
        fs::remove_dir_all(game).expect("cleanup");
    });
}

#[test]
fn local_http_install_verifies_downloads_extracts_natives_and_only_then_marks_verified() {
    tauri::async_runtime::block_on(async {
        let body = b"verified-client".to_vec();
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server");
        let address = listener.local_addr().expect("server address");
        let served = body.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("request bytes");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                served.len()
            )
            .expect("response headers");
            stream.write_all(&served).expect("response body");
        });
        let version: VersionJson = serde_json::from_value(serde_json::json!({
            "id": "local-1.0",
            "downloads": {"client": {
                "url": format!("http://{address}/client.jar"),
                "size": body.len(),
                "sha1": format!("{:x}", Sha1::digest(&body))
            }},
            "minecraftArguments": "--username ${auth_player_name}",
            "arguments": {"game": ["--demo", {"rules":[{"action":"allow","os":{"name":"windows"}}],"value":["--width","854"]}]}
        })).expect("legacy and modern metadata");
        let game = temporary_game("local-http-install");
        let store = Arc::new(RecordingInstallations::default());
        let installer = Installer::with_dependencies(
            game.clone(),
            Arc::new(FixtureVersions(version.into())),
            Arc::new(DownloadService::new(game.clone()).expect("download service")),
            store.clone(),
        )
        .expect("installer");

        let summary = installer
            .install(
                "local-http-operation".to_owned(),
                "local-1.0".to_owned(),
                DownloadCancellationToken::new(),
            )
            .await
            .expect("installation succeeds");

        server.join().expect("server thread");
        assert_eq!(summary.version_id, "local-1.0");
        let installed_version: serde_json::Value = serde_json::from_slice(
            &fs::read(game.join("versions/local-1.0/local-1.0.json"))
                .expect("installed version JSON"),
        )
        .expect("installed version JSON parses");
        assert_eq!(
            installed_version["minecraftArguments"],
            "--username ${auth_player_name}"
        );
        assert_eq!(installed_version["arguments"]["game"][0], "--demo");
        assert_eq!(
            installed_version["arguments"]["game"][1]["value"],
            serde_json::json!(["--width", "854"])
        );
        assert_eq!(
            fs::read(game.join("versions/local-1.0/local-1.0.jar")).expect("client"),
            body
        );
        assert!(summary.natives_directory.is_dir());
        let states = store.0.lock().expect("states").clone();
        assert_eq!(
            states,
            vec![
                ("local-1.0".to_owned(), "installing".to_owned()),
                ("local-1.0".to_owned(), "verified".to_owned())
            ]
        );
        fs::remove_dir_all(game).expect("cleanup");
    });
}

#[test]
fn legacy_library_without_download_metadata_is_sized_then_downloaded_by_verified_queue() {
    tauri::async_runtime::block_on(async {
        let body = b"legacy-library".to_vec();
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server");
        let address = listener.local_addr().expect("server address");
        let served = body.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("request");
                let mut request = [0_u8; 2048];
                let read = stream.read(&mut request).expect("request bytes");
                let is_head = request[..read].starts_with(b"HEAD ");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    served.len()
                )
                .expect("response headers");
                if !is_head {
                    stream.write_all(&served).expect("response body");
                }
            }
        });
        let version: VersionJson = serde_json::from_value(serde_json::json!({
            "id": "legacy-1.0",
            "minecraftArguments": "--username ${auth_player_name}",
            "libraries": [{"name":"org.example:legacy:1.0", "url": format!("http://{address}/")}]
        }))
        .expect("legacy metadata");
        let game = temporary_game("legacy-local-http");
        let store = Arc::new(RecordingInstallations::default());
        let installer = Installer::with_dependencies(
            game.clone(),
            Arc::new(FixtureVersions(version.into())),
            Arc::new(DownloadService::new(game.clone()).expect("download service")),
            store.clone(),
        )
        .expect("installer");

        installer
            .install(
                "legacy-operation".to_owned(),
                "legacy-1.0".to_owned(),
                DownloadCancellationToken::new(),
            )
            .await
            .expect("legacy installation succeeds");

        server.join().expect("server thread");
        assert_eq!(
            fs::read(game.join("libraries/org/example/legacy/1.0/legacy-1.0.jar"))
                .expect("legacy library"),
            body
        );
        assert_eq!(
            store
                .0
                .lock()
                .expect("states")
                .last()
                .map(|entry| entry.1.as_str()),
            Some("verified")
        );
        fs::remove_dir_all(game).expect("cleanup");
    });
}

#[test]
fn unsafe_version_identifiers_never_become_filesystem_components() {
    let game = temporary_game("unsafe-version-id");
    for id in [".", "..", "reserved.PART", "trailing.", "../escape"] {
        let mut version = resolved_fixture();
        version.id = id.to_owned();
        assert_eq!(
            plan_installation(&game, &version)
                .expect_err("unsafe id")
                .code(),
            "metadata_invalid"
        );
    }
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn legacy_lwjgl_fixture_plans_base_and_windows_classifier_with_extract_rules() {
    let game = temporary_game("legacy-lwjgl-plan");
    let version: VersionJson = serde_json::from_str(include_str!(
        "../tests/fixtures/installer_legacy_lwjgl_version.json"
    ))
    .expect("old LWJGL fixture");

    let plan = plan_installation(&game, &version.into()).expect("legacy plan");
    let lwjgl_files = plan
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.kind,
                InstallFileKind::Library | InstallFileKind::Native
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(lwjgl_files.len(), 3);
    assert!(lwjgl_files.iter().any(|file| {
        file.kind == InstallFileKind::Library
            && file
                .destination
                .ends_with("org/lwjgl/lwjgl/lwjgl/2.9.3/lwjgl-2.9.3.jar")
    }));
    let native = lwjgl_files
        .iter()
        .find(|file| file.kind == InstallFileKind::Native)
        .expect("Windows native classifier");
    assert!(native.destination.ends_with(
        "org/lwjgl/lwjgl/lwjgl-platform/2.9.3/lwjgl-platform-2.9.3-natives-windows.jar"
    ));
    assert!(native.url.as_deref().is_some_and(|url| {
        url.ends_with(
            "org/lwjgl/lwjgl/lwjgl-platform/2.9.3/lwjgl-platform-2.9.3-natives-windows.jar",
        )
    }));
    assert_eq!(plan.natives.len(), 1);
    assert_eq!(plan.natives[0].excludes, vec!["META-INF/".to_owned()]);
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn shared_asset_hash_is_normalized_and_planned_once() {
    let game = temporary_game("shared-asset");
    let index = br#"{"objects":{"first":{"hash":"AABBCCDDEEFF00112233445566778899AABBCCDD","size":7},"second":{"hash":"aabbccddeeff00112233445566778899aabbccdd","size":7}}}"#;
    let index_path = game.join("assets/indexes/shared.json");
    fs::create_dir_all(index_path.parent().expect("asset index parent")).expect("parent");
    fs::write(&index_path, index).expect("asset index");
    let mut version = resolved_fixture();
    version.asset_index = Some(AssetIndex {
        id: "shared".to_owned(),
        url: "https://example.test/shared.json".to_owned(),
        sha1: Some(format!("{:x}", Sha1::digest(index))),
        size: Some(index.len() as u64),
        total_size: Some(14),
    });

    let plan = plan_installation(&game, &version).expect("shared objects plan");
    let objects = plan
        .files
        .iter()
        .filter(|file| file.kind == InstallFileKind::AssetObject)
        .collect::<Vec<_>>();

    assert_eq!(objects.len(), 1);
    assert!(objects[0]
        .destination
        .ends_with("assets/objects/aa/aabbccddeeff00112233445566778899aabbccdd"));
    assert_eq!(
        objects[0].sha1.as_deref(),
        Some("aabbccddeeff00112233445566778899aabbccdd")
    );
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn metadata_paths_reject_windows_aliases_ads_trailing_names_and_controls() {
    for unsafe_path in [
        "libraries/CON.jar",
        "libraries/aux",
        "libraries/COM1.dll",
        "libraries/lpt9.native",
        "libraries/file.jar:stream",
        "libraries/trailing.",
        "libraries/trailing ",
        "libraries/control\u{001f}.jar",
    ] {
        assert_eq!(
            validate_metadata_path(unsafe_path)
                .expect_err("Windows-unsafe path must be rejected")
                .code(),
            "metadata_invalid",
            "{unsafe_path:?}"
        );
    }
    assert_eq!(
        validate_metadata_path("libraries/safe/name-1.0.jar").expect("safe artifact path"),
        PathBuf::from("libraries/safe/name-1.0.jar")
    );
}

#[test]
fn asset_and_logging_ids_use_the_same_strict_windows_path_validation() {
    let game = temporary_game("unsafe-metadata-ids");
    let mut asset_version = resolved_fixture();
    asset_version.asset_index.as_mut().expect("asset index").id = "CON".to_owned();
    assert_eq!(
        plan_installation(&game, &asset_version)
            .expect_err("reserved asset index id")
            .code(),
        "metadata_invalid"
    );

    let mut logging_version = resolved_fixture();
    logging_version.logging = Some(serde_json::json!({
        "client": {"file": {"id":"aux.xml", "url":"https://example.test/log.xml", "size":1, "sha1":"0000000000000000000000000000000000000000"}}
    }));
    assert_eq!(
        plan_installation(&game, &logging_version)
            .expect_err("reserved logging id")
            .code(),
        "metadata_invalid"
    );
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn successful_native_activation_is_not_failed_by_backup_cleanup_error() {
    let game = temporary_game("native-cleanup-failure");
    let version_relative = PathBuf::from("versions/fixture-1.0");
    let destination = game.join(&version_relative).join("natives");
    let staging_relative = version_relative.join("natives.installing-test");
    fs::create_dir_all(&destination).expect("old natives");
    fs::write(destination.join("old.dll"), b"old").expect("old native");
    fs::create_dir_all(game.join(&staging_relative)).expect("new staging");
    fs::write(game.join(&staging_relative).join("new.dll"), b"new").expect("new native");

    let activated =
        activate_staging_with_cleanup(&game, &version_relative, &staging_relative, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "locked",
            ))
        })
        .expect("activation success is authoritative");

    assert_eq!(
        fs::read(activated.join("new.dll")).expect("new active native"),
        b"new"
    );
    assert!(fs::read_dir(game.join(&version_relative))
        .expect("version entries")
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("natives.backup-")));
    recover_interrupted(&game, &version_relative).expect("later recovery cleans stale backup");
    assert!(!fs::read_dir(game.join(&version_relative))
        .expect("version entries")
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("natives.backup-")));
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn native_archive_budget_bounds_entry_count_total_size_and_overflow() {
    assert_eq!(
        validate_native_budget(
            MAX_NATIVE_ENTRIES + 1,
            std::iter::repeat_n(0, MAX_NATIVE_ENTRIES + 1)
        )
        .expect_err("entry-count bomb")
        .code(),
        "native_archive_invalid"
    );
    assert_eq!(
        validate_native_budget(2, [MAX_TOTAL_NATIVE_BYTES, 1])
            .expect_err("aggregate zip bomb")
            .code(),
        "native_archive_invalid"
    );
    assert_eq!(
        validate_native_budget(2, [u64::MAX, 1])
            .expect_err("size overflow")
            .code(),
        "native_archive_invalid"
    );
    validate_native_budget(2, [1024, 2048]).expect("small archive budget");
}

#[test]
fn native_extractor_rejects_archive_entry_count_bomb_before_writes() {
    let game = temporary_game("native-entry-count");
    let archive = game.join("libraries/entry-count.jar");
    fs::create_dir_all(archive.parent().expect("library parent")).expect("parent");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..=MAX_NATIVE_ENTRIES {
        zip.start_file(format!("entry-{index}.dll"), SimpleFileOptions::default())
            .expect("zip entry");
    }
    fs::write(&archive, zip.finish().expect("zip closes").into_inner()).expect("archive");

    let error = extract_natives_transactional(
        &game,
        "fixture-1.0",
        &[NativeArchive {
            archive,
            excludes: Vec::new(),
        }],
        &DownloadCancellationToken::new(),
    )
    .expect_err("entry-count bomb is rejected");

    assert_eq!(error.code(), "native_archive_invalid");
    assert!(!game.join("versions/fixture-1.0/natives").exists());
    fs::remove_dir_all(game).expect("cleanup");
}

#[test]
fn terminal_operation_exposes_sanitized_stable_error_and_history_is_bounded() {
    let registry = OperationRegistry::default();
    let failed = registry.begin("failed-version").expect("failed operation");
    registry
        .finish(
            &failed.operation_id,
            OperationState::Failed,
            None,
            Some(LauncherError::internal("access_token=terminal-secret")),
        )
        .expect("terminal state");
    let status = registry
        .status(&failed.operation_id)
        .expect("failed status");
    let serialized = serde_json::to_string(&status).expect("status serializes");
    assert_eq!(
        status.error.as_ref().map(LauncherError::code),
        Some("internal_error")
    );
    assert!(!serialized.contains("terminal-secret"));
    assert!(serialized.contains("[REDACTED]"));

    for index in 0..MAX_TERMINAL_OPERATIONS {
        let handle = registry
            .begin(&format!("version-{index}"))
            .expect("operation");
        registry
            .finish(&handle.operation_id, OperationState::Completed, None, None)
            .expect("finish");
    }
    assert_eq!(
        registry
            .status(&failed.operation_id)
            .expect_err("oldest terminal record is pruned")
            .code(),
        "operation_not_found"
    );
}

#[derive(Default)]
struct RecordingProgress(Mutex<Vec<DownloadProgress>>);

impl ProgressSink for RecordingProgress {
    fn emit(&self, event: DownloadProgress) {
        self.0.lock().expect("progress").push(event);
    }
}

#[test]
fn phase_progress_offsets_asset_bytes_without_regressing_operation_progress() {
    let recorded = Arc::new(RecordingProgress::default());
    let base = PhaseProgressSink::new(recorded.clone(), 0, 10);
    base.emit(DownloadProgress {
        operation_id: "install-progress".to_owned(),
        total_bytes: 10,
        completed_bytes: 10,
        current_file: PathBuf::from("client.jar"),
    });
    let assets = PhaseProgressSink::new(recorded.clone(), 10, 15);
    assets.emit(DownloadProgress {
        operation_id: "install-progress".to_owned(),
        total_bytes: 5,
        completed_bytes: 1,
        current_file: PathBuf::from("asset"),
    });

    let events = recorded.0.lock().expect("progress");
    assert_eq!(
        events
            .iter()
            .map(|event| event.completed_bytes)
            .collect::<Vec<_>>(),
        vec![10, 11]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.total_bytes)
            .collect::<Vec<_>>(),
        vec![10, 15]
    );
}
