//! Authenticated HTTP portal and clipboard sync for the guest.
//!
//! QEMU user networking exposes the host as `10.222.0.2`; the guest agent
//! connects to `http://10.222.0.2:<bridge_port>` with a bearer token. The
//! bridge itself binds to loopback only and uses no `guestfwd`.
//!
//! # Quick start
//!
//! ```no_run
//! use std::path::PathBuf;
//!
//! use reims_guest_bridge::{serve, BridgeConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = BridgeConfig::new(8080, String::from("secret"), PathBuf::from("share"));
//! serve(config).await?;
//! # Ok(())
//! # }
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Address of the host in the SLIRP network.
pub const SLIRP_GATEWAY: &str = "10.222.0.2";
/// Maximum accepted request body.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
/// Maximum accepted header size.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Failures of the guest bridge.
#[derive(Debug, Error)]
pub enum BridgeError {
    /// sockets or file I/O failed.
    #[error("bridge I/O failure: {0}")]
    Io(#[from] std::io::Error),
    /// The request exceeded a size or time limit.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// A connection timed out.
    #[error("request timed out")]
    Timeout,
}

/// Configuration of one bridge instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    /// TCP port on loopback, reachable from the guest via `10.222.0.2`.
    pub port: u16,
    /// Bearer token shared with the guest agent.
    pub token: String,
    /// Directory for the file portal.
    pub share_dir: PathBuf,
}

impl BridgeConfig {
    /// Creates a bridge configuration.
    pub fn new(port: u16, token: impl Into<String>, share_dir: impl Into<PathBuf>) -> Self {
        Self {
            port,
            token: token.into(),
            share_dir: share_dir.into(),
        }
    }
}

struct State {
    token: String,
    share_dir: PathBuf,
    clipboard: Mutex<String>,
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '\0') || character.is_control())
}

fn find_headers(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), BridgeError> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

fn bearer(header: &str) -> Option<&str> {
    header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| value.trim().strip_prefix("Bearer "))
}

async fn dispatch(
    stream: &mut TcpStream,
    state: &State,
    method: &str,
    target: &str,
    body: &[u8],
    header: &str,
) -> Result<(), BridgeError> {
    if target == "/health" && method == "GET" {
        return respond(stream, "200 OK", "text/plain", b"ok").await;
    }
    if bearer(header) != Some(state.token.as_str()) {
        return respond(stream, "401 Unauthorized", "text/plain", b"unauthorized").await;
    }

    match (method, target) {
        ("GET", "/clipboard") => {
            let text = state.clipboard.lock().await.clone();
            respond(
                stream,
                "200 OK",
                "text/plain; charset=utf-8",
                text.as_bytes(),
            )
            .await
        }
        ("POST", "/clipboard") => {
            let text = String::from_utf8_lossy(body).into_owned();
            *state.clipboard.lock().await = text;
            respond(stream, "204 No Content", "text/plain", b"").await
        }
        ("GET", path) if path.starts_with("/files/") => {
            let name = &path["/files/".len()..];
            if !is_safe_name(name) {
                return respond(stream, "400 Bad Request", "text/plain", b"unsafe name").await;
            }
            let file = state.share_dir.join(name);
            match tokio::fs::read(&file).await {
                Ok(content) => {
                    respond(stream, "200 OK", "application/octet-stream", &content).await
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    respond(stream, "404 Not Found", "text/plain", b"not found").await
                }
                Err(error) => Err(error.into()),
            }
        }
        ("POST", path) if path.starts_with("/files/") => {
            let name = &path["/files/".len()..];
            if !is_safe_name(name) {
                return respond(stream, "400 Bad Request", "text/plain", b"unsafe name").await;
            }
            tokio::fs::create_dir_all(&state.share_dir).await?;
            tokio::fs::write(state.share_dir.join(name), body).await?;
            respond(stream, "204 No Content", "text/plain", b"").await
        }
        _ => respond(stream, "404 Not Found", "text/plain", b"not found").await,
    }
}

async fn handle_connection(mut stream: TcpStream, state: Arc<State>) -> Result<(), BridgeError> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = timeout(REQUEST_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| BridgeError::Timeout)??;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_headers(&buffer) {
            break position;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return respond(
                &mut stream,
                "431 Too Large",
                "text/plain",
                b"headers too large",
            )
            .await;
        }
    };
    let header = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let request_line = header.split("\r\n").next().unwrap_or_default();
    let mut words = request_line.split_whitespace();
    let method = words.next().unwrap_or_default().to_owned();
    let target = words.next().unwrap_or_default().to_owned();
    let content_length = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or_default();

    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length && content_length <= MAX_BODY_BYTES {
        let read = timeout(REQUEST_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| BridgeError::Timeout)??;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length.min(body.len()));
    dispatch(&mut stream, &state, &method, &target, &body, &header).await
}

/// Serves the bridge on loopback at `config.port`.
///
/// # Errors
///
/// Returns [`BridgeError::Io`] when the port cannot be bound.
pub async fn serve(config: BridgeConfig) -> Result<(), BridgeError> {
    let listener = TcpListener::bind(("127.0.0.1", config.port)).await?;
    serve_on(listener, config).await
}

/// Serves the bridge on an already bound listener; useful for tests.
///
/// # Errors
///
/// Returns [`BridgeError::Io`] when an accept fails.
pub async fn serve_on(listener: TcpListener, config: BridgeConfig) -> Result<(), BridgeError> {
    let state = Arc::new(State {
        token: config.token,
        share_dir: config.share_dir,
        clipboard: Mutex::new(String::new()),
    });
    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            // A failed client connection cannot stop the portal.
            let _ = handle_connection(stream, state).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    struct Fixture {
        _directory: tempfile::TempDir,
        address: SocketAddr,
        server: tokio::task::JoinHandle<Result<(), BridgeError>>,
        share: PathBuf,
    }

    async fn fixture() -> Fixture {
        let directory = tempfile::tempdir().expect("temp dir");
        let share = directory.path().join("share");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let config = BridgeConfig::new(0, "secret", share.clone());
        let server = tokio::spawn(serve_on(listener, config));
        Fixture {
            _directory: directory,
            address,
            server,
            share,
        }
    }

    async fn request(address: SocketAddr, raw: &[u8]) -> String {
        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream.write_all(raw).await.expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        String::from_utf8_lossy(&response).into_owned()
    }

    #[tokio::test]
    async fn health_is_public() {
        let fixture = fixture().await;
        let response = request(
            fixture.address,
            b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .await;
        assert!(response.contains("200 OK"));
        assert!(response.ends_with("ok"));
        fixture.server.abort();
    }

    #[tokio::test]
    async fn token_is_required() {
        let fixture = fixture().await;
        let response = request(
            fixture.address,
            b"GET /clipboard HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .await;
        assert!(response.contains("401 Unauthorized"));
        fixture.server.abort();
    }

    #[tokio::test]
    async fn clipboard_round_trips() {
        let fixture = fixture().await;
        let post = b"POST /clipboard HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\nContent-Length: 5\r\n\r\nhello";
        let response = request(fixture.address, post).await;
        assert!(response.contains("204 No Content"));
        let response = request(
            fixture.address,
            b"GET /clipboard HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\n\r\n",
        )
        .await;
        assert!(response.ends_with("hello"));
        fixture.server.abort();
    }

    #[tokio::test]
    async fn files_upload_and_download() {
        let fixture = fixture().await;
        let post = b"POST /files/note.txt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\nContent-Length: 7\r\n\r\ncontent";
        assert!(
            request(fixture.address, post)
                .await
                .contains("204 No Content")
        );
        assert_eq!(
            tokio::fs::read(fixture.share.join("note.txt"))
                .await
                .expect("uploaded file"),
            b"content"
        );
        let response = request(
            fixture.address,
            b"GET /files/note.txt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\n\r\n",
        )
        .await;
        assert!(response.ends_with("content"));
        fixture.server.abort();
    }

    #[tokio::test]
    async fn traversal_is_rejected() {
        let fixture = fixture().await;
        let response = request(
            fixture.address,
            b"GET /files/../secret HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\n\r\n",
        )
        .await;
        assert!(response.contains("400 Bad Request"));
        fixture.server.abort();
    }
}
