use super::{
    assets::asset_object_path,
    libraries::{library_allowed, maven_artifact_path, WindowsRuleContext},
    natives::extract_natives_transactional,
    plan_installation, InstallFileKind, InstallationStore, Installer, NativeArchive,
    OperationRegistry, OperationState, VerifiedDownloader, VersionProvider,
};
use crate::metadata::models::{ResolvedVersion, VersionJson};
use crate::{
    downloads::{DownloadCancellationToken, DownloadService, DownloadSpec, ProgressSink},
    error::LauncherError,
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
        .finish(&first.operation_id, OperationState::Cancelled, None)
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
