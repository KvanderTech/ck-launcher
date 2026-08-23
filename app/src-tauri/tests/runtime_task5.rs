use app_lib::runtime::{
    parse_java_major, JavaRequirement, JavaRuntimeSource, ProcessOutput, ProcessRunner,
    RuntimeManager,
};
use async_trait::async_trait;
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

struct FakeRunner {
    versions: HashMap<String, ProcessOutput>,
}

#[async_trait]
impl ProcessRunner for FakeRunner {
    async fn run(
        &self,
        executable: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<ProcessOutput, app_lib::error::LauncherError> {
        assert_eq!(args, ["-version"]);
        assert_eq!(timeout, Duration::from_secs(5));
        self.versions
            .get(&executable.to_string_lossy().into_owned())
            .cloned()
            .ok_or_else(|| app_lib::error::LauncherError::internal("unknown fake executable"))
    }
}

#[test]
fn parses_supported_java_version_output_from_stderr_or_stdout() {
    for (text, expected) in [
        ("java version \"1.8.0_431\"", 8),
        ("openjdk version \"17.0.12\"", 17),
        ("openjdk 21.0.4 2024-07-16", 21),
        ("openjdk version \"25\"", 25),
    ] {
        assert_eq!(parse_java_major(text), Some(expected));
    }
}

#[test]
fn resolution_prefers_matching_managed_then_manual_then_system_and_never_mismatch() {
    tauri::async_runtime::block_on(async {
        for major in [8, 17, 21, 25] {
            let root = std::env::temp_dir().join(format!(
                "ck-runtime-resolve-{major}-{}",
                rand::random::<u64>()
            ));
            std::fs::create_dir_all(&root).expect("runtime root");
            let managed = root
                .join(format!("java-{major}"))
                .join("bin")
                .join("java.exe");
            let manual = root.join(format!("manual-{major}.exe"));
            let system = root.join(format!("system-{major}.exe"));
            let mismatched = root.join(format!("system-wrong-{major}.exe"));
            std::fs::create_dir_all(managed.parent().unwrap()).expect("managed bin");
            for path in [&managed, &manual, &system, &mismatched] {
                std::fs::write(path, b"fake executable").expect("fake executable");
            }
            let version = if major == 8 {
                "java version \"1.8.0_431\"".to_owned()
            } else {
                format!("openjdk version \"{major}.0.1\"")
            };
            let wrong_major = if major == 25 { 21 } else { 25 };
            let versions = HashMap::from([
                (managed.to_string_lossy().into_owned(), output(&version)),
                (manual.to_string_lossy().into_owned(), output(&version)),
                (system.to_string_lossy().into_owned(), output(&version)),
                (
                    mismatched.to_string_lossy().into_owned(),
                    output(&format!("openjdk version \"{wrong_major}.0.1\"")),
                ),
            ]);
            let manager = RuntimeManager::new(
                root.clone(),
                Arc::new(FakeRunner { versions }),
                vec![mismatched, system.clone()],
            );

            let status = manager
                .resolve(JavaRequirement::new(major).unwrap(), Some(manual.clone()))
                .await
                .expect("managed resolves");
            assert_eq!(status.source, Some(JavaRuntimeSource::Managed));
            assert_eq!(status.path.as_deref(), Some(managed.as_path()));

            std::fs::remove_dir_all(root.join(format!("java-{major}"))).expect("managed removed");
            let status = manager
                .resolve(JavaRequirement::new(major).unwrap(), Some(manual.clone()))
                .await
                .expect("manual resolves");
            assert_eq!(status.source, Some(JavaRuntimeSource::Manual));
            assert_eq!(status.path.as_deref(), Some(manual.as_path()));

            let status = manager
                .resolve(JavaRequirement::new(major).unwrap(), None)
                .await
                .expect("system resolves");
            assert_eq!(status.source, Some(JavaRuntimeSource::System));
            assert_eq!(status.path.as_deref(), Some(system.as_path()));
            std::fs::remove_dir_all(root).expect("test root removed");
        }
    });
}

fn output(version: &str) -> ProcessOutput {
    ProcessOutput {
        success: true,
        stdout: String::new(),
        stderr: version.to_owned(),
    }
}
