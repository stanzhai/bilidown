//! Loopback captcha server — port of `util/auth/server.py::run_server`.
//!
//! Binds `127.0.0.1:2333` (the JS in `captcha.html` hardcodes that port)
//! and serves exactly three routes:
//!
//! - `OPTIONS *` → 200 + CORS headers (the browser preflight)
//! - `GET /geetest/captcha/init` → `{"challenge","gt"}` from the pending
//!   `CaptchaInfo`
//! - `POST /geetest/captcha/callback` (only with
//!   `Content-Type: application/json;charset=UTF-8`) → store
//!   `{challenge, seccode, validate}`, respond `OK`
//!
//! Python shuttled data via queues; here the `CaptchaInfo` and the solved
//! result live in shared slots on the handle.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use auth::login::CaptchaInfo;

/// The port `captcha.html` hardcodes (`http://localhost:2333/...`).
pub const CAPTCHA_PORT: u16 = 2333;

/// The seccode/validate pair the GeeTest widget posts back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptchaSolved {
    pub challenge: String,
    pub seccode: String,
    pub validate: String,
}

#[derive(Default)]
struct Shared {
    /// `CaptchaInfo` the page should initialize with (`get_geetest_info`).
    pending: Option<CaptchaInfo>,
    /// Solved captcha waiting to drive `sms_send`.
    solved: Option<CaptchaSolved>,
}

/// Running server handle: stop by dropping (the accept task is aborted).
pub struct CaptchaServer {
    shared: Arc<Mutex<Shared>>,
    task: tokio::task::JoinHandle<()>,
    pub port: u16,
}

impl CaptchaServer {
    /// Bind `127.0.0.1:2333` and start serving. Fails when the port is
    /// taken (Python's `HTTPServer` constructor raised the same way).
    pub async fn start() -> std::io::Result<Self> {
        Self::bind(CAPTCHA_PORT).await
    }

    /// Test seam: bind an ephemeral port so parallel tests never collide.
    pub async fn bind(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port)).await?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let task_shared = shared.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let shared = task_shared.clone();
                tokio::spawn(async move {
                    if serve(stream, shared).await.is_err() {
                        // malformed request: connection dropped, nothing to do
                    }
                });
            }
        });
        Ok(Self { shared, task, port })
    }

    /// Queue the `CaptchaInfo` the page will fetch (`init_geetest`).
    pub fn set_captcha(&self, info: CaptchaInfo) {
        self.shared.lock().pending = Some(info);
    }

    /// The solved pair, if the widget posted back (`captcha_success`).
    pub fn take_solved(&self) -> Option<CaptchaSolved> {
        self.shared.lock().solved.take()
    }

    pub fn stop(self) {
        self.task.abort();
    }
}

const CORS: &str = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n";

async fn serve(
    mut stream: tokio::net::TcpStream,
    shared: Arc<Mutex<Shared>>,
) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    // read until end of headers
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 64 * 1024 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let path = path.split('?').next().unwrap_or_default().to_string();

    let mut content_type = String::new();
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "content-type" {
            content_type = value.to_string();
        } else if name == "content-length" {
            content_length = value.parse().unwrap_or(0);
        }
    }

    // body (headers already fully read into buf)
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(buf.len());
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }

    match (method.as_str(), path.as_str()) {
        ("OPTIONS", _) => respond(&mut stream, 200, "text/html", b"", true).await,
        ("GET", "/geetest/captcha/init") => {
            let pair = {
                let guard = shared.lock();
                guard
                    .pending
                    .as_ref()
                    .map(|info| (info.challenge.clone(), info.gt.clone()))
            };
            match pair {
                Some((challenge, gt)) => {
                    let body = format!(
                        "{{\"challenge\":{},\"gt\":{}}}",
                        serde_json::to_string(&challenge).unwrap_or_default(),
                        serde_json::to_string(&gt).unwrap_or_default()
                    );
                    respond(&mut stream, 200, "application/json", body.as_bytes(), true).await
                }
                None => respond(&mut stream, 404, "text/html", b"Not Found", true).await,
            }
        }
        ("POST", "/geetest/captcha/callback") => {
            if content_type == "application/json;charset=UTF-8" {
                if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&body) {
                    let solved = CaptchaSolved {
                        challenge: parsed["challenge"].as_str().unwrap_or_default().to_string(),
                        seccode: parsed["seccode"].as_str().unwrap_or_default().to_string(),
                        validate: parsed["validate"].as_str().unwrap_or_default().to_string(),
                    };
                    shared.lock().solved = Some(solved);
                }
                respond(&mut stream, 200, "text/html", b"OK", true).await
            } else {
                respond(&mut stream, 404, "text/html", b"Not Found", true).await
            }
        }
        _ => respond(&mut stream, 404, "text/html", b"Not Found", true).await,
    }
}

async fn respond(
    stream: &mut tokio::net::TcpStream,
    code: u16,
    content_type: &str,
    body: &[u8],
    cors: bool,
) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
        body.len(),
        if cors { CORS } else { "" }
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn raw(port: u16, request: &str) -> (String, Vec<u8>) {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        use tokio::io::AsyncWriteExt;
        s.write_all(request.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf).into_owned();
        let split = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(buf.len());
        let body = buf[split..].to_vec();
        (text, body)
    }

    fn captcha() -> CaptchaInfo {
        CaptchaInfo {
            token: "t".into(),
            challenge: "ch-1".into(),
            gt: "gt-1".into(),
            seccode: String::new(),
            validate: String::new(),
            captcha_key: String::new(),
        }
    }

    #[tokio::test]
    async fn options_preflight_returns_cors() {
        let server = CaptchaServer::bind(0).await.unwrap();
        let (text, _) = raw(
            server.port,
            "OPTIONS * HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        server.stop();
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.contains("Access-Control-Allow-Origin: *"));
        assert!(text.contains("Access-Control-Allow-Methods: GET, POST, OPTIONS"));
        assert!(text.contains("Access-Control-Allow-Headers: Content-Type"));
    }

    #[tokio::test]
    async fn init_serves_pending_captcha() {
        let server = CaptchaServer::bind(0).await.unwrap();
        server.set_captcha(captcha());
        let (text, body) = raw(
            server.port,
            "GET /geetest/captcha/init HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        server.stop();
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.contains("Content-Type: application/json"));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["challenge"], "ch-1");
        assert_eq!(json["gt"], "gt-1");
    }

    #[tokio::test]
    async fn init_without_pending_captcha_404s() {
        let server = CaptchaServer::bind(0).await.unwrap();
        let (text, _) = raw(
            server.port,
            "GET /geetest/captcha/init HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        server.stop();
        assert!(text.starts_with("HTTP/1.1 404"), "{text}");
    }

    #[tokio::test]
    async fn callback_stores_solved_pair() {
        let server = CaptchaServer::bind(0).await.unwrap();
        let body = r#"{"challenge":"ch-1","seccode":"sec-1","validate":"val-1"}"#;
        let req = format!(
            "POST /geetest/captcha/callback HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (text, resp_body) = raw(server.port, &req).await;
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert_eq!(resp_body, b"OK");
        assert_eq!(
            server.take_solved(),
            Some(CaptchaSolved {
                challenge: "ch-1".into(),
                seccode: "sec-1".into(),
                validate: "val-1".into()
            })
        );
        assert_eq!(server.take_solved(), None, "solved is take-once");
        server.stop();
    }

    #[tokio::test]
    async fn callback_with_wrong_content_type_is_rejected() {
        let server = CaptchaServer::bind(0).await.unwrap();
        let req = "POST /geetest/captcha/callback HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
        let (text, _) = raw(server.port, req).await;
        server.stop();
        assert!(text.starts_with("HTTP/1.1 404"), "{text}");
    }

    #[tokio::test]
    async fn unknown_path_404s() {
        let server = CaptchaServer::bind(0).await.unwrap();
        let (text, _) = raw(
            server.port,
            "GET /nope HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        server.stop();
        assert!(text.starts_with("HTTP/1.1 404"), "{text}");
    }
}
