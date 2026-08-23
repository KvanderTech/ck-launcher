use super::{
    build_launch,
    process::{
        ChildProcess, EventSink, GameProcessEvent, ProcessLog, ProcessOutcome, ProcessSpawner,
    },
    LaunchAccount, LaunchBuildRequest, LaunchContextProvider, Launcher,
};
use crate::{
    metadata::models::{
        Argument, AssetIndex, Download, Library, LibraryDownloads, ResolvedVersion, Rule,
        VersionArguments, VersionDownloads,
    },
    runtime::{JavaRuntimeSource, JavaRuntimeState, JavaRuntimeStatus},
    storage::LauncherProfile,
};
use async_trait::async_trait;
use serde_json::json;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn temporary_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ck-launcher-task8-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("root");
    root
}

fn library(name: &str, path: &str, rules: Vec<Rule>) -> Library {
    Library {
        name: name.to_owned(),
        url: None,
        downloads: Some(LibraryDownloads {
            artifact: Some(Download {
                sha1: None,
                size: Some(1),
                url: "https://example.test/library.jar".to_owned(),
                path: Some(path.to_owned()),
            }),
            classifiers: BTreeMap::new(),
        }),
        rules,
        natives: None,
        extract: None,
    }
}

fn fixture_request(name: &str) -> LaunchBuildRequest {
    let root = temporary_root(name);
    let game = root.join("game");
    let java = root.join("runtime").join("bin").join("java.exe");
    let ordinary = game.join("libraries/org/example/ordinary/1.0/ordinary-1.0.jar");
    let client = game.join("versions/fixture/fixture.jar");
    let natives = game.join("versions/fixture/natives");
    let logging = game.join("assets/log_configs/log4j.xml");
    for parent in [
        java.parent().expect("java parent"),
        ordinary.parent().expect("library parent"),
        client.parent().expect("client parent"),
        &natives,
        logging.parent().expect("logging parent"),
    ] {
        fs::create_dir_all(parent).expect("fixture directory");
    }
    for file in [&java, &ordinary, &client, &logging] {
        fs::write(file, b"x").expect("fixture file");
    }
    LaunchBuildRequest {
        account: LaunchAccount::new("Player One", "0123456789abcdef", "access-secret"),
        version: ResolvedVersion {
            id: "fixture".to_owned(),
            main_class: Some("net.minecraft.client.main.Main".to_owned()),
            assets: Some("legacy-assets".to_owned()),
            asset_index: Some(AssetIndex {
                id: "fixture-assets".to_owned(),
                url: "https://example.test/assets.json".to_owned(),
                sha1: None,
                size: None,
                total_size: None,
            }),
            downloads: VersionDownloads::default(),
            libraries: vec![library(
                "org.example:ordinary:1.0",
                "org/example/ordinary/1.0/ordinary-1.0.jar",
                vec![],
            )],
            logging: Some(
                json!({"client":{"argument":"-Dlog4j.configurationFile=${path}","file":{"id":"log4j.xml"}}}),
            ),
            java_version: None,
            arguments: VersionArguments::default(),
            minecraft_arguments: None,
        },
        profile: LauncherProfile {
            id: "default".to_owned(),
            name: "Default".to_owned(),
            version_id: Some("fixture".to_owned()),
            memory_mb: 20_000,
            game_dir: game.to_string_lossy().into_owned(),
            java_override: None,
        },
        runtime: JavaRuntimeStatus {
            requirement: 21,
            state: JavaRuntimeState::Valid,
            path: Some(java),
            source: Some(JavaRuntimeSource::Managed),
            version: Some("21.0.8".to_owned()),
        },
        game_root: game,
        physical_memory_mb: 16_384,
    }
}

fn strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn modern_arguments_substitute_every_required_launch_value_and_owned_jvm_settings() {
    let mut request = fixture_request("modern");
    request.version.arguments.jvm = vec![
        Argument::Literal(
            "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"
                .to_owned(),
        ),
        Argument::Literal("-Xss1M".to_owned()),
        Argument::Literal("-Djava.library.path=${natives_directory}".to_owned()),
        Argument::Literal("-Djna.tmpdir=${natives_directory}".to_owned()),
        Argument::Literal(
            "-Dorg.lwjgl.system.SharedLibraryExtractPath=${natives_directory}".to_owned(),
        ),
        Argument::Literal("-Dio.netty.native.workdir=${natives_directory}".to_owned()),
        Argument::Literal("-Dminecraft.launcher.brand=${launcher_name}".to_owned()),
        Argument::Literal("-Dminecraft.launcher.version=${launcher_version}".to_owned()),
        Argument::Literal("-cp".to_owned()),
        Argument::Literal("${classpath}".to_owned()),
    ];
    request.version.arguments.game = vec![
        Argument::Literal("--username".to_owned()),
        Argument::Literal("${auth_player_name}".to_owned()),
        Argument::Literal("--uuid".to_owned()),
        Argument::Literal("${auth_uuid}".to_owned()),
        Argument::Literal("--accessToken".to_owned()),
        Argument::Literal("${auth_access_token}".to_owned()),
        Argument::Literal("--version".to_owned()),
        Argument::Literal("${version_name}".to_owned()),
        Argument::Literal("--gameDir=${game_directory}".to_owned()),
        Argument::Literal("--assetsDir=${assets_root}".to_owned()),
        Argument::Literal("--assetIndex=${assets_index_name}".to_owned()),
    ];

    let prepared = build_launch(request).expect("modern command builds");
    let args = strings(&prepared.command.args);
    assert_eq!(
        args[0],
        "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"
    );
    assert_eq!(args[1], "-Xss1M");
    assert!(args
        .iter()
        .any(|arg| arg.starts_with("-Djna.tmpdir=") && arg.ends_with("natives")));
    assert!(args.iter().any(|arg| {
        arg.starts_with("-Dorg.lwjgl.system.SharedLibraryExtractPath=") && arg.ends_with("natives")
    }));
    assert!(args
        .iter()
        .any(|arg| { arg.starts_with("-Dio.netty.native.workdir=") && arg.ends_with("natives") }));
    assert!(args.contains(&"-Dminecraft.launcher.brand=CKLauncher".to_owned()));
    assert!(args.contains(&format!(
        "-Dminecraft.launcher.version={}",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(args.contains(&"-Xms512M".to_owned()));
    assert!(args.contains(&"-Xmx12288M".to_owned()));
    assert_eq!(args.iter().filter(|arg| arg.as_str() == "-cp").count(), 1);
    assert!(args.contains(&"Player One".to_owned()));
    assert!(args.contains(&"0123456789abcdef".to_owned()));
    assert!(args.contains(&"access-secret".to_owned()));
    assert!(args.contains(&"fixture".to_owned()));
    assert!(args.iter().any(|arg| arg.ends_with("game")));
    assert!(args.iter().any(|arg| arg.ends_with("assets")));
    assert!(args.contains(&"--assetIndex=fixture-assets".to_owned()));
    assert!(args
        .iter()
        .any(|arg| arg.starts_with("-Djava.library.path=") && arg.ends_with("natives")));
    assert!(args
        .iter()
        .any(|arg| arg.starts_with("-Dlog4j.configurationFile=") && arg.ends_with("log4j.xml")));
    let main = args
        .iter()
        .position(|arg| arg == "net.minecraft.client.main.Main")
        .expect("main class");
    assert_eq!(args.get(main + 1).map(String::as_str), Some("--username"));
}

#[test]
fn legacy_minecraft_arguments_are_tokenized_before_safe_substitution() {
    let mut request = fixture_request("legacy");
    request.version.minecraft_arguments = Some(
        "--username ${auth_player_name} --uuid ${auth_uuid} --token ${auth_access_token} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --assetIndex ${assets_index_name} --natives ${natives_directory} --classpath ${classpath}".to_owned(),
    );

    let prepared = build_launch(request).expect("legacy command builds");
    let args = strings(&prepared.command.args);
    let main = args
        .iter()
        .position(|arg| arg == "net.minecraft.client.main.Main")
        .expect("main class");
    assert_eq!(
        &args[main + 1..main + 5],
        ["--username", "Player One", "--uuid", "0123456789abcdef"]
    );
    assert!(args[main + 1..].contains(&"access-secret".to_owned()));
    assert!(args[main + 1..].iter().any(|arg| arg.contains(';')));
}

#[test]
fn windows_x64_rules_exclude_disallowed_and_native_libraries_and_classpath_is_deduplicated() {
    let mut request = fixture_request("classpath");
    let allowed_path = request
        .game_root
        .join("libraries/org/example/allowed/1/allowed-1.jar");
    fs::create_dir_all(allowed_path.parent().expect("parent")).expect("dir");
    fs::write(&allowed_path, b"x").expect("file");
    let windows_rule = Rule {
        action: "allow".to_owned(),
        os: Some(crate::metadata::models::OsRule {
            name: Some("windows".to_owned()),
            arch: Some("amd64".to_owned()),
            version: None,
        }),
        features: None,
    };
    let linux_rule = Rule {
        action: "allow".to_owned(),
        os: Some(crate::metadata::models::OsRule {
            name: Some("linux".to_owned()),
            arch: None,
            version: None,
        }),
        features: None,
    };
    request.version.libraries = vec![
        library(
            "org.example:allowed:1",
            "org/example/allowed/1/allowed-1.jar",
            vec![windows_rule],
        ),
        library(
            "org.example:duplicate:1",
            "org/example/allowed/1/allowed-1.jar",
            vec![],
        ),
        library(
            "org.example:linux:1",
            "org/example/linux/1/linux-1.jar",
            vec![linux_rule],
        ),
        Library {
            name: "org.example:native:1".to_owned(),
            url: None,
            downloads: Some(LibraryDownloads {
                artifact: None,
                classifiers: BTreeMap::from([(
                    "natives-windows".to_owned(),
                    Download {
                        sha1: None,
                        size: Some(1),
                        url: "https://example.test/native.jar".to_owned(),
                        path: Some("org/example/native/1/native-1-natives-windows.jar".to_owned()),
                    },
                )]),
            }),
            rules: vec![],
            natives: Some(BTreeMap::from([(
                "windows".to_owned(),
                "natives-windows".to_owned(),
            )])),
            extract: None,
        },
    ];

    let prepared = build_launch(request).expect("classpath builds");
    let args = strings(&prepared.command.args);
    let cp = &args[args
        .iter()
        .position(|arg| arg == "-cp")
        .expect("classpath flag")
        + 1];
    let entries: Vec<_> = cp.split(';').collect();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| !entry.starts_with(r"\\?\")));
    assert!(entries[0].ends_with("allowed-1.jar"));
    assert!(entries[1].ends_with("fixture.jar"));
    assert!(!cp.contains("linux-1.jar"));
    assert!(!cp.contains("natives-windows"));
}

#[test]
fn windows_rule_jvm_fixture_reconstructs_exact_safe_os_properties() {
    let mut request = fixture_request("windows-os-properties");
    let windows_rule = Rule {
        action: "allow".to_owned(),
        os: Some(crate::metadata::models::OsRule {
            name: Some("windows".to_owned()),
            arch: Some("amd64".to_owned()),
            version: Some(r"^10\.".to_owned()),
        }),
        features: None,
    };
    let linux_rule = Rule {
        action: "allow".to_owned(),
        os: Some(crate::metadata::models::OsRule {
            name: Some("linux".to_owned()),
            arch: None,
            version: None,
        }),
        features: None,
    };
    request.version.arguments.jvm = vec![
        Argument::Conditional {
            rules: vec![windows_rule],
            value: json!(["-Dos.name=Windows 10", "-Dos.version=10.0"]),
        },
        Argument::Conditional {
            rules: vec![linux_rule],
            value: json!(["-Dos.name=Linux", "-Dos.version=6.0"]),
        },
        Argument::Literal("-cp".to_owned()),
        Argument::Literal("${classpath}".to_owned()),
    ];

    let prepared = build_launch(request).expect("official Windows JVM fixture builds");
    let args = strings(&prepared.command.args);
    assert_eq!(
        args.iter()
            .filter(|argument| argument.as_str() == "-Dos.name=Windows 10")
            .count(),
        1
    );
    assert_eq!(
        args.iter()
            .filter(|argument| argument.as_str() == "-Dos.version=10.0")
            .count(),
        1
    );
    assert!(!args.iter().any(|argument| argument.contains("=Linux")));
}

#[test]
fn unknown_required_placeholders_fail_without_reaching_a_process_command() {
    let mut request = fixture_request("placeholder");
    request.version.arguments.game =
        vec![Argument::Literal("${future_required_secret}".to_owned())];
    let error = build_launch(request).expect_err("unknown placeholder is rejected");
    assert_eq!(error.code(), "launch_argument_invalid");
}

#[test]
fn metadata_cannot_override_memory_classpath_natives_or_security_arguments() {
    for malicious in [
        "-Xmx65536M",
        "-Xms1M",
        "-jar",
        "-m",
        "--module",
        "--module=evil.module/EvilMain",
        "@evil.args",
        "@@nested.args",
        "-javaagent=C:\\evil.jar",
        "-agentlib:jdwp=transport=dt_socket,server=y",
        "-Djava.library.path=C:\\evil",
        "-Djavax.net.ssl.trustStore=C:\\evil",
        "-Djava.security.manager=allow",
        "-Dos.name=Windows 11",
        "-Dos.version=11.0",
    ] {
        let mut request = fixture_request("malicious");
        request.version.arguments.jvm = vec![Argument::Literal(malicious.to_owned())];
        let error = build_launch(request).expect_err("owned or security argument is rejected");
        assert_eq!(error.code(), "unsafe_launch_argument", "{malicious}");
    }
}

#[test]
fn metadata_jvm_rejects_source_file_mode_and_every_unexpected_operand() {
    let malicious_sequences = [
        vec![
            "--source",
            "21",
            r"${game_directory}\libraries\evil\Evil.java",
        ],
        vec!["--source=21", r"${game_directory}\libraries\evil\Evil.java"],
        vec![r"${game_directory}\libraries\evil\Evil.java"],
        vec!["Evil.java"],
        vec!["evil/Evil.java"],
        vec!["net.evil.Evil"],
        vec!["21"],
        vec!["unexpected-operand"],
        vec!["-Dlauncher.test=true"],
        vec!["-cp", "@evil.args"],
        vec!["--class-path", "-jar"],
        vec!["-classpath", "net.evil.Evil"],
    ];

    for malicious in malicious_sequences {
        let mut request = fixture_request("malicious-source-mode");
        request.version.arguments.jvm = malicious
            .iter()
            .map(|value| Argument::Literal((*value).to_owned()))
            .collect();
        let error = build_launch(request).expect_err("unexpected JVM input is rejected");
        assert_eq!(error.code(), "unsafe_launch_argument", "{malicious:?}");
    }
}

#[test]
fn main_class_must_be_a_strict_qualified_java_class_name() {
    for malicious in [
        "-jar",
        "@evil.args",
        "9Main",
        "net..Main",
        ".net.minecraft.Main",
        "net.minecraft.Main.",
        "net.minecraft.Main/evil",
        "net.minecraft.Main;Evil",
    ] {
        let mut request = fixture_request("malicious-main-class");
        request.version.main_class = Some(malicious.to_owned());
        let error = build_launch(request).expect_err("invalid main class is rejected");
        assert_eq!(error.code(), "launch_argument_invalid", "{malicious}");
    }
}

#[test]
fn logging_argument_accepts_only_the_exact_mojang_local_path_template() {
    for malicious in [
        r"-Dlog4j.configurationFile=C:\outside.xml",
        "-Dlog4j.configurationFile=https://evil.test/${path}",
        "-Dlog4j.configurationFile=${path}${path}",
        "-Dlog4j.configurationFile=${path}\n-Djava.security.manager=allow",
        "-Dlog4j2.configurationFile=${path}",
    ] {
        let mut request = fixture_request("malicious-logging-template");
        request.version.logging = Some(json!({
            "client": {
                "argument": malicious,
                "file": {"id": "log4j.xml"}
            }
        }));
        let error = build_launch(request).expect_err("logging template is rejected");
        assert_eq!(error.code(), "launch_argument_invalid", "{malicious:?}");
    }
}

#[test]
fn logging_file_id_cannot_select_a_path_outside_the_verified_log_config_directory() {
    let mut request = fixture_request("malicious-logging-path");
    let outside = request.game_root.join("assets").join("outside.xml");
    fs::write(&outside, b"outside").expect("outside fixture");
    request.version.logging = Some(json!({
        "client": {
            "argument": "-Dlog4j.configurationFile=${path}",
            "file": {"id": "../outside.xml"}
        }
    }));
    let error = build_launch(request).expect_err("outside logging path is rejected");
    assert_eq!(error.code(), "launch_argument_invalid");
}

#[test]
fn metadata_classpath_pair_must_equal_the_backend_built_classpath() {
    let mut request = fixture_request("malicious-classpath");
    request.version.arguments.jvm = vec![
        Argument::Literal("-cp".to_owned()),
        Argument::Literal(r"C:\evil.jar;C:\other.jar".to_owned()),
    ];
    let error = build_launch(request).expect_err("metadata classpath injection is rejected");
    assert_eq!(error.code(), "unsafe_launch_argument");
}

#[test]
fn launch_command_debug_output_never_contains_access_tokens() {
    let prepared = build_launch(fixture_request("debug")).expect("command builds");
    let debug = format!("{:?}", prepared.command);
    assert!(!debug.contains("access-secret"));
    assert!(debug.contains("[REDACTED]"));
}

struct PreparedContext(Mutex<Option<super::PreparedLaunch>>);

#[async_trait]
impl LaunchContextProvider for PreparedContext {
    async fn prepare(
        &self,
        _profile_id: &str,
    ) -> Result<super::PreparedLaunch, crate::error::LauncherError> {
        self.0
            .lock()
            .expect("context lock")
            .take()
            .ok_or_else(|| crate::error::LauncherError::internal("context used twice"))
    }
}

struct MockChild {
    pid: u32,
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    post_exit_error: Option<crate::error::LauncherError>,
    release: Arc<tokio::sync::Semaphore>,
}

#[async_trait]
impl ChildProcess for MockChild {
    fn pid(&self) -> u32 {
        self.pid
    }

    async fn wait(
        self: Box<Self>,
        log: Arc<ProcessLog>,
    ) -> Result<ProcessOutcome, crate::error::LauncherError> {
        self.release.acquire().await.expect("release").forget();
        log.write_complete(&self.stdout)?;
        log.write_complete(&self.stderr)?;
        Ok(ProcessOutcome {
            exit_code: self.exit_code,
            auxiliary_error: self.post_exit_error,
        })
    }
}

struct MockSpawner {
    commands: Mutex<Vec<super::LaunchCommand>>,
    release: Arc<tokio::sync::Semaphore>,
    fail: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
    post_exit_error: Option<crate::error::LauncherError>,
}

#[async_trait]
impl ProcessSpawner for MockSpawner {
    async fn spawn(
        &self,
        command: super::LaunchCommand,
    ) -> Result<Box<dyn ChildProcess>, crate::error::LauncherError> {
        self.commands.lock().expect("commands lock").push(command);
        if self.fail {
            return Err(crate::error::LauncherError::new(
                "game_spawn_failed",
                "Minecraft could not be started.",
                None,
                true,
            ));
        }
        Ok(Box::new(MockChild {
            pid: 4242,
            exit_code: self.exit_code,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            post_exit_error: self.post_exit_error.clone(),
            release: self.release.clone(),
        }))
    }
}

#[derive(Default)]
struct RecordingEvents(Mutex<Vec<GameProcessEvent>>);

impl EventSink for RecordingEvents {
    fn emit(&self, event: GameProcessEvent) {
        self.0.lock().expect("events lock").push(event);
    }
}

#[test]
fn supervisor_spawns_the_exact_command_rejects_duplicates_and_emits_one_started_and_exit() {
    tauri::async_runtime::block_on(async {
        let request = fixture_request("supervisor");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let expected_executable = prepared.command.executable.clone();
        let expected_args = prepared.command.args.clone();
        let expected_cwd = prepared.command.cwd.clone();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let spawner = Arc::new(MockSpawner {
            commands: Mutex::new(Vec::new()),
            release: release.clone(),
            fail: false,
            stdout: b"game output".to_vec(),
            stderr: Vec::new(),
            exit_code: 7,
            post_exit_error: None,
        });
        let events = Arc::new(RecordingEvents::default());
        let launcher = Launcher::new(
            Arc::new(PreparedContext(Mutex::new(Some(prepared)))),
            spawner.clone(),
            events.clone(),
            logs,
        );

        let operation = launcher.launch("default").await.expect("launch starts");
        let duplicate = launcher
            .launch("default")
            .await
            .expect_err("duplicate rejected");
        assert_eq!(duplicate.code(), "game_already_running");
        {
            let commands = spawner.commands.lock().expect("commands lock");
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].executable, expected_executable);
            assert_eq!(commands[0].args, expected_args);
            assert_eq!(commands[0].cwd, expected_cwd);
        }
        release.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if launcher.status(&operation).expect("status").exit_code == Some(7) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("process exits");
        let recorded = events.0.lock().expect("events lock");
        assert_eq!(
            recorded
                .iter()
                .filter(|event| matches!(event, GameProcessEvent::Started { .. }))
                .count(),
            1
        );
        assert_eq!(
            recorded
                .iter()
                .filter(|event| matches!(event, GameProcessEvent::Exited { exit_code: 7, .. }))
                .count(),
            1
        );
        assert_eq!(launcher.active_count().expect("registry"), 0);
    });
}

#[test]
fn shared_latest_log_serializes_launches_across_different_profiles() {
    tauri::async_runtime::block_on(async {
        let request = fixture_request("global-serialization");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let spawner = Arc::new(MockSpawner {
            commands: Mutex::new(Vec::new()),
            release: release.clone(),
            fail: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 0,
            post_exit_error: None,
        });
        let launcher = Launcher::new(
            Arc::new(PreparedContext(Mutex::new(Some(prepared)))),
            spawner.clone(),
            Arc::new(RecordingEvents::default()),
            logs,
        );

        let operation = launcher
            .launch("first-profile")
            .await
            .expect("first launch");
        let error = launcher
            .launch("second-profile")
            .await
            .expect_err("shared log permits only one active launch");

        assert_eq!(error.code(), "game_already_running");
        assert_eq!(spawner.commands.lock().expect("commands").len(), 1);
        release.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if launcher.status(&operation).expect("status").exit_code == Some(0) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first process exits");
    });
}

#[test]
fn auxiliary_output_failure_preserves_exit_status_and_emits_exit_exactly_once() {
    tauri::async_runtime::block_on(async {
        let request = fixture_request("auxiliary-output-failure");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let spawner = Arc::new(MockSpawner {
            commands: Mutex::new(Vec::new()),
            release: Arc::new(tokio::sync::Semaphore::new(1)),
            fail: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 23,
            post_exit_error: Some(crate::error::LauncherError::new(
                "game_log_unavailable",
                "Game output could not be written to the launcher log.",
                Some("access_token=auxiliary-secret".to_owned()),
                true,
            )),
        });
        let events = Arc::new(RecordingEvents::default());
        let launcher = Launcher::new(
            Arc::new(PreparedContext(Mutex::new(Some(prepared)))),
            spawner,
            events.clone(),
            logs,
        );

        let operation = launcher.launch("default").await.expect("launch");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if launcher.status(&operation).expect("status").error.is_some() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("auxiliary error recorded");

        let status = launcher.status(&operation).expect("status");
        assert_eq!(status.exit_code, Some(23));
        assert_eq!(
            status.error.as_ref().map(|error| error.code()),
            Some("game_log_unavailable")
        );
        let recorded = events.0.lock().expect("events");
        assert_eq!(
            recorded
                .iter()
                .filter(|event| matches!(event, GameProcessEvent::Exited { exit_code: 23, .. }))
                .count(),
            1
        );
        assert_eq!(
            recorded
                .iter()
                .filter(|event| matches!(event, GameProcessEvent::Error { error, .. } if error.code() == "game_log_unavailable"))
                .count(),
            1
        );
        let serialized = serde_json::to_string(&*recorded).expect("events serialize");
        assert!(!serialized.contains("auxiliary-secret"));
    });
}

#[test]
fn logs_are_bounded_and_redact_tokens_even_when_the_child_prints_them() {
    tauri::async_runtime::block_on(async {
        let request = fixture_request("redaction");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let release = Arc::new(tokio::sync::Semaphore::new(1));
        let spawner = Arc::new(MockSpawner {
            commands: Mutex::new(Vec::new()),
            release,
            fail: false,
            stdout: [
                b"access-secret ".as_slice(),
                &vec![b'x'; super::process::MAX_LOG_BYTES + 1024],
            ]
            .concat(),
            stderr: b"Bearer access-secret".to_vec(),
            exit_code: 0,
            post_exit_error: None,
        });
        let events = Arc::new(RecordingEvents::default());
        let launcher = Launcher::new(
            Arc::new(PreparedContext(Mutex::new(Some(prepared)))),
            spawner,
            events,
            logs.clone(),
        );
        let operation = launcher.launch("default").await.expect("launch");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if launcher
                    .status(&operation)
                    .expect("status")
                    .exit_code
                    .is_some()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("exit");
        let bytes = fs::read(logs.join("latest.log")).expect("latest log");
        assert!(bytes.len() <= super::process::MAX_LOG_BYTES);
        assert!(!String::from_utf8_lossy(&bytes).contains("access-secret"));
        assert!(String::from_utf8_lossy(&bytes).contains("[REDACTED]"));
    });
}

#[test]
fn spawn_failure_emits_one_error_and_releases_the_profile_registry() {
    tauri::async_runtime::block_on(async {
        let request = fixture_request("spawn-failure");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let spawner = Arc::new(MockSpawner {
            commands: Mutex::new(Vec::new()),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
            fail: true,
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 0,
            post_exit_error: None,
        });
        let events = Arc::new(RecordingEvents::default());
        let launcher = Launcher::new(
            Arc::new(PreparedContext(Mutex::new(Some(prepared)))),
            spawner,
            events.clone(),
            logs,
        );
        let error = launcher.launch("default").await.expect_err("spawn fails");
        assert_eq!(error.code(), "game_spawn_failed");
        assert_eq!(launcher.active_count().expect("registry"), 0);
        assert_eq!(
            events
                .0
                .lock()
                .expect("events")
                .iter()
                .filter(|event| matches!(event, GameProcessEvent::Error { .. }))
                .count(),
            1
        );
    });
}

#[test]
fn orchestrated_prepared_spawn_failure_is_returned_without_a_second_terminal_event() {
    tauri::async_runtime::block_on(async {
        let request = fixture_request("prepared-spawn-failure");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let events = Arc::new(RecordingEvents::default());
        let launcher = Launcher::new(
            Arc::new(PreparedContext(Mutex::new(None))),
            Arc::new(MockSpawner {
                commands: Mutex::new(Vec::new()),
                release: Arc::new(tokio::sync::Semaphore::new(0)),
                fail: true,
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit_code: 0,
                post_exit_error: None,
            }),
            events.clone(),
            logs,
        );

        let error = launcher
            .launch_prepared("default", "workflow-operation", prepared)
            .await
            .expect_err("spawn fails");

        assert_eq!(error.code(), "game_spawn_failed");
        assert_eq!(launcher.active_count().expect("registry"), 0);
        assert!(events.0.lock().expect("events").is_empty());
    });
}

#[test]
fn terminal_process_history_is_bounded_and_expires_the_oldest_operation() {
    let registry = Arc::new(Mutex::new(super::ProcessRegistry::default()));
    for index in 0..=super::MAX_TERMINAL_PROCESSES {
        let operation_id = format!("launch-{index}");
        let profile_id = format!("profile-{index}");
        {
            let mut inner = registry.lock().expect("registry");
            inner.operations.insert(
                operation_id.clone(),
                super::GameProcessStatus {
                    operation_id: operation_id.clone(),
                    profile_id: profile_id.clone(),
                    pid: Some(index as u32),
                    exit_code: None,
                    error: None,
                },
            );
            inner
                .active_profiles
                .insert(profile_id.clone(), operation_id.clone());
        }
        super::finish_registry(&registry, &profile_id, &operation_id, Some(0), None)
            .expect("terminal record");
    }
    let inner = registry.lock().expect("registry");
    assert_eq!(inner.operations.len(), super::MAX_TERMINAL_PROCESSES);
    assert!(!inner.operations.contains_key("launch-0"));
    assert!(inner
        .operations
        .contains_key(&format!("launch-{}", super::MAX_TERMINAL_PROCESSES)));
}

#[test]
fn every_prepared_path_is_revalidated_for_reparse_points_immediately_before_spawn() {
    struct ReparsePointInspector {
        inspected: Mutex<Vec<PathBuf>>,
    }

    impl super::LaunchPathInspector for ReparsePointInspector {
        fn validate(&self, path: &std::path::Path) -> Result<(), crate::error::LauncherError> {
            self.inspected
                .lock()
                .expect("inspected paths")
                .push(path.to_path_buf());
            if path.to_string_lossy().ends_with("ordinary-1.0.jar") {
                return Err(super::invalid_launch_path());
            }
            Ok(())
        }
    }

    tauri::async_runtime::block_on(async {
        let request = fixture_request("reparse");
        let logs = request.game_root.parent().expect("root").join("logs");
        fs::create_dir_all(&logs).expect("logs");
        let prepared = build_launch(request).expect("prepared");
        let inspector = Arc::new(ReparsePointInspector {
            inspected: Mutex::new(Vec::new()),
        });
        let spawner = Arc::new(MockSpawner {
            commands: Mutex::new(Vec::new()),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
            fail: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 0,
            post_exit_error: None,
        });
        let launcher = Launcher::new_with_path_inspector(
            Arc::new(PreparedContext(Mutex::new(Some(prepared)))),
            spawner.clone(),
            Arc::new(RecordingEvents::default()),
            logs,
            inspector.clone(),
        );
        let error = launcher
            .launch("default")
            .await
            .expect_err("reparse rejected");
        assert_eq!(error.code(), "invalid_launch_path");
        assert!(spawner.commands.lock().expect("commands").is_empty());
        assert!(inspector
            .inspected
            .lock()
            .expect("inspected paths")
            .iter()
            .any(|path| path.to_string_lossy().ends_with("ordinary-1.0.jar")));
    });
}
