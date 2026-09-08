//! Private JSON-lines channel over inherited pipes. No network listener.
use ck_launcher_core::{
    api, context::AppContext, error::LauncherError, events::EventBus, paths::AppPaths, tasks,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::{mpsc, Arc},
};
const MAX_REQUEST: usize = 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    method: String,
    #[serde(default = "object")]
    params: Value,
}
fn object() -> Value {
    json!({})
}
fn send(tx: &mpsc::SyncSender<Value>, value: Value) {
    let _ = tx.send(value);
}

fn control_request(method: &str) -> bool {
    matches!(
        method,
        "stop_game"
            | "cancel_operation"
            | "cancel_content_operation"
            | "cancel_microsoft_login"
            | "launch_status"
            | "installation_status"
    )
}

fn reserve_request(
    method: &str,
    ordinary: &Arc<tokio::sync::Semaphore>,
    control: &Arc<tokio::sync::Semaphore>,
) -> Result<tokio::sync::OwnedSemaphorePermit, tokio::sync::TryAcquireError> {
    // Icon downloads may wait on their own six-request queue. They must not exhaust
    // the capacity needed to stop a game or cancel a long-running operation.
    if control_request(method) {
        control.clone()
    } else {
        ordinary.clone()
    }
    .try_acquire_owned()
}
fn run() -> Result<(), LauncherError> {
    let (tx, rx) = mpsc::sync_channel::<Value>(256);
    std::thread::spawn(move || {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for value in rx {
            if serde_json::to_writer(&mut out, &value).is_err()
                || out.write_all(b"\n").is_err()
                || out.flush().is_err()
            {
                break;
            }
        }
    });
    let event_tx = tx.clone();
    let events = EventBus::new(move |event, data| {
        let frame = json!({"event":event,"data":data});
        if matches!(event, "launcher://progress" | "launcher://content-progress") {
            let _ = event_tx.try_send(frame);
        } else {
            let _ = event_tx.send(frame);
        }
    });
    let paths = AppPaths::windows_default()?;
    let ctx = Arc::new(tasks::block_on(AppContext::new(paths, events))?);
    let concurrency = Arc::new(tokio::sync::Semaphore::new(32));
    let control_capacity = Arc::new(tokio::sync::Semaphore::new(8));
    let stdin = io::stdin();
    let mut input = stdin.lock();
    loop {
        let mut line = Vec::new();
        loop {
            let buf = input
                .fill_buf()
                .map_err(|e| LauncherError::internal(e.to_string()))?;
            if buf.is_empty() {
                break;
            }
            let n = buf
                .iter()
                .position(|b| *b == b'\n')
                .map_or(buf.len(), |p| p + 1);
            if line.len() + n > MAX_REQUEST {
                return Err(LauncherError::new(
                    "request_too_large",
                    "Слишком большой запрос.",
                    None,
                    false,
                ));
            }
            line.extend_from_slice(&buf[..n]);
            input.consume(n);
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        if line.is_empty() {
            break;
        }
        let req: Request = match serde_json::from_slice(&line) {
            Ok(req) => req,
            Err(_) => {
                send(
                    &tx,
                    json!({"id":null,"error":{"code":"invalid_request","message":"Некорректный запрос."}}),
                );
                continue;
            }
        };
        let permit = match reserve_request(&req.method, &concurrency, &control_capacity) {
            Ok(p) => p,
            Err(_) => {
                send(
                    &tx,
                    json!({"id":req.id,"error":{"code":"busy","message":"Слишком много операций."}}),
                );
                continue;
            }
        };
        let ctx = ctx.clone();
        let tx = tx.clone();
        tasks::spawn(async move {
            let _permit = permit;
            let response = match api::dispatch(&ctx, &req.method, req.params).await {
                Ok(value) => json!({"id":req.id,"result":value}),
                Err(err) => json!({"id":req.id,"error":err}),
            };
            send(&tx, response);
        });
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {}", error.code(), error.message());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturated_image_queue_cannot_block_stop_or_cancellation() {
        let ordinary = Arc::new(tokio::sync::Semaphore::new(32));
        let control = Arc::new(tokio::sync::Semaphore::new(8));
        let images: Vec<_> = (0..32)
            .map(|_| reserve_request("load_public_image", &ordinary, &control).unwrap())
            .collect();
        assert!(reserve_request("load_public_image", &ordinary, &control).is_err());
        for method in [
            "stop_game",
            "cancel_operation",
            "cancel_content_operation",
            "cancel_microsoft_login",
            "launch_status",
            "installation_status",
        ] {
            assert!(
                reserve_request(method, &ordinary, &control).is_ok(),
                "{method}"
            );
        }
        drop(images);
        assert!(reserve_request("load_public_image", &ordinary, &control).is_ok());
    }

    #[test]
    fn reserved_control_queue_is_bounded_and_never_available_to_slow_requests() {
        let ordinary = Arc::new(tokio::sync::Semaphore::new(0));
        let control = Arc::new(tokio::sync::Semaphore::new(8));
        let requests: Vec<_> = (0..8)
            .map(|_| reserve_request("stop_game", &ordinary, &control).unwrap())
            .collect();
        assert!(reserve_request("stop_game", &ordinary, &control).is_err());
        drop(requests);
        assert!(reserve_request("install_modrinth_modpack", &ordinary, &control).is_err());
        assert!(reserve_request("stop_game", &ordinary, &control).is_ok());
    }
}
