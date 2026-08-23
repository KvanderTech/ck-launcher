use crate::downloads::DownloadCancellationToken;
use crate::error::LauncherError;
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use url::Url;

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const SUCCESS_HTML: &str = "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\"><title>ЦК Лаунчер</title><body><h1>Авторизация завершена</h1><p>Можно вернуться в лаунчер.</p></body></html>";
const ERROR_HTML: &str = "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\"><title>ЦК Лаунчер</title><body><h1>Вход не завершён</h1><p>Закройте эту страницу и повторите попытку.</p></body></html>";

pub struct CallbackReceiver {
    listener: Option<TcpListener>,
    state: String,
    timeout: Duration,
    used: bool,
}

impl CallbackReceiver {
    pub fn bind(state: impl Into<String>, timeout: Duration) -> Result<Self, LauncherError> {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| callback_unavailable())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| callback_unavailable())?;

        Ok(Self {
            listener: Some(listener),
            state: state.into(),
            timeout,
            used: false,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.listener
            .as_ref()
            .expect("an unused callback receiver owns its listener")
            .local_addr()
            .expect("a bound loopback listener always has a local address")
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://localhost:{}/callback", self.address().port())
    }

    pub fn receive(&mut self) -> Result<String, LauncherError> {
        self.receive_cancellable(&DownloadCancellationToken::new())
    }

    pub fn receive_cancellable(
        &mut self,
        cancel: &DownloadCancellationToken,
    ) -> Result<String, LauncherError> {
        if self.used {
            return Err(LauncherError::new(
                "auth_callback_used",
                "This sign-in callback has already been used.",
                None,
                false,
            ));
        }
        self.used = true;
        let listener = self.listener.take().ok_or_else(|| {
            LauncherError::new(
                "auth_callback_used",
                "This sign-in callback has already been used.",
                None,
                false,
            )
        })?;

        let deadline = Instant::now() + self.timeout;
        loop {
            if cancel.is_cancelled() {
                return Err(auth_cancelled());
            }
            match listener.accept() {
                Ok((mut stream, peer)) => {
                    if !is_ipv4_loopback(peer.ip()) {
                        write_response(&mut stream, false);
                        return Err(LauncherError::new(
                            "auth_invalid_callback",
                            "The sign-in callback was not local.",
                            None,
                            false,
                        ));
                    }
                    return self.handle_stream(&mut stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(LauncherError::new(
                            "auth_callback_timeout",
                            "Microsoft sign-in timed out.",
                            None,
                            true,
                        ));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(callback_unavailable()),
            }
        }
    }

    fn handle_stream(&self, stream: &mut TcpStream) -> Result<String, LauncherError> {
        stream
            .set_read_timeout(Some(self.timeout.min(Duration::from_secs(5))))
            .map_err(|_| callback_unavailable())?;
        let target = read_request_target(stream)?;
        let callback =
            Url::parse(&format!("http://localhost{target}")).map_err(|_| invalid_callback())?;
        if callback.path() != "/callback" {
            write_response(stream, false);
            return Err(invalid_callback());
        }

        let mut state = None;
        let mut code = None;
        for (key, value) in callback.query_pairs() {
            match key.as_ref() {
                "state" => state = Some(value.into_owned()),
                "code" => code = Some(value.into_owned()),
                _ => {}
            }
        }

        if state.as_deref() != Some(self.state.as_str()) {
            write_response(stream, false);
            return Err(LauncherError::new(
                "auth_invalid_state",
                "Microsoft sign-in state validation failed.",
                None,
                false,
            ));
        }
        let code = code.filter(|value| !value.is_empty()).ok_or_else(|| {
            write_response(stream, false);
            LauncherError::new(
                "auth_missing_code",
                "Microsoft did not return an authorization code.",
                None,
                true,
            )
        })?;

        write_response(stream, true);
        Ok(code)
    }
}

fn read_request_target(stream: &mut TcpStream) -> Result<String, LauncherError> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 512];
    while request.len() < MAX_REQUEST_BYTES {
        let read = stream.read(&mut chunk).map_err(|_| invalid_callback())?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&request).map_err(|_| invalid_callback())?;
    let mut parts = request
        .lines()
        .next()
        .ok_or_else(invalid_callback)?
        .split_ascii_whitespace();
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("GET"), Some(target), Some(version), None) if version.starts_with("HTTP/1.") => {
            Ok(target.to_owned())
        }
        _ => Err(invalid_callback()),
    }
}

fn write_response(stream: &mut TcpStream, success: bool) {
    let (status, body) = if success {
        ("200 OK", SUCCESS_HTML)
    } else {
        ("400 Bad Request", ERROR_HTML)
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn is_ipv4_loopback(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(address) if address.is_loopback())
}

fn invalid_callback() -> LauncherError {
    LauncherError::new(
        "auth_invalid_callback",
        "The Microsoft sign-in callback was invalid.",
        None,
        false,
    )
}

fn callback_unavailable() -> LauncherError {
    LauncherError::new(
        "auth_callback_unavailable",
        "The local Microsoft sign-in callback is unavailable.",
        None,
        true,
    )
}

fn auth_cancelled() -> LauncherError {
    LauncherError::new(
        "auth_cancelled",
        "Microsoft sign-in was cancelled.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::CallbackReceiver;
    use std::{
        io::{Read, Write},
        net::{IpAddr, TcpStream},
        thread,
        time::Duration,
    };

    fn send_callback(receiver: &CallbackReceiver, target: &str) -> thread::JoinHandle<String> {
        let address = receiver.address();
        let target = target.to_owned();
        thread::spawn(move || {
            let mut stream = TcpStream::connect(address).expect("callback connects");
            write!(
                stream,
                "GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .expect("callback request writes");
            let mut response = String::new();
            stream
                .read_to_string(&mut response)
                .expect("callback response reads");
            response
        })
    }

    #[test]
    fn binds_random_ipv4_loopback_and_returns_success_page() {
        let mut receiver =
            CallbackReceiver::bind("expected-state", Duration::from_secs(2)).expect("binds");
        assert_eq!(receiver.address().ip(), IpAddr::from([127, 0, 0, 1]));
        assert_ne!(receiver.address().port(), 0);
        assert_eq!(
            receiver.redirect_uri(),
            format!("http://localhost:{}/callback", receiver.address().port())
        );

        let client = send_callback(&receiver, "/callback?state=expected-state&code=oauth-code");
        assert_eq!(receiver.receive().expect("callback succeeds"), "oauth-code");
        let response = client.join().expect("client completes");
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("Авторизация завершена"));
    }

    #[test]
    fn rejects_wrong_state_and_missing_code_with_visible_error_pages() {
        for (target, expected_code) in [
            (
                "/callback?state=wrong&code=oauth-code",
                "auth_invalid_state",
            ),
            ("/callback?state=expected-state", "auth_missing_code"),
        ] {
            let mut receiver =
                CallbackReceiver::bind("expected-state", Duration::from_secs(2)).expect("binds");
            let client = send_callback(&receiver, target);
            let error = receiver.receive().expect_err("callback is rejected");
            assert_eq!(error.code(), expected_code);
            let response = client.join().expect("client completes");
            assert!(response.contains("HTTP/1.1 400 Bad Request"));
            assert!(response.contains("Вход не завершён"));
        }
    }

    #[test]
    fn callback_receiver_is_one_shot() {
        let mut receiver = CallbackReceiver::bind("state", Duration::from_secs(2)).expect("binds");
        let address = receiver.address();
        let client = send_callback(&receiver, "/callback?state=state&code=first");
        assert_eq!(receiver.receive().expect("first use works"), "first");
        client.join().expect("client completes");
        assert!(TcpStream::connect(address).is_err());

        let error = receiver.receive().expect_err("second use is rejected");
        assert_eq!(error.code(), "auth_callback_used");
    }

    #[test]
    fn cancellation_interrupts_a_blocked_callback_wait() {
        let mut receiver = CallbackReceiver::bind("state", Duration::from_secs(30)).expect("binds");
        let cancel = crate::downloads::DownloadCancellationToken::new();
        cancel.cancel();

        let started = std::time::Instant::now();
        let error = receiver
            .receive_cancellable(&cancel)
            .expect_err("cancelled callback stops");

        assert_eq!(error.code(), "auth_cancelled");
        assert!(started.elapsed() < Duration::from_millis(250));
    }
}
