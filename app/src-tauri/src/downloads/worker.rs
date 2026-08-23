use super::{
    plan::{build_plan, DownloadSpec, PlannedDownload},
    verify::{storage_error, validated_path, verify_file},
};
use crate::error::LauncherError;
use async_trait::async_trait;
use futures_util::{
    future::{select, Either},
    pin_mut, stream, StreamExt, TryStreamExt,
};
use reqwest::{header, Response, StatusCode};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};
use tokio::sync::Notify;

const MAX_CONCURRENT_DOWNLOADS: usize = 6;
const MAX_ATTEMPTS: usize = 3;
const INITIAL_BACKOFF: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
pub struct DownloadTimeouts {
    pub connect: Duration,
    pub request: Duration,
    pub idle: Duration,
}

impl Default for DownloadTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            request: Duration::from_secs(300),
            idle: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub operation_id: String,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub current_file: PathBuf,
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: DownloadProgress);
}

#[async_trait]
pub trait Sleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

pub trait Jitter: Send + Sync {
    fn duration(&self, upper_bound: Duration) -> Duration;
}

struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

struct RandomJitter;

impl Jitter for RandomJitter {
    fn duration(&self, upper_bound: Duration) -> Duration {
        if upper_bound.is_zero() {
            return Duration::ZERO;
        }
        let upper_millis = upper_bound.as_millis().min(u64::MAX as u128) as u64;
        Duration::from_millis(rand::random_range(0..=upper_millis))
    }
}

#[derive(Clone, Default)]
pub struct DownloadCancellationToken {
    inner: Arc<CancellationInner>,
}

#[derive(Default)]
struct CancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl DownloadCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::SeqCst) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

pub struct DownloadHttpClient {
    client: reqwest::Client,
    idle_timeout: Duration,
}

impl DownloadHttpClient {
    pub fn new(timeouts: DownloadTimeouts) -> Result<Self, LauncherError> {
        if timeouts.connect.is_zero() || timeouts.request.is_zero() || timeouts.idle.is_zero() {
            return Err(download_configuration_error());
        }
        let client = reqwest::Client::builder()
            .connect_timeout(timeouts.connect)
            .timeout(timeouts.request)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|_| download_configuration_error())?;
        Ok(Self {
            client,
            idle_timeout: timeouts.idle,
        })
    }

    pub async fn fetch_bytes_bounded(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, LauncherError> {
        let sleeper = TokioSleeper;
        let jitter = RandomJitter;
        for attempt in 0..MAX_ATTEMPTS {
            let token = DownloadCancellationToken::new();
            match self.fetch_bytes_once(url, max_bytes, &token).await {
                Ok(bytes) => return Ok(bytes),
                Err(failure) if failure.retryable && attempt + 1 < MAX_ATTEMPTS => {
                    sleeper.sleep(retry_delay(attempt, &jitter)).await;
                }
                Err(failure) => return Err(failure.error),
            }
        }
        Err(download_network_error())
    }

    async fn fetch_bytes_once(
        &self,
        url: &str,
        max_bytes: usize,
        cancel: &DownloadCancellationToken,
    ) -> Result<Vec<u8>, AttemptFailure> {
        let mut response = self.request(url, None, cancel).await?;
        if response.status() != StatusCode::OK {
            return Err(AttemptFailure::transient(download_resume_error()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(AttemptFailure::permanent(download_too_large_error()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = self.next_chunk(&mut response, cancel).await? {
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(AttemptFailure::permanent(download_too_large_error()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn request(
        &self,
        url: &str,
        offset: Option<u64>,
        cancel: &DownloadCancellationToken,
    ) -> Result<Response, AttemptFailure> {
        if cancel.is_cancelled() {
            return Err(AttemptFailure::cancelled());
        }
        let mut request = self.client.get(url);
        if let Some(offset) = offset {
            request = request.header(header::RANGE, format!("bytes={offset}-"));
        }
        let cancelled = cancel.cancelled();
        let request = request.send();
        pin_mut!(cancelled, request);
        let response = match select(cancelled, request).await {
            Either::Left(_) => return Err(AttemptFailure::cancelled()),
            Either::Right((response, _)) => response.map_err(classify_reqwest_error)?,
        };
        let status = response.status();
        if offset.is_some() && status == StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(response);
        }
        if !status.is_success() {
            return Err(AttemptFailure {
                error: LauncherError::new(
                    "download_http_status",
                    "The download server returned an unsuccessful response.",
                    Some(format!("HTTP status {}", status.as_u16())),
                    is_transient_status(status),
                ),
                retryable: is_transient_status(status),
            });
        }
        Ok(response)
    }

    async fn next_chunk(
        &self,
        response: &mut Response,
        cancel: &DownloadCancellationToken,
    ) -> Result<Option<Vec<u8>>, AttemptFailure> {
        let cancelled = cancel.cancelled();
        let chunk = tokio::time::timeout(self.idle_timeout, response.chunk());
        pin_mut!(cancelled, chunk);
        match select(cancelled, chunk).await {
            Either::Left(_) => Err(AttemptFailure::cancelled()),
            Either::Right((chunk, _)) => match chunk {
                Err(_) => Err(AttemptFailure::transient(download_timeout_error())),
                Ok(Err(error)) => Err(classify_reqwest_error(error)),
                Ok(Ok(chunk)) => Ok(chunk.map(|chunk| chunk.to_vec())),
            },
        }
    }
}

impl Default for DownloadHttpClient {
    fn default() -> Self {
        Self::new(DownloadTimeouts::default()).expect("default download client is valid")
    }
}

pub struct DownloadService {
    root: PathBuf,
    http: Arc<DownloadHttpClient>,
    sleeper: Arc<dyn Sleeper>,
    jitter: Arc<dyn Jitter>,
}

impl DownloadService {
    pub fn new(root: PathBuf) -> Result<Self, LauncherError> {
        Self::with_configuration(
            root,
            DownloadTimeouts::default(),
            Arc::new(TokioSleeper),
            Arc::new(RandomJitter),
        )
    }

    pub fn with_retry_dependencies(
        root: PathBuf,
        sleeper: Arc<dyn Sleeper>,
        jitter: Arc<dyn Jitter>,
    ) -> Result<Self, LauncherError> {
        Self::with_configuration(root, DownloadTimeouts::default(), sleeper, jitter)
    }

    pub fn with_configuration(
        root: PathBuf,
        timeouts: DownloadTimeouts,
        sleeper: Arc<dyn Sleeper>,
        jitter: Arc<dyn Jitter>,
    ) -> Result<Self, LauncherError> {
        let root = crate::paths::AppPaths::new(root.clone()).safe_join(&root, Path::new(""))?;
        Ok(Self {
            root,
            http: Arc::new(DownloadHttpClient::new(timeouts)?),
            sleeper,
            jitter,
        })
    }

    pub async fn execute(
        &self,
        operation_id: impl Into<String>,
        specs: Vec<DownloadSpec>,
        cancel: DownloadCancellationToken,
        progress_sink: Arc<dyn ProgressSink>,
    ) -> Result<(), LauncherError> {
        if cancel.is_cancelled() {
            return Err(download_cancelled_error());
        }
        let plan = build_plan(&self.root, specs)?;
        let progress = Arc::new(ProgressState {
            operation_id: operation_id.into(),
            total_bytes: plan.total_bytes,
            inner: StdMutex::new(ProgressInner {
                completed_bytes: plan.completed_bytes,
                sink: progress_sink,
            }),
            #[cfg(test)]
            before_emit: None,
        });

        stream::iter(plan.pending)
            .map(|download| self.execute_one(download, cancel.clone(), progress.clone()))
            .buffer_unordered(MAX_CONCURRENT_DOWNLOADS)
            .try_collect::<Vec<_>>()
            .await?;
        if cancel.is_cancelled() {
            return Err(download_cancelled_error());
        }
        Ok(())
    }

    async fn execute_one(
        &self,
        download: PlannedDownload,
        cancel: DownloadCancellationToken,
        progress: Arc<ProgressState>,
    ) -> Result<(), LauncherError> {
        if cancel.is_cancelled() {
            return Err(download_cancelled_error());
        }
        ensure_parent_directories(&self.root, &download.relative_destination)?;
        let _lock = PartLock::acquire(&self.root, &download.relative_lock)?;
        if verify_file(&self.root, &download.relative_destination, &download.spec)? {
            progress.set_file_bytes(&download.spec.destination, 0, download.spec.expected_size);
            return Ok(());
        }

        let mut reported = 0_u64;
        for attempt in 0..MAX_ATTEMPTS {
            match self
                .download_attempt(&download, &cancel, &progress, &mut reported)
                .await
            {
                Ok(()) => return Ok(()),
                Err(failure) if failure.retryable && attempt + 1 < MAX_ATTEMPTS => {
                    self.sleeper
                        .sleep(retry_delay(attempt, self.jitter.as_ref()))
                        .await;
                }
                Err(failure) => return Err(failure.error),
            }
        }
        Err(download_network_error())
    }

    async fn download_attempt(
        &self,
        download: &PlannedDownload,
        cancel: &DownloadCancellationToken,
        progress: &ProgressState,
        reported: &mut u64,
    ) -> Result<(), AttemptFailure> {
        if cancel.is_cancelled() {
            return Err(AttemptFailure::cancelled());
        }
        let part = validated_path(&self.root, &download.relative_part)
            .map_err(AttemptFailure::permanent)?;
        let mut offset = match fs::metadata(&part) {
            Ok(metadata) if metadata.file_type().is_file() => metadata.len(),
            Ok(_) => return Err(AttemptFailure::permanent(LauncherError::invalid_path())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(_) => return Err(AttemptFailure::permanent(storage_error())),
        };
        let hashless_partial =
            offset > 0 && download.spec.sha1.is_none() && download.spec.sha256.is_none();
        if hashless_partial || offset > download.spec.expected_size {
            remove_part(&self.root, &download.relative_part)?;
            offset = 0;
        } else if offset == download.spec.expected_size && offset > 0 {
            if verify_file(&self.root, &download.relative_part, &download.spec)
                .map_err(AttemptFailure::permanent)?
            {
                progress.set_file_bytes(&download.spec.destination, *reported, offset);
                *reported = offset;
                if cancel.is_cancelled() {
                    return Err(AttemptFailure::cancelled());
                }
                finalize_verified_part(&self.root, download).map_err(AttemptFailure::permanent)?;
                return Ok(());
            }
            remove_part(&self.root, &download.relative_part)?;
            offset = 0;
        }

        let mut file = open_part(&self.root, &download.relative_part, offset > 0)?;
        progress.set_file_bytes(&download.spec.destination, *reported, offset);
        *reported = offset;
        let requested_offset = (offset > 0).then_some(offset);
        let mut response = self
            .http
            .request(&download.spec.url, requested_offset, cancel)
            .await?;
        if offset > 0 {
            if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                drop(file);
                remove_part(&self.root, &download.relative_part)?;
                progress.set_file_bytes(&download.spec.destination, *reported, 0);
                *reported = 0;
                return Err(AttemptFailure::transient(download_resume_error()));
            } else if response.status() == StatusCode::PARTIAL_CONTENT {
                if !compatible_content_range(&response, offset, download.spec.expected_size) {
                    drop(file);
                    remove_part(&self.root, &download.relative_part)?;
                    progress.set_file_bytes(&download.spec.destination, *reported, 0);
                    *reported = 0;
                    return Err(AttemptFailure::transient(download_resume_error()));
                }
            } else if response.status() == StatusCode::OK {
                file = open_part(&self.root, &download.relative_part, false)?;
                offset = 0;
                progress.set_file_bytes(&download.spec.destination, *reported, 0);
                *reported = 0;
            } else {
                return Err(AttemptFailure::transient(download_resume_error()));
            }
        } else if response.status() != StatusCode::OK {
            return Err(AttemptFailure::transient(download_resume_error()));
        }
        if response.content_length().is_some_and(|length| {
            offset
                .checked_add(length)
                .is_none_or(|total| total > download.spec.expected_size)
        }) {
            drop(file);
            remove_part(&self.root, &download.relative_part)?;
            progress.set_file_bytes(&download.spec.destination, *reported, 0);
            *reported = 0;
            return Err(AttemptFailure::transient(download_integrity_error()));
        }

        let mut received = offset;
        while let Some(chunk) = self.http.next_chunk(&mut response, cancel).await? {
            let next = received
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| AttemptFailure::transient(download_integrity_error()))?;
            if next > download.spec.expected_size {
                drop(file);
                remove_part(&self.root, &download.relative_part)?;
                progress.set_file_bytes(&download.spec.destination, *reported, 0);
                *reported = 0;
                return Err(AttemptFailure::transient(download_integrity_error()));
            }
            file.write_all(&chunk)
                .map_err(|_| AttemptFailure::permanent(storage_error()))?;
            received = next;
            progress.set_file_bytes(&download.spec.destination, *reported, received);
            *reported = received;
        }
        file.sync_all()
            .map_err(|_| AttemptFailure::permanent(storage_error()))?;
        drop(file);

        if !verify_file(&self.root, &download.relative_part, &download.spec)
            .map_err(AttemptFailure::permanent)?
        {
            remove_part(&self.root, &download.relative_part)?;
            progress.set_file_bytes(&download.spec.destination, *reported, 0);
            *reported = 0;
            return Err(AttemptFailure::transient(download_integrity_error()));
        }
        if cancel.is_cancelled() {
            return Err(AttemptFailure::cancelled());
        }
        finalize_verified_part(&self.root, download).map_err(AttemptFailure::permanent)?;
        Ok(())
    }
}

pub(super) struct ProgressState {
    operation_id: String,
    total_bytes: u64,
    inner: StdMutex<ProgressInner>,
    #[cfg(test)]
    before_emit: Option<Arc<dyn Fn(u64) + Send + Sync>>,
}

struct ProgressInner {
    completed_bytes: u64,
    sink: Arc<dyn ProgressSink>,
}

impl ProgressState {
    #[cfg(test)]
    pub(super) fn for_concurrency_test(
        total_bytes: u64,
        sink: Arc<dyn ProgressSink>,
        before_emit: Arc<dyn Fn(u64) + Send + Sync>,
    ) -> Self {
        Self {
            operation_id: "progress-test".to_owned(),
            total_bytes,
            inner: StdMutex::new(ProgressInner {
                completed_bytes: 0,
                sink,
            }),
            before_emit: Some(before_emit),
        }
    }

    pub(super) fn set_file_bytes(&self, current_file: &Path, previous: u64, current: u64) {
        let mut inner = self.inner.lock().expect("progress state lock");
        let completed = if current >= previous {
            inner.completed_bytes.saturating_add(current - previous)
        } else {
            inner.completed_bytes.saturating_sub(previous - current)
        }
        .min(self.total_bytes);
        inner.completed_bytes = completed;
        #[cfg(test)]
        if let Some(before_emit) = &self.before_emit {
            before_emit(completed);
        }
        let event = DownloadProgress {
            operation_id: self.operation_id.clone(),
            total_bytes: self.total_bytes,
            completed_bytes: completed,
            current_file: current_file.to_path_buf(),
        };
        inner.sink.emit(event);
    }
}

struct PartLock {
    #[cfg(not(windows))]
    path: PathBuf,
    _file: File,
    #[cfg(windows)]
    overlapped: Box<windows_sys::Win32::System::IO::OVERLAPPED>,
}

// SAFETY: the Windows OVERLAPPED is boxed, so moving PartLock between executor threads does not
// change the address passed to LockFileEx/UnlockFileEx. The owned file handle and box remain live
// together until Drop, and no operation accesses them concurrently.
#[cfg(windows)]
unsafe impl Send for PartLock {}

impl PartLock {
    #[cfg(windows)]
    fn acquire(root: &Path, relative: &Path) -> Result<Self, LauncherError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };

        let path = validated_path(root, relative)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|_| storage_error())?;
        let mut overlapped = Box::new(windows_sys::Win32::System::IO::OVERLAPPED::default());
        // SAFETY: the file handle and boxed OVERLAPPED remain alive in PartLock until Drop.
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut *overlapped,
            )
        };
        if locked == 0 {
            return Err(download_in_progress_error());
        }
        Ok(Self {
            _file: file,
            overlapped,
        })
    }

    #[cfg(not(windows))]
    fn acquire(root: &Path, relative: &Path) -> Result<Self, LauncherError> {
        let path = validated_path(root, relative)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    download_in_progress_error()
                } else {
                    storage_error()
                }
            })?;
        Ok(Self { path, _file: file })
    }
}

impl Drop for PartLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;

            // SAFETY: this releases the exact byte range locked with the same live file handle
            // and OVERLAPPED value in acquire. The persistent empty marker is intentionally kept
            // so process termination cannot strand an ownership record that blocks future resume.
            unsafe {
                UnlockFileEx(
                    self._file.as_raw_handle(),
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut *self.overlapped,
                );
            }
        }
        #[cfg(not(windows))]
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn ensure_parent_directories(root: &Path, relative_file: &Path) -> Result<(), LauncherError> {
    let Some(parent) = relative_file.parent() else {
        return Ok(());
    };
    let mut relative = PathBuf::new();
    for component in parent.components() {
        let Component::Normal(component) = component else {
            return Err(LauncherError::invalid_path());
        };
        relative.push(component);
        let path = validated_path(root, &relative)?;
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(storage_error()),
        }
        let revalidated = validated_path(root, &relative)?;
        if !revalidated.is_dir() {
            return Err(LauncherError::invalid_path());
        }
    }
    Ok(())
}

fn open_part(root: &Path, relative: &Path, append: bool) -> Result<File, AttemptFailure> {
    let path = validated_path(root, relative).map_err(AttemptFailure::permanent)?;
    OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|_| AttemptFailure::permanent(storage_error()))
}

fn remove_part(root: &Path, relative: &Path) -> Result<(), AttemptFailure> {
    let path = validated_path(root, relative).map_err(AttemptFailure::permanent)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AttemptFailure::permanent(storage_error())),
    }
}

fn finalize_verified_part(root: &Path, download: &PlannedDownload) -> Result<(), LauncherError> {
    if verify_file(root, &download.relative_destination, &download.spec)? {
        let part = validated_path(root, &download.relative_part)?;
        fs::remove_file(part).map_err(|_| storage_error())?;
        return Ok(());
    }
    let destination = validated_path(root, &download.relative_destination)?;
    if !verify_file(root, &download.relative_part, &download.spec)? {
        return Err(download_integrity_error());
    }

    let had_previous = destination.exists();
    let backup_relative = if had_previous {
        let destination = validated_path(root, &download.relative_destination)?;
        let metadata = destination.metadata().map_err(|_| storage_error())?;
        if !metadata.file_type().is_file() {
            return Err(LauncherError::invalid_path());
        }
        let backup = unique_backup_path(&download.relative_destination);
        let backup_path = validated_path(root, &backup)?;
        let destination = validated_path(root, &download.relative_destination)?;
        fs::rename(destination, backup_path).map_err(|_| storage_error())?;
        Some(backup)
    } else {
        None
    };

    let part = validated_path(root, &download.relative_part)?;
    let destination = validated_path(root, &download.relative_destination)?;
    if let Err(error) = fs::rename(&part, &destination) {
        if let Some(backup) = &backup_relative {
            let backup = validated_path(root, backup)?;
            let destination = validated_path(root, &download.relative_destination)?;
            if fs::rename(backup, destination).is_err() {
                return Err(LauncherError::new(
                    "download_state_inconsistent",
                    "The previous file could not be restored consistently.",
                    None,
                    true,
                ));
            }
        }
        return Err(LauncherError::new(
            "download_replace_failed",
            "The verified download could not replace its destination.",
            Some(error.to_string()),
            true,
        ));
    }
    if !verify_file(root, &download.relative_destination, &download.spec)? {
        if let Some(backup) = &backup_relative {
            let destination = validated_path(root, &download.relative_destination)?;
            fs::remove_file(destination).map_err(|_| storage_error())?;
            let backup = validated_path(root, backup)?;
            let destination = validated_path(root, &download.relative_destination)?;
            fs::rename(backup, destination).map_err(|_| {
                LauncherError::new(
                    "download_state_inconsistent",
                    "The previous file could not be restored consistently.",
                    None,
                    true,
                )
            })?;
        }
        return Err(download_integrity_error());
    }
    if let Some(backup) = backup_relative {
        let backup = validated_path(root, &backup)?;
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn unique_backup_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .expect("validated destination has a file name")
        .to_os_string();
    name.push(format!(".replace-{}.bak", rand::random::<u64>()));
    destination.with_file_name(name)
}

fn compatible_content_range(response: &Response, offset: u64, expected_total: u64) -> bool {
    let Some(value) = response
        .headers()
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(value) = value.strip_prefix("bytes ") else {
        return false;
    };
    let Some((range, total)) = value.split_once('/') else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    start.parse::<u64>().ok() == Some(offset)
        && total.parse::<u64>().ok() == Some(expected_total)
        && end
            .parse::<u64>()
            .is_ok_and(|end| end >= offset && end < expected_total)
}

struct AttemptFailure {
    error: LauncherError,
    retryable: bool,
}

impl AttemptFailure {
    fn transient(error: LauncherError) -> Self {
        Self {
            error,
            retryable: true,
        }
    }

    fn permanent(error: LauncherError) -> Self {
        Self {
            error,
            retryable: false,
        }
    }

    fn cancelled() -> Self {
        Self::permanent(download_cancelled_error())
    }
}

fn classify_reqwest_error(error: reqwest::Error) -> AttemptFailure {
    if error.is_timeout() {
        AttemptFailure::transient(download_timeout_error())
    } else if error.is_connect() || error.is_request() || error.is_body() {
        AttemptFailure::transient(download_network_error())
    } else {
        AttemptFailure::permanent(download_network_error())
    }
}

fn is_transient_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_EARLY
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn retry_delay(attempt: usize, jitter: &dyn Jitter) -> Duration {
    let multiplier = 1_u32 << attempt;
    let base = INITIAL_BACKOFF.saturating_mul(multiplier);
    base.saturating_add(jitter.duration(base))
}

fn download_configuration_error() -> LauncherError {
    LauncherError::new(
        "download_configuration_invalid",
        "The download service could not be configured.",
        None,
        false,
    )
}

fn download_network_error() -> LauncherError {
    LauncherError::new(
        "download_network_failed",
        "The file could not be downloaded.",
        None,
        true,
    )
}

fn download_timeout_error() -> LauncherError {
    LauncherError::new("download_timeout", "The download timed out.", None, true)
}

fn download_integrity_error() -> LauncherError {
    LauncherError::new(
        "download_integrity_failed",
        "The downloaded file failed its integrity check.",
        None,
        true,
    )
}

fn download_resume_error() -> LauncherError {
    LauncherError::new(
        "download_resume_incompatible",
        "The server could not safely resume the partial download.",
        None,
        true,
    )
}

fn download_cancelled_error() -> LauncherError {
    LauncherError::new(
        "download_cancelled",
        "The download was cancelled.",
        None,
        true,
    )
}

fn download_too_large_error() -> LauncherError {
    LauncherError::new(
        "download_too_large",
        "The downloaded file exceeded its allowed size.",
        None,
        false,
    )
}

fn download_in_progress_error() -> LauncherError {
    LauncherError::new(
        "download_in_progress",
        "This file is already being downloaded.",
        None,
        true,
    )
}
