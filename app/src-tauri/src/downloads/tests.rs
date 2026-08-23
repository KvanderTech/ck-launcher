use super::{
    DownloadCancellationToken, DownloadHttpClient, DownloadProgress, DownloadService, DownloadSpec,
    DownloadTimeouts, Jitter, ProgressSink, Sleeper,
};
use async_trait::async_trait;
use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[derive(Clone)]
struct TestResponse {
    status: u16,
    headers: Vec<(String, String)>,
    chunks: Vec<(Vec<u8>, Duration)>,
}

impl TestResponse {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            chunks: vec![(body, Duration::ZERO)],
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            chunks: vec![(Vec::new(), Duration::ZERO)],
        }
    }
}

struct TestServer {
    url: String,
    requests: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start(handler: impl Fn(usize, &str) -> TestResponse + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("local server binds");
        listener
            .set_nonblocking(true)
            .expect("listener becomes nonblocking");
        let address = listener.local_addr().expect("local server address");
        let requests = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let thread = {
            let requests = requests.clone();
            let max_active = max_active.clone();
            let active = active.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let request_number = requests.fetch_add(1, Ordering::SeqCst) + 1;
                            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                            max_active.fetch_max(now, Ordering::SeqCst);
                            let handler = handler.clone();
                            let active = active.clone();
                            thread::spawn(move || {
                                serve_connection(stream, request_number, handler.as_ref());
                                active.fetch_sub(1, Ordering::SeqCst);
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            url: format!("http://{address}"),
            requests,
            max_active,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_connection(
    mut stream: TcpStream,
    request_number: usize,
    handler: &(dyn Fn(usize, &str) -> TestResponse + Send + Sync),
) {
    stream
        .set_nonblocking(false)
        .expect("accepted stream becomes blocking");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("test stream timeout");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request);
    let response = handler(request_number, &request);
    let reason = match response.status {
        200 => "OK",
        206 => "Partial Content",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Test Status",
    };
    let length: usize = response.chunks.iter().map(|(chunk, _)| chunk.len()).sum();
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status, reason, length
    );
    for (name, value) in response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    for (chunk, delay) in response.chunks {
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        if stream.write_all(&chunk).is_err() {
            return;
        }
        let _ = stream.flush();
    }
}

fn temporary_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ck-downloads-{name}-{}", rand::random::<u64>()));
    fs::create_dir_all(&root).expect("temporary root is created");
    root
}

fn spec(url: String, destination: PathBuf, bytes: &[u8]) -> DownloadSpec {
    DownloadSpec {
        url,
        destination,
        expected_size: bytes.len() as u64,
        sha1: Some(format!("{:x}", Sha1::digest(bytes))),
        sha256: Some(format!("{:x}", Sha256::digest(bytes))),
    }
}

fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

#[derive(Default)]
struct RecordingProgress {
    events: Mutex<Vec<DownloadProgress>>,
    inside: AtomicBool,
    overlapped: AtomicBool,
    expected_part: Mutex<Option<PathBuf>>,
    saw_part: AtomicBool,
}

impl ProgressSink for RecordingProgress {
    fn emit(&self, event: DownloadProgress) {
        if self.inside.swap(true, Ordering::SeqCst) {
            self.overlapped.store(true, Ordering::SeqCst);
        }
        if self
            .expected_part
            .lock()
            .expect("part path lock")
            .as_ref()
            .is_some_and(|path| path.is_file())
        {
            self.saw_part.store(true, Ordering::SeqCst);
        }
        thread::sleep(Duration::from_millis(1));
        self.events.lock().expect("events lock").push(event);
        self.inside.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct RecordingSleeper(Mutex<Vec<Duration>>);

#[async_trait]
impl Sleeper for RecordingSleeper {
    async fn sleep(&self, duration: Duration) {
        self.0.lock().expect("sleep lock").push(duration);
    }
}

struct FixedJitter(Duration);

impl Jitter for FixedJitter {
    fn duration(&self, _upper_bound: Duration) -> Duration {
        self.0
    }
}

#[test]
fn verified_download_is_atomic_and_second_execution_makes_no_request() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x5a; 1024];
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse::ok(body.clone()));
        let root = temporary_root("atomic");
        let destination = root.join("versions/client.jar");
        let progress = Arc::new(RecordingProgress::default());
        *progress.expected_part.lock().unwrap() = Some(part_path(&destination));
        let service = DownloadService::new(root.clone()).expect("download service");
        let download = spec(
            format!("{}/client", server.url),
            destination.clone(),
            &bytes,
        );

        service
            .execute(
                "install-1",
                vec![download.clone()],
                DownloadCancellationToken::new(),
                progress.clone(),
            )
            .await
            .expect("first download succeeds");
        service
            .execute(
                "install-2",
                vec![download],
                DownloadCancellationToken::new(),
                progress.clone(),
            )
            .await
            .expect("verified file is skipped");

        assert_eq!(server.requests(), 1);
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        assert!(!part_path(&destination).exists());
        assert!(progress.saw_part.load(Ordering::SeqCst));
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn corrupt_response_is_retried_once_and_only_verified_bytes_are_renamed() {
    tauri::async_runtime::block_on(async {
        let correct = vec![0x71; 1024];
        let expected = correct.clone();
        let server = TestServer::start(move |request, _| {
            if request == 1 {
                TestResponse::ok(vec![0x13; 1024])
            } else {
                TestResponse::ok(expected.clone())
            }
        });
        let root = temporary_root("retry-corrupt");
        let destination = root.join("client.jar");
        let sleeper = Arc::new(RecordingSleeper::default());
        let service = DownloadService::with_retry_dependencies(
            root.clone(),
            sleeper,
            Arc::new(FixedJitter(Duration::ZERO)),
        )
        .expect("download service");

        service
            .execute(
                "install",
                vec![spec(
                    format!("{}/corrupt", server.url),
                    destination.clone(),
                    &correct,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect("retry succeeds");

        assert_eq!(server.requests(), 2);
        assert_eq!(fs::read(&destination).unwrap(), correct);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn correct_files_are_skipped_and_wrong_size_only_files_are_replaced() {
    tauri::async_runtime::block_on(async {
        let bytes = b"replacement".to_vec();
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse::ok(body.clone()));
        let root = temporary_root("replace");
        let correct = root.join("correct.bin");
        let wrong = root.join("wrong.bin");
        fs::write(&correct, &bytes).unwrap();
        fs::write(&wrong, b"bad").unwrap();
        let no_hash = |destination| DownloadSpec {
            url: format!("{}/file", server.url),
            destination,
            expected_size: bytes.len() as u64,
            sha1: None,
            sha256: None,
        };
        let service = DownloadService::new(root.clone()).unwrap();

        service
            .execute(
                "install",
                vec![no_hash(correct.clone()), no_hash(wrong.clone())],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();

        assert_eq!(server.requests(), 1);
        assert_eq!(fs::read(correct).unwrap(), bytes);
        assert_eq!(fs::read(wrong).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn worker_pool_never_exceeds_six_http_requests_and_serializes_progress() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x2d; 128];
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse {
            status: 200,
            headers: Vec::new(),
            chunks: vec![(body.clone(), Duration::from_millis(80))],
        });
        let root = temporary_root("concurrency");
        let specs = (0..12)
            .map(|index| {
                spec(
                    format!("{}/file/{index}", server.url),
                    root.join(format!("{index}.bin")),
                    &bytes,
                )
            })
            .collect();
        let progress = Arc::new(RecordingProgress::default());

        DownloadService::new(root.clone())
            .unwrap()
            .execute(
                "parallel",
                specs,
                DownloadCancellationToken::new(),
                progress.clone(),
            )
            .await
            .unwrap();

        assert_eq!(server.requests(), 12);
        assert_eq!(server.max_active.load(Ordering::SeqCst), 6);
        assert!(!progress.overlapped.load(Ordering::SeqCst));
        for event in progress.events.lock().unwrap().iter() {
            assert_eq!(event.operation_id, "parallel");
            assert!(event.completed_bytes <= event.total_bytes);
            assert!(event.current_file.starts_with(&root));
        }
        assert!(progress
            .events
            .lock()
            .unwrap()
            .windows(2)
            .all(|events| events[0].completed_bytes <= events[1].completed_bytes));
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn progress_counter_mutation_and_emission_are_one_serialized_operation() {
    use super::worker::ProgressState;
    use std::sync::Condvar;

    #[derive(Default)]
    struct Values(Mutex<Vec<u64>>);
    impl ProgressSink for Values {
        fn emit(&self, event: DownloadProgress) {
            self.0.lock().unwrap().push(event.completed_bytes);
        }
    }

    struct Gate {
        state: Mutex<(bool, bool)>,
        changed: Condvar,
    }
    let gate = Arc::new(Gate {
        state: Mutex::new((false, false)),
        changed: Condvar::new(),
    });
    let hook_gate = gate.clone();
    let hook = Arc::new(move |completed| {
        if completed == 1 {
            let mut state = hook_gate.state.lock().unwrap();
            state.0 = true;
            hook_gate.changed.notify_all();
            while !state.1 {
                state = hook_gate.changed.wait(state).unwrap();
            }
        }
    });
    let values = Arc::new(Values::default());
    let progress = Arc::new(ProgressState::for_concurrency_test(2, values.clone(), hook));
    let first_progress = progress.clone();
    let first = thread::spawn(move || {
        first_progress.set_file_bytes(Path::new("first"), 0, 1);
    });
    {
        let mut state = gate.state.lock().unwrap();
        while !state.0 {
            state = gate.changed.wait(state).unwrap();
        }
    }
    let second_progress = progress.clone();
    let second = thread::spawn(move || {
        second_progress.set_file_bytes(Path::new("second"), 0, 1);
    });
    for _ in 0..50 {
        if !values.0.lock().unwrap().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    {
        let mut state = gate.state.lock().unwrap();
        state.1 = true;
        gate.changed.notify_all();
    }
    first.join().unwrap();
    second.join().unwrap();

    let values = values.0.lock().unwrap();
    assert_eq!(values.len(), 2);
    assert!(values.windows(2).all(|values| values[0] <= values[1]));
    assert!(values.iter().all(|value| *value <= 2));
}

#[test]
fn retries_transient_status_three_attempts_with_deterministic_backoff_but_not_404() {
    tauri::async_runtime::block_on(async {
        let bytes = b"eventual".to_vec();
        let body = bytes.clone();
        let transient = TestServer::start(move |request, _| {
            if request < 3 {
                TestResponse::status(503)
            } else {
                TestResponse::ok(body.clone())
            }
        });
        let root = temporary_root("backoff");
        let sleeper = Arc::new(RecordingSleeper::default());
        let service = DownloadService::with_retry_dependencies(
            root.clone(),
            sleeper.clone(),
            Arc::new(FixedJitter(Duration::from_millis(7))),
        )
        .unwrap();
        service
            .execute(
                "retry",
                vec![spec(
                    format!("{}/eventual", transient.url),
                    root.join("eventual.bin"),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        assert_eq!(transient.requests(), 3);
        assert_eq!(
            *sleeper.0.lock().unwrap(),
            [Duration::from_millis(107), Duration::from_millis(207)]
        );

        let permanent = TestServer::start(|_, _| TestResponse::status(404));
        let error = service
            .execute(
                "permanent",
                vec![spec(
                    format!("{}/missing?access_token=secret", permanent.url),
                    root.join("missing.bin"),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect_err("404 is permanent");
        assert_eq!(permanent.requests(), 1);
        assert_eq!(error.code(), "download_http_status");
        assert!(!error.details().unwrap_or_default().contains("secret"));
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn cancellation_leaves_partial_data_and_never_creates_a_final_file() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x44; 1024];
        let first = bytes[..512].to_vec();
        let second = bytes[512..].to_vec();
        let server = TestServer::start(move |_, _| TestResponse {
            status: 200,
            headers: Vec::new(),
            chunks: vec![
                (first.clone(), Duration::ZERO),
                (second.clone(), Duration::from_millis(300)),
            ],
        });
        let root = temporary_root("cancel");
        let destination = root.join("large.bin");
        let token = DownloadCancellationToken::new();
        let cancel = token.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            cancel.cancel();
        });

        let error = DownloadService::new(root.clone())
            .unwrap()
            .execute(
                "cancel",
                vec![spec(
                    format!("{}/slow", server.url),
                    destination.clone(),
                    &bytes,
                )],
                token,
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect_err("cancellation interrupts the body");

        assert_eq!(error.code(), "download_cancelled");
        assert!(!destination.exists());
        assert_eq!(fs::metadata(part_path(&destination)).unwrap().len(), 512);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn cancellation_stops_the_queue_before_scheduling_waiting_files() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x48; 64];
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse {
            status: 200,
            headers: Vec::new(),
            chunks: vec![(body.clone(), Duration::from_millis(300))],
        });
        let root = temporary_root("cancel-scheduling");
        let specs = (0..12)
            .map(|index| {
                spec(
                    format!("{}/file/{index}", server.url),
                    root.join(format!("{index}.bin")),
                    &bytes,
                )
            })
            .collect();
        let token = DownloadCancellationToken::new();
        let cancel = token.clone();
        let requests = server.requests.clone();
        thread::spawn(move || {
            while requests.load(Ordering::SeqCst) < 6 {
                thread::sleep(Duration::from_millis(2));
            }
            cancel.cancel();
        });

        let error = DownloadService::new(root.clone())
            .unwrap()
            .execute(
                "cancel-queue",
                specs,
                token,
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect_err("cancellation stops the active queue");

        assert_eq!(error.code(), "download_cancelled");
        assert!(server.requests() <= 6);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn compatible_range_resumes_and_incompatible_range_restarts_from_zero() {
    tauri::async_runtime::block_on(async {
        let bytes = (0..=255).cycle().take(1024).collect::<Vec<_>>();
        let resumed_body = bytes.clone();
        let saw_range = Arc::new(AtomicBool::new(false));
        let saw_range_server = saw_range.clone();
        let compatible = TestServer::start(move |_, request| {
            saw_range_server.store(request.contains("range: bytes=400-"), Ordering::SeqCst);
            TestResponse {
                status: 206,
                headers: vec![("Content-Range".to_owned(), "bytes 400-1023/1024".to_owned())],
                chunks: vec![(resumed_body[400..].to_vec(), Duration::ZERO)],
            }
        });
        let root = temporary_root("resume");
        let resumed = root.join("resumed.bin");
        fs::write(part_path(&resumed), &bytes[..400]).unwrap();
        let service = DownloadService::new(root.clone()).unwrap();
        service
            .execute(
                "resume",
                vec![spec(
                    format!("{}/range", compatible.url),
                    resumed.clone(),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        assert!(saw_range.load(Ordering::SeqCst));
        assert_eq!(fs::read(resumed).unwrap(), bytes);

        let full = bytes.clone();
        let incompatible = TestServer::start(move |_, request| {
            assert!(request.contains("range: bytes=300-"));
            TestResponse::ok(full.clone())
        });
        let restarted = root.join("restarted.bin");
        fs::write(part_path(&restarted), &bytes[..300]).unwrap();
        service
            .execute(
                "restart",
                vec![spec(
                    format!("{}/no-range", incompatible.url),
                    restarted.clone(),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        assert_eq!(incompatible.requests(), 1);
        assert_eq!(fs::read(restarted).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn hashless_download_discards_a_mixed_version_prefix_before_requesting() {
    tauri::async_runtime::block_on(async {
        let fresh = b"BBBBBBBB".to_vec();
        let body = fresh.clone();
        let server = TestServer::start(move |_, request| {
            if request.contains("range: bytes=4-") {
                TestResponse {
                    status: 206,
                    headers: vec![("Content-Range".to_owned(), "bytes 4-7/8".to_owned())],
                    chunks: vec![(body[4..].to_vec(), Duration::ZERO)],
                }
            } else {
                TestResponse::ok(body.clone())
            }
        });
        let root = temporary_root("hashless-prefix");
        let destination = root.join("file.bin");
        fs::write(part_path(&destination), b"AAAA").unwrap();
        let download = DownloadSpec {
            url: format!("{}/file", server.url),
            destination: destination.clone(),
            expected_size: fresh.len() as u64,
            sha1: None,
            sha256: None,
        };

        DownloadService::new(root.clone())
            .unwrap()
            .execute(
                "hashless",
                vec![download],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();

        assert_eq!(server.requests(), 1);
        assert_eq!(fs::read(destination).unwrap(), fresh);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn planner_rejects_final_part_lock_and_case_insensitive_aliases() {
    let root = temporary_root("plan-aliases").canonicalize().unwrap();
    for (first, second) in [
        ("a", "a.part"),
        ("a", "a.part.lock"),
        ("A.PART", "a.part"),
        ("File.bin", "file.bin"),
    ] {
        let specs = [first, second]
            .into_iter()
            .map(|name| DownloadSpec {
                url: "https://example.test/file".to_owned(),
                destination: root.join(name),
                expected_size: 1,
                sha1: None,
                sha256: None,
            })
            .collect();

        let error = match super::plan::build_plan(&root, specs) {
            Ok(_) => panic!("{first:?} and {second:?} must not share a queue path"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "download_spec_invalid");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn planner_reserves_internal_suffixes_across_separate_executions() {
    let root = temporary_root("reserved-download-paths")
        .canonicalize()
        .unwrap();
    let make_spec = |name: &str| DownloadSpec {
        url: "https://example.test/file".to_owned(),
        destination: root.join(name),
        expected_size: 1,
        sha1: None,
        sha256: None,
    };

    super::plan::build_plan(&root, vec![make_spec("a")])
        .expect("a normal destination remains valid");

    for reserved in ["a.part", "a.PART", "a.part.lock", "a.PART.LOCK"] {
        let error = match super::plan::build_plan(&root, vec![make_spec(reserved)]) {
            Ok(_) => panic!("{reserved:?} must be reserved in every plan"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "download_spec_invalid");
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn planner_handles_a_large_manifest_and_still_rejects_a_late_case_alias() {
    const FILE_COUNT: usize = 2_048;
    let root = temporary_root("large-download-plan")
        .canonicalize()
        .unwrap();
    let make_spec = |name: String| DownloadSpec {
        url: "https://example.test/file".to_owned(),
        destination: root.join(name),
        expected_size: 1,
        sha1: None,
        sha256: None,
    };
    let specs = (0..FILE_COUNT)
        .map(|index| make_spec(format!("objects/{index:04}.bin")))
        .collect();

    let plan = super::plan::build_plan(&root, specs).expect("large unique manifest is valid");
    assert_eq!(plan.total_bytes, FILE_COUNT as u64);
    assert_eq!(plan.pending.len(), FILE_COUNT);

    let mut colliding_specs = (0..FILE_COUNT)
        .map(|index| make_spec(format!("objects/{index:04}.bin")))
        .collect::<Vec<_>>();
    colliding_specs.push(make_spec("OBJECTS/0000.BIN".to_owned()));
    let error = match super::plan::build_plan(&root, colliding_specs) {
        Ok(_) => panic!("late Windows case alias must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "download_spec_invalid");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mismatched_partial_response_is_discarded_before_a_full_retry() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x52; 256];
        let full = bytes.clone();
        let server = TestServer::start(move |request_number, request| {
            if request_number == 1 {
                assert!(request.contains("range: bytes=64-"));
                TestResponse {
                    status: 206,
                    headers: vec![("Content-Range".to_owned(), "bytes 63-255/256".to_owned())],
                    chunks: vec![(full[64..].to_vec(), Duration::ZERO)],
                }
            } else {
                assert!(!request.contains("range:"));
                TestResponse::ok(full.clone())
            }
        });
        let root = temporary_root("bad-range");
        let destination = root.join("file.bin");
        fs::write(part_path(&destination), &bytes[..64]).unwrap();

        DownloadService::with_retry_dependencies(
            root.clone(),
            Arc::new(RecordingSleeper::default()),
            Arc::new(FixedJitter(Duration::ZERO)),
        )
        .unwrap()
        .execute(
            "bad-range",
            vec![spec(
                format!("{}/range", server.url),
                destination.clone(),
                &bytes,
            )],
            DownloadCancellationToken::new(),
            Arc::new(RecordingProgress::default()),
        )
        .await
        .unwrap();

        assert_eq!(server.requests(), 2);
        assert_eq!(fs::read(destination).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn range_not_satisfiable_discards_stale_part_and_retries_from_zero() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x27; 256];
        let full = bytes.clone();
        let server = TestServer::start(move |request_number, request| {
            if request_number == 1 {
                assert!(request.contains("range: bytes=64-"));
                TestResponse::status(416)
            } else {
                assert!(!request.contains("range:"));
                TestResponse::ok(full.clone())
            }
        });
        let root = temporary_root("range-416");
        let destination = root.join("file.bin");
        fs::write(part_path(&destination), &bytes[..64]).unwrap();

        DownloadService::with_retry_dependencies(
            root.clone(),
            Arc::new(RecordingSleeper::default()),
            Arc::new(FixedJitter(Duration::ZERO)),
        )
        .unwrap()
        .execute(
            "range-416",
            vec![spec(
                format!("{}/range", server.url),
                destination.clone(),
                &bytes,
            )],
            DownloadCancellationToken::new(),
            Arc::new(RecordingProgress::default()),
        )
        .await
        .unwrap();

        assert_eq!(server.requests(), 2);
        assert_eq!(fs::read(destination).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn idle_timeout_is_finite_and_destination_must_stay_inside_the_owned_root() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x66; 8];
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse {
            status: 200,
            headers: Vec::new(),
            chunks: vec![(body.clone(), Duration::from_millis(100))],
        });
        let root = temporary_root("timeout-path");
        let sleeper = Arc::new(RecordingSleeper::default());
        let service = DownloadService::with_configuration(
            root.clone(),
            DownloadTimeouts {
                connect: Duration::from_secs(1),
                request: Duration::from_secs(1),
                idle: Duration::from_millis(20),
            },
            sleeper,
            Arc::new(FixedJitter(Duration::ZERO)),
        )
        .unwrap();
        let error = service
            .execute(
                "timeout",
                vec![spec(
                    format!("{}/idle", server.url),
                    root.join("idle.bin"),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect_err("three idle timeouts fail");
        assert_eq!(server.requests(), 3);
        assert_eq!(error.code(), "download_timeout");

        let outside = root.parent().unwrap().join("outside.bin");
        let error = service
            .execute(
                "escape",
                vec![spec(format!("{}/file", server.url), outside, &bytes)],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect_err("outside destination is rejected before HTTP");
        assert_eq!(error.code(), "invalid_path");
        assert_eq!(server.requests(), 3);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn bounded_memory_adapter_rejects_unsolicited_partial_responses() {
    tauri::async_runtime::block_on(async {
        let server = TestServer::start(|_, _| TestResponse {
            status: 206,
            headers: vec![("Content-Range".to_owned(), "bytes 4-7/8".to_owned())],
            chunks: vec![(vec![0x41; 4], Duration::ZERO)],
        });

        let error = DownloadHttpClient::default()
            .fetch_bytes_bounded(&format!("{}/partial", server.url), 8)
            .await
            .expect_err("a full-memory fetch never accepts unsolicited partial data");

        assert_eq!(error.code(), "download_resume_incompatible");
        assert_eq!(server.requests(), 3);
    });
}

#[cfg(windows)]
#[test]
fn destination_parent_junction_is_rejected_before_any_http_request() {
    use std::process::Command;

    tauri::async_runtime::block_on(async {
        let bytes = b"outside".to_vec();
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse::ok(body.clone()));
        let root = temporary_root("junction");
        let external = temporary_root("junction-external");
        let junction = root.join("linked");
        let output = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&external)
            .output()
            .expect("junction command starts");
        if !output.status.success() {
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(external).unwrap();
            return;
        }

        let error = DownloadService::new(root.clone())
            .unwrap()
            .execute(
                "junction",
                vec![spec(
                    format!("{}/file", server.url),
                    junction.join("escaped.bin"),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect_err("junction-backed destination is rejected");

        assert_eq!(error.code(), "invalid_path");
        assert_eq!(server.requests(), 0);
        assert!(!external.join("escaped.bin").exists());
        fs::remove_dir(&junction).unwrap();
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(external).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn service_rejects_a_reparse_point_as_its_owned_root() {
    use std::process::Command;

    let holder = temporary_root("root-junction-holder");
    let external = temporary_root("root-junction-external");
    let junction = holder.join("download-root");
    let output = Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&external)
        .output()
        .expect("junction command starts");
    if !output.status.success() {
        fs::remove_dir_all(holder).unwrap();
        fs::remove_dir_all(external).unwrap();
        return;
    }

    let error = match DownloadService::new(junction.clone()) {
        Ok(_) => panic!("a junction cannot become the queue's trusted root"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "invalid_path");
    fs::remove_dir(&junction).unwrap();
    fs::remove_dir_all(holder).unwrap();
    fs::remove_dir_all(external).unwrap();
}

#[cfg(windows)]
#[test]
fn stale_lock_marker_does_not_block_safe_part_resume() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x39; 128];
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse::ok(body.clone()));
        let root = temporary_root("stale-lock");
        let destination = root.join("file.bin");
        fs::write(part_path(&destination), &bytes[..32]).unwrap();
        let mut lock_name = destination.as_os_str().to_os_string();
        lock_name.push(".part.lock");
        fs::write(PathBuf::from(lock_name), b"").unwrap();

        DownloadService::new(root.clone())
            .unwrap()
            .execute(
                "stale-lock",
                vec![spec(
                    format!("{}/file", server.url),
                    destination.clone(),
                    &bytes,
                )],
                DownloadCancellationToken::new(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .expect("an unlocked marker from a terminated owner is reusable");

        assert_eq!(fs::read(destination).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn exclusive_part_ownership_rejects_a_concurrent_writer() {
    tauri::async_runtime::block_on(async {
        let bytes = vec![0x61; 128];
        let body = bytes.clone();
        let server = TestServer::start(move |_, _| TestResponse {
            status: 200,
            headers: Vec::new(),
            chunks: vec![(body.clone(), Duration::from_millis(100))],
        });
        let root = temporary_root("part-owner");
        let destination = root.join("file.bin");
        let download = spec(format!("{}/file", server.url), destination.clone(), &bytes);
        let service = DownloadService::new(root.clone()).unwrap();

        let first = service.execute(
            "owner-1",
            vec![download.clone()],
            DownloadCancellationToken::new(),
            Arc::new(RecordingProgress::default()),
        );
        let second = service.execute(
            "owner-2",
            vec![download],
            DownloadCancellationToken::new(),
            Arc::new(RecordingProgress::default()),
        );
        let (first, second) = futures_util::future::join(first, second).await;

        let results = [first, second];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .map(|error| error.code())
                .collect::<Vec<_>>(),
            ["download_in_progress"]
        );
        assert_eq!(server.requests(), 1);
        assert_eq!(fs::read(destination).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    });
}
