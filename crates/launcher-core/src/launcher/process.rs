use super::LaunchCommand;
use crate::{error::LauncherError, paths::AppPaths};
use async_trait::async_trait;
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAX_LOG_BYTES: usize = 1024 * 1024;
const ROTATED_LOGS: usize = 3;

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum GameProcessEvent {
    Started {
        operation_id: String,
        profile_id: String,
        pid: u32,
    },
    Exited {
        operation_id: String,
        profile_id: String,
        exit_code: i32,
    },
    Error {
        operation_id: String,
        profile_id: String,
        error: LauncherError,
        terminal: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_path: Option<PathBuf>,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: GameProcessEvent);
}

pub struct ProcessOutcome {
    pub exit_code: i32,
    pub auxiliary_error: Option<LauncherError>,
}

#[async_trait]
pub trait ChildProcess: Send {
    fn pid(&self) -> u32;
    async fn wait(self: Box<Self>, log: Arc<ProcessLog>) -> Result<ProcessOutcome, LauncherError>;
}

#[async_trait]
pub trait ProcessSpawner: Send + Sync {
    async fn spawn(&self, command: LaunchCommand) -> Result<Box<dyn ChildProcess>, LauncherError>;
}

pub struct TokioProcessSpawner;

#[async_trait]
impl ProcessSpawner for TokioProcessSpawner {
    async fn spawn(&self, command: LaunchCommand) -> Result<Box<dyn ChildProcess>, LauncherError> {
        let executable = background_java_executable(&command.executable);
        let mut process = tokio::process::Command::new(executable);
        process
            .args(&command.args)
            .current_dir(&command.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Ok(Box::new(TokioChild {
            child: process.spawn().map_err(|_| spawn_failed())?,
        }))
    }
}

#[cfg(windows)]
fn background_java_executable(executable: &Path) -> PathBuf {
    let javaw = executable.with_file_name("javaw.exe");
    if javaw.is_file() {
        javaw
    } else {
        executable.to_path_buf()
    }
}

#[cfg(not(windows))]
fn background_java_executable(executable: &Path) -> PathBuf {
    executable.to_path_buf()
}

struct TokioChild {
    child: tokio::process::Child,
}

#[async_trait]
impl ChildProcess for TokioChild {
    fn pid(&self) -> u32 {
        self.child.id().unwrap_or(0)
    }

    async fn wait(
        mut self: Box<Self>,
        log: Arc<ProcessLog>,
    ) -> Result<ProcessOutcome, LauncherError> {
        let stdout = self.child.stdout.take();
        let stderr = self.child.stderr.take();
        let stdout_task = stdout.map(|reader| {
            let log = log.clone();
            crate::tasks::spawn(async move { stream_output(reader, log).await })
        });
        let stderr_task = stderr.map(|reader| {
            let log = log.clone();
            crate::tasks::spawn(async move { stream_output(reader, log).await })
        });
        let status = self.child.wait().await.map_err(|_| wait_failed())?;
        let mut auxiliary_error = None;
        for task in [stdout_task, stderr_task].into_iter().flatten() {
            let error = match task.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(_) => Some(log_failed()),
            };
            if auxiliary_error.is_none() {
                auxiliary_error = error;
            }
        }
        Ok(ProcessOutcome {
            exit_code: status.code().unwrap_or(-1),
            auxiliary_error,
        })
    }
}

async fn stream_output(
    mut reader: impl AsyncRead + Unpin,
    log: Arc<ProcessLog>,
) -> Result<(), LauncherError> {
    let mut writer = RedactingStream::new(log);
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await.map_err(|_| log_failed())?;
        if read == 0 {
            break;
        }
        writer.write(&buffer[..read])?;
    }
    writer.finish()
}

pub struct ProcessLog {
    inner: Mutex<ProcessLogInner>,
    secrets: Vec<String>,
}
struct ProcessLogInner {
    file: File,
    written: usize,
}

impl ProcessLog {
    pub fn open(root: &Path, secrets: Vec<String>) -> Result<Arc<Self>, LauncherError> {
        fs::create_dir_all(root).map_err(|_| log_failed())?;
        rotate_logs(root)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(safe_log_path(root, Path::new("latest.log"))?)
            .map_err(|_| log_failed())?;
        Ok(Arc::new(Self {
            inner: Mutex::new(ProcessLogInner { file, written: 0 }),
            secrets: secrets
                .into_iter()
                .filter(|secret| !secret.is_empty())
                .collect(),
        }))
    }

    pub fn write_complete(&self, bytes: &[u8]) -> Result<(), LauncherError> {
        self.append_sanitized(&sanitize_text(
            &String::from_utf8_lossy(bytes),
            &self.secrets,
        ))
    }

    fn append_sanitized(&self, text: &str) -> Result<(), LauncherError> {
        let mut inner = self.inner.lock().map_err(|_| log_failed())?;
        // Keep a live tail rather than silently freezing the console after the first MiB.
        let mut start = text.len().saturating_sub(MAX_LOG_BYTES);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        let text = &text[start..];
        if text.is_empty() {
            return Ok(());
        }
        if inner.written.saturating_add(text.len()) > MAX_LOG_BYTES {
            // Compact in half-buffer batches, not on every output chunk once full.
            let keep = (MAX_LOG_BYTES / 2)
                .min(MAX_LOG_BYTES - text.len())
                .min(inner.written);
            let offset = inner.written - keep;
            inner
                .file
                .seek(SeekFrom::Start(offset as u64))
                .map_err(|_| log_failed())?;
            let mut tail = vec![0; keep];
            inner.file.read_exact(&mut tail).map_err(|_| log_failed())?;
            let boundary = tail
                .iter()
                .position(|byte| byte & 0xc0 != 0x80)
                .unwrap_or(tail.len());
            let tail = &tail[boundary..];
            inner
                .file
                .seek(SeekFrom::Start(0))
                .map_err(|_| log_failed())?;
            inner.file.set_len(0).map_err(|_| log_failed())?;
            inner.file.write_all(tail).map_err(|_| log_failed())?;
            inner.written = tail.len();
        }
        inner
            .file
            .write_all(text.as_bytes())
            .map_err(|_| log_failed())?;
        inner.file.flush().map_err(|_| log_failed())?;
        inner.written += text.len();
        Ok(())
    }
}

struct RedactingStream {
    log: Arc<ProcessLog>,
    pending: Vec<u8>,
    retain: usize,
}
impl RedactingStream {
    fn new(log: Arc<ProcessLog>) -> Self {
        let retain = log
            .secrets
            .iter()
            .map(String::len)
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        Self {
            log,
            pending: Vec::new(),
            retain,
        }
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), LauncherError> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() <= self.retain {
            return Ok(());
        }
        let mut split = self.pending.len() - self.retain;
        for secret in &self.log.secrets {
            let needle = secret.as_bytes();
            if needle.is_empty() || needle.len() > self.pending.len() {
                continue;
            }
            for start in 0..=self.pending.len() - needle.len() {
                let end = start + needle.len();
                if start < split && end > split && self.pending[start..end] == *needle {
                    split = start;
                }
            }
        }
        split = complete_utf8_prefix(&self.pending[..split]);
        if split == 0 {
            return Ok(());
        }
        let complete = self.pending[..split].to_vec();
        self.pending.drain(..split);
        self.log.write_complete(&complete)
    }
    fn finish(self) -> Result<(), LauncherError> {
        self.log.write_complete(&self.pending)
    }
}

// A pipe read may end halfway through a Unicode scalar. Keep that suffix for the next
// read, while still allowing genuinely invalid bytes to be replaced by from_utf8_lossy.
fn complete_utf8_prefix(bytes: &[u8]) -> usize {
    let mut offset = 0;
    loop {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(_) => return bytes.len(),
            Err(error) => match error.error_len() {
                Some(length) => offset += error.valid_up_to() + length,
                None => return offset + error.valid_up_to(),
            },
        }
    }
}

fn sanitize_text(text: &str, secrets: &[String]) -> String {
    let mut sanitized = text.to_owned();
    for secret in secrets {
        sanitized = sanitized.replace(secret, "[REDACTED]");
    }
    crate::error::sanitize(&sanitized)
}

fn rotate_logs(root: &Path) -> Result<(), LauncherError> {
    let oldest = safe_log_path(root, &PathBuf::from(format!("latest.{ROTATED_LOGS}.log")))?;
    if oldest.exists() {
        fs::remove_file(oldest).map_err(|_| log_failed())?;
    }
    for index in (1..ROTATED_LOGS).rev() {
        let source = safe_log_path(root, &PathBuf::from(format!("latest.{index}.log")))?;
        if source.exists() {
            fs::rename(
                source,
                safe_log_path(root, &PathBuf::from(format!("latest.{}.log", index + 1)))?,
            )
            .map_err(|_| log_failed())?;
        }
    }
    let latest = safe_log_path(root, Path::new("latest.log"))?;
    if latest.exists() {
        fs::rename(latest, safe_log_path(root, Path::new("latest.1.log"))?)
            .map_err(|_| log_failed())?;
    }
    Ok(())
}

fn safe_log_path(root: &Path, relative: &Path) -> Result<PathBuf, LauncherError> {
    AppPaths::new(root.to_path_buf()).safe_join(root, relative)
}
fn spawn_failed() -> LauncherError {
    LauncherError::new(
        "game_spawn_failed",
        "Minecraft could not be started.",
        None,
        true,
    )
}
fn wait_failed() -> LauncherError {
    LauncherError::new(
        "game_wait_failed",
        "Minecraft process tracking failed.",
        None,
        true,
    )
}
fn log_failed() -> LauncherError {
    LauncherError::new(
        "game_log_unavailable",
        "The Minecraft log could not be written.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{GameProcessEvent, ProcessLog, RedactingStream, MAX_LOG_BYTES};
    use std::{fs, sync::Arc};

    #[test]
    fn process_events_use_the_camel_case_contract_expected_by_the_ui() {
        let event = GameProcessEvent::Started {
            operation_id: "operation".to_owned(),
            profile_id: "profile".to_owned(),
            pid: 42,
        };
        let value = serde_json::to_value(event).expect("serialize event");
        assert_eq!(value["operationId"], "operation");
        assert_eq!(value["profileId"], "profile");
        assert!(value.get("operation_id").is_none());
        assert!(value.get("profile_id").is_none());
    }

    #[test]
    fn stream_redaction_catches_a_token_split_across_reader_chunks() {
        let root = std::env::temp_dir().join(format!(
            "ck-launcher-log-split-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("root");
        let log = ProcessLog::open(&root, vec!["access-secret".to_owned()]).expect("log");
        let mut stream = RedactingStream::new(Arc::clone(&log));
        stream.write(b"prefix access-").expect("first chunk");
        stream.write(b"secret suffix").expect("second chunk");
        stream.finish().expect("finish");
        drop(log);
        let text = fs::read_to_string(root.join("latest.log")).expect("read");
        assert_eq!(text, "prefix [REDACTED] suffix");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn stream_preserves_unicode_at_every_byte_boundary_without_exposing_secrets() {
        let text = "Игра запущена 🎮 access-secret 日本語\n";
        for secrets in [Vec::new(), vec!["access-secret".to_owned()]] {
            for split in 0..=text.len() {
                let root = tempfile::tempdir().expect("root");
                let log = ProcessLog::open(root.path(), secrets.clone()).expect("log");
                let mut stream = RedactingStream::new(log.clone());
                stream
                    .write(&text.as_bytes()[..split])
                    .expect("first chunk");
                stream
                    .write(&text.as_bytes()[split..])
                    .expect("second chunk");
                stream.finish().expect("finish");
                drop(log);
                let expected = if secrets.is_empty() {
                    text.to_owned()
                } else {
                    text.replace("access-secret", "[REDACTED]")
                };
                assert_eq!(
                    fs::read_to_string(root.path().join("latest.log")).unwrap(),
                    expected,
                    "UTF-8 reader split at byte {split}"
                );
            }
        }
    }

    #[test]
    fn live_log_keeps_new_output_after_reaching_its_size_limit() {
        let root = tempfile::tempdir().expect("root");
        let log = ProcessLog::open(root.path(), vec!["access-secret".to_owned()]).expect("log");
        log.write_complete("Старые строки\n".repeat(MAX_LOG_BYTES / 10).as_bytes())
            .unwrap();
        log.write_complete("Игра полностью запущена 🎮 access-secret\n".as_bytes())
            .unwrap();
        drop(log);
        let text = fs::read_to_string(root.path().join("latest.log")).unwrap();
        assert!(text.len() <= MAX_LOG_BYTES);
        assert!(text.ends_with("Игра полностью запущена 🎮 [REDACTED]\n"));
        assert!(!text.contains("access-secret"));
        assert!(!text.contains('\u{fffd}'));
    }

    #[test]
    fn log_rotation_keeps_only_three_bounded_predecessors() {
        let root = std::env::temp_dir().join(format!(
            "ck-launcher-log-rotation-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("root");
        for index in 0..5 {
            let log = ProcessLog::open(&root, Vec::new()).expect("log");
            log.write_complete(format!("run-{index}").as_bytes())
                .expect("write");
            drop(log);
        }
        assert_eq!(
            fs::read_to_string(root.join("latest.log")).expect("latest"),
            "run-4"
        );
        assert_eq!(
            fs::read_to_string(root.join("latest.1.log")).expect("one"),
            "run-3"
        );
        assert_eq!(
            fs::read_to_string(root.join("latest.3.log")).expect("three"),
            "run-1"
        );
        assert!(!root.join("latest.4.log").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
