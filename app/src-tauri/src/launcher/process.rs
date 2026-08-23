use super::LaunchCommand;
use crate::{error::LauncherError, paths::AppPaths};
use async_trait::async_trait;
use regex::Regex;
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAX_LOG_BYTES: usize = 1024 * 1024;
const ROTATED_LOGS: usize = 3;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
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
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: GameProcessEvent);
}

#[async_trait]
pub trait ChildProcess: Send {
    fn pid(&self) -> u32;
    async fn wait(self: Box<Self>, log: Arc<ProcessLog>) -> Result<i32, LauncherError>;
}

#[async_trait]
pub trait ProcessSpawner: Send + Sync {
    async fn spawn(&self, command: LaunchCommand) -> Result<Box<dyn ChildProcess>, LauncherError>;
}

pub struct TokioProcessSpawner;

#[async_trait]
impl ProcessSpawner for TokioProcessSpawner {
    async fn spawn(&self, command: LaunchCommand) -> Result<Box<dyn ChildProcess>, LauncherError> {
        let mut process = tokio::process::Command::new(&command.executable);
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

struct TokioChild {
    child: tokio::process::Child,
}

#[async_trait]
impl ChildProcess for TokioChild {
    fn pid(&self) -> u32 {
        self.child.id().unwrap_or(0)
    }

    async fn wait(mut self: Box<Self>, log: Arc<ProcessLog>) -> Result<i32, LauncherError> {
        let stdout = self.child.stdout.take();
        let stderr = self.child.stderr.take();
        let stdout_task = stdout.map(|reader| {
            let log = log.clone();
            tauri::async_runtime::spawn(async move { stream_output(reader, log).await })
        });
        let stderr_task = stderr.map(|reader| {
            let log = log.clone();
            tauri::async_runtime::spawn(async move { stream_output(reader, log).await })
        });
        let status = self.child.wait().await.map_err(|_| wait_failed())?;
        for task in [stdout_task, stderr_task].into_iter().flatten() {
            task.await.map_err(|_| wait_failed())??;
        }
        Ok(status.code().unwrap_or(-1))
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
        let mut length = MAX_LOG_BYTES.saturating_sub(inner.written).min(text.len());
        while length > 0 && !text.is_char_boundary(length) {
            length -= 1;
        }
        if length == 0 {
            return Ok(());
        }
        inner
            .file
            .write_all(&text.as_bytes()[..length])
            .map_err(|_| log_failed())?;
        inner.file.flush().map_err(|_| log_failed())?;
        inner.written += length;
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

fn sanitize_text(text: &str, secrets: &[String]) -> String {
    let mut sanitized = text.to_owned();
    for secret in secrets {
        sanitized = sanitized.replace(secret, "[REDACTED]");
    }
    let bearer = Regex::new(r"(?i)\bBearer\s+[^\s,;]+")
        .expect("bearer pattern")
        .replace_all(&sanitized, "Bearer [REDACTED]");
    Regex::new(r"(?i)\b(access_token|refresh_token)(\s*[:=]\s*)[^\s,;&]+")
        .expect("token pattern")
        .replace_all(&bearer, "$1$2[REDACTED]")
        .into_owned()
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
    use super::{ProcessLog, RedactingStream};
    use std::{fs, sync::Arc};

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
