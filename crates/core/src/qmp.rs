//! Minimal TCP QMP client with explicit watchdogs.
//!
//! # Quick start
//!
//! ```no_run
//! use std::net::SocketAddr;
//! use std::time::Duration;
//!
//! use reims_core::qmp::QmpClient;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let address: SocketAddr = "127.0.0.1:4444".parse()?;
//! let mut qmp = QmpClient::connect(address, Duration::from_secs(2)).await?;
//! qmp.greet().await?;
//! qmp.shutdown().await?;
//! # Ok(())
//! # }
//! ```

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Watchdog for `system_powerdown` and `quit`.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
/// Watchdog for `screendump`.
pub const SCREENDUMP_TIMEOUT: Duration = Duration::from_secs(3);

/// Failures of the QMP client.
#[derive(Debug, Error)]
pub enum QmpError {
    /// Socket or stream failure.
    #[error("QMP I/O failure: {source}")]
    Io {
        /// Underlying I/O failure.
        #[source]
        #[from]
        source: std::io::Error,
    },
    /// Invalid JSON on the wire.
    #[error("invalid QMP JSON: {source}")]
    Json {
        /// Underlying parse failure.
        #[source]
        #[from]
        source: serde_json::Error,
    },
    /// The operation exceeded its watchdog.
    #[error("timeout after {after_secs}s while executing {operation}")]
    Timeout {
        /// Operation that timed out.
        operation: String,
        /// Watchdog length in seconds.
        after_secs: u64,
    },
    /// QMP replied with an `error` object.
    #[error("QMP command {command} failed: {message}")]
    Command {
        /// Command name.
        command: String,
        /// QMP error description.
        message: String,
    },
    /// The peer violated the QMP handshake or reply format.
    #[error("QMP protocol error: {0}")]
    Protocol(String),
}

/// A QMP connection over any asynchronous byte stream.
#[derive(Debug)]
pub struct QmpClient<S> {
    reader: BufReader<S>,
    next_id: u64,
}

impl QmpClient<TcpStream> {
    /// Opens a TCP QMP connection within `timeout`.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError::Timeout`] or [`QmpError::Io`] on connection failure.
    pub async fn connect(address: SocketAddr, timeout: Duration) -> Result<Self, QmpError> {
        let stream = match tokio::time::timeout(timeout, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(source)) => return Err(QmpError::Io { source }),
            Err(_) => {
                return Err(QmpError::Timeout {
                    operation: String::from("connect"),
                    after_secs: timeout.as_secs(),
                });
            }
        };
        Ok(Self::new(stream))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> QmpClient<S> {
    /// Wraps an existing stream.
    pub fn new(stream: S) -> Self {
        Self {
            reader: BufReader::new(stream),
            next_id: 1,
        }
    }

    /// Reads and validates the QMP greeting banner.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError`] when the banner is missing or malformed.
    pub async fn greet(&mut self) -> Result<Value, QmpError> {
        let value = self.read_value().await?;
        if value.get("QMP").is_none() {
            return Err(QmpError::Protocol(String::from("missing QMP banner")));
        }
        Ok(value)
    }

    /// Executes a command with a total watchdog.
    ///
    /// Events without a matching `id` are ignored. Callers that need a custom
    /// deadline can pass it explicitly; shutdown and screendump use the
    /// constants defined in this module.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError::Timeout`] when no reply arrives within `timeout`.
    pub async fn command(
        &mut self,
        name: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<Value, QmpError> {
        match tokio::time::timeout(timeout, self.command_inner(name, arguments)).await {
            Ok(result) => result,
            Err(_) => Err(QmpError::Timeout {
                operation: name.to_owned(),
                after_secs: timeout.as_secs(),
            }),
        }
    }

    /// Requests a graceful guest shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError`] if QMP declines the command or the watchdog fires.
    pub async fn shutdown(&mut self) -> Result<Value, QmpError> {
        self.command("system_powerdown", json!({}), SHUTDOWN_TIMEOUT)
            .await
    }

    /// Resets the guest.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError`] if QMP declines the command or the watchdog fires.
    pub async fn reset(&mut self) -> Result<Value, QmpError> {
        self.command("system_reset", json!({}), SHUTDOWN_TIMEOUT)
            .await
    }

    /// Terminates QEMU immediately.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError`] if QMP declines the command or the watchdog fires.
    pub async fn quit(&mut self) -> Result<Value, QmpError> {
        self.command("quit", json!({}), SHUTDOWN_TIMEOUT).await
    }

    /// Writes a screendump to `filename` on the host.
    ///
    /// # Errors
    ///
    /// Returns [`QmpError`] if QMP declines the command or the watchdog fires.
    pub async fn screendump(&mut self, filename: &str) -> Result<Value, QmpError> {
        self.command(
            "screendump",
            json!({ "filename": filename }),
            SCREENDUMP_TIMEOUT,
        )
        .await
    }

    async fn command_inner(&mut self, name: &str, arguments: Value) -> Result<Value, QmpError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| QmpError::Protocol(String::from("command id space exhausted")))?;
        let request = json!({ "execute": name, "arguments": arguments, "id": id });
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        let stream = self.reader.get_mut();
        stream.write_all(line.as_bytes()).await?;
        stream.flush().await?;

        loop {
            let value = self.read_value().await?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(QmpError::Command {
                    command: name.to_owned(),
                    message: error.to_string(),
                });
            }
            if let Some(result) = value.get("return") {
                return Ok(result.clone());
            }
            return Err(QmpError::Protocol(String::from(
                "reply has neither return nor error",
            )));
        }
    }

    async fn read_value(&mut self) -> Result<Value, QmpError> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).await?;
        if read == 0 {
            return Err(QmpError::Protocol(String::from("connection closed")));
        }
        Ok(serde_json::from_str(line.trim())?)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

    use super::*;

    #[tokio::test]
    async fn command_skips_events_and_matches_the_id()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (client_side, server_side) = duplex(4096);
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_side);
            let mut line = String::new();
            reader.read_line(&mut line).await?;
            let request: Value = serde_json::from_str(&line)?;
            assert_eq!(request["execute"], "system_powerdown");
            let id = request["id"].clone();
            reader
                .get_mut()
                .write_all(b"{\"event\":\"SHUTDOWN\"}\n")
                .await?;
            let reply = json!({ "return": {}, "id": id }).to_string() + "\n";
            reader.get_mut().write_all(reply.as_bytes()).await?;
            reader.get_mut().flush().await?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });

        let mut client = QmpClient::new(client_side);
        let result = client
            .shutdown()
            .await
            .expect("shutdown replies after an event");
        assert_eq!(result, json!({}));
        server.await.expect("server task")?;
        Ok(())
    }

    #[tokio::test]
    async fn command_reports_qmp_errors() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (client_side, server_side) = duplex(4096);
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_side);
            let mut line = String::new();
            reader.read_line(&mut line).await?;
            let request: Value = serde_json::from_str(&line)?;
            let reply = json!({
                "error": { "class": "GenericError", "desc": "no such device" },
                "id": request["id"]
            })
            .to_string()
                + "\n";
            reader.get_mut().write_all(reply.as_bytes()).await?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });

        let mut client = QmpClient::new(client_side);
        let error = client
            .screendump("screen.png")
            .await
            .expect_err("QMP error becomes an error");
        assert!(matches!(error, QmpError::Command { .. }));
        server.await.expect("server task")?;
        Ok(())
    }

    #[tokio::test]
    async fn greet_accepts_only_the_banner() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        let (client_side, server_side) = duplex(4096);
        let server = tokio::spawn(async move {
            let mut writer = server_side;
            writer
                .write_all(b"{\"QMP\":{\"version\":{},\"capabilities\":[]}}\n")
                .await?;
            Ok::<(), std::io::Error>(())
        });
        let mut client = QmpClient::new(client_side);
        let banner = client.greet().await.expect("banner");
        assert!(banner.get("QMP").is_some());
        server.await.expect("server task")?;
        Ok(())
    }

    #[tokio::test]
    async fn watchdog_stops_a_silent_command() {
        let (client_side, server_side) = duplex(4096);
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_side);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            // Keep the stream open without replying.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let mut client = QmpClient::new(client_side);
        let error = client
            .command("query-status", json!({}), Duration::from_millis(50))
            .await
            .expect_err("silent peer times out");
        assert!(matches!(error, QmpError::Timeout { .. }));
        server.abort();
    }
}
