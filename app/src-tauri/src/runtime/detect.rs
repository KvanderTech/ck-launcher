use crate::error::LauncherError;
use async_trait::async_trait;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

pub const JAVA_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn run(
        &self,
        executable: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<ProcessOutput, LauncherError>;
}

pub struct TokioProcessRunner;

#[async_trait]
impl ProcessRunner for TokioProcessRunner {
    async fn run(
        &self,
        executable: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<ProcessOutput, LauncherError> {
        let child = Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| runtime_invalid())?;
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| runtime_invalid())?
            .map_err(|_| runtime_invalid())?;

        Ok(ProcessOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub async fn probe_java(
    runner: &dyn ProcessRunner,
    executable: &Path,
) -> Result<(u16, String), LauncherError> {
    let output = runner
        .run(executable, &["-version"], JAVA_PROBE_TIMEOUT)
        .await?;
    if !output.success {
        return Err(runtime_invalid());
    }
    let combined = format!("{}\n{}", output.stderr, output.stdout);
    let major = parse_java_major(&combined).ok_or_else(runtime_invalid)?;
    let version = java_version_text(&combined).unwrap_or_else(|| major.to_string());
    Ok((major, version))
}

pub fn parse_java_major(output: &str) -> Option<u16> {
    let capture =
        regex::Regex::new(r#"(?i)(?:java|openjdk)(?:\s+version)?\s+\"?(\d+)(?:\.(\d+))?"#)
            .expect("Java version regex is valid")
            .captures(output)?;
    let first = capture.get(1)?.as_str().parse::<u16>().ok()?;
    if first == 1 {
        capture.get(2)?.as_str().parse().ok()
    } else {
        Some(first)
    }
}

fn java_version_text(output: &str) -> Option<String> {
    let capture = regex::Regex::new(r#"(?i)(?:java|openjdk)(?:\s+version)?\s+\"?([^\"\s]+)"#)
        .expect("Java version text regex is valid")
        .captures(output)?;
    Some(capture.get(1)?.as_str().to_owned())
}

fn runtime_invalid() -> LauncherError {
    LauncherError::new(
        "java_runtime_invalid",
        "The selected Java runtime is missing, incompatible, or could not be started.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{probe_java, ProcessOutput, ProcessRunner};
    use crate::error::LauncherError;
    use async_trait::async_trait;
    use std::{path::Path, time::Duration};

    enum Outcome {
        Output(ProcessOutput),
        Timeout,
    }
    struct FakeRunner(Outcome);
    #[async_trait]
    impl ProcessRunner for FakeRunner {
        async fn run(
            &self,
            _executable: &Path,
            args: &[&str],
            timeout: Duration,
        ) -> Result<ProcessOutput, LauncherError> {
            assert_eq!(args, ["-version"]);
            assert_eq!(timeout, Duration::from_secs(5));
            match &self.0 {
                Outcome::Output(output) => Ok(output.clone()),
                Outcome::Timeout => Err(LauncherError::new(
                    "java_runtime_invalid",
                    "timeout",
                    None,
                    true,
                )),
            }
        }
    }

    #[test]
    fn nonzero_exit_and_timeout_are_invalid_even_when_output_mentions_a_supported_version() {
        tauri::async_runtime::block_on(async {
            let nonzero = FakeRunner(Outcome::Output(ProcessOutput {
                success: false,
                stdout: "openjdk version \"21\"".to_owned(),
                stderr: String::new(),
            }));
            assert_eq!(
                probe_java(&nonzero, Path::new("java.exe"))
                    .await
                    .unwrap_err()
                    .code(),
                "java_runtime_invalid"
            );
            let timeout = FakeRunner(Outcome::Timeout);
            assert_eq!(
                probe_java(&timeout, Path::new("java.exe"))
                    .await
                    .unwrap_err()
                    .code(),
                "java_runtime_invalid"
            );
        });
    }
}
