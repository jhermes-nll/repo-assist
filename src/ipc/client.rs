//! A synchronous one-shot IPC client (Section 6): connect, perform the required `hello`
//! handshake, then issue one or more requests on the same connection, reading one response
//! line per request. Used by the CLI commands, which stay off the async runtime.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde_json::Value;

use crate::ipc::{Method, Reply, Request, DEFAULT_CLIENT, PROTO_VERSION};

/// Errors surfaced by the synchronous CLI client.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// No daemon is listening at the expected socket (Section 6).
    #[error("no daemon running at {0}")]
    NotRunning(String),
    /// The daemon socket could not be connected to.
    #[error("could not connect to the daemon at {0}: {1}")]
    Connect(String, String),
    /// The daemon speaks a different protocol version (Section 6).
    #[error("incompatible protocol: the daemon speaks proto {0}")]
    IncompatibleProto(u64),
    /// The daemon replied with an error object.
    #[error("daemon replied with an error: {0}")]
    Daemon(String),
    /// A transport-level failure (read/write/json).
    #[error("transport error: {0}")]
    Transport(String),
    /// A read/write timed out (retryable).
    #[error("io timeout: {0}")]
    Timeout(String),
}

/// A synchronous IPC client backed by a single Unix domain connection (Section 6).
pub struct Client {
    stream: UnixStream,
}

impl Client {
    /// Connect to the daemon at `sock`, perform the required `hello` handshake, and return a
    /// client that can issue further requests.
    pub fn connect(sock: impl Into<PathBuf>) -> Result<Self, IpcError> {
        let sock = sock.into();
        if !sock.exists() {
            return Err(IpcError::NotRunning(sock.display().to_string()));
        }
        let mut stream = UnixStream::connect(&sock)
            .map_err(|e| IpcError::Connect(sock.display().to_string(), e.to_string()))?;
        let to = std::time::Duration::from_secs(3);
        let _ = stream.set_read_timeout(Some(to));
        let _ = stream.set_write_timeout(Some(to));

        // The first message on every connection must be `hello` (Section 6).
        write_line(&mut stream, &Request::hello(DEFAULT_CLIENT))
            .map_err(|e| IpcError::Transport(e.to_string()))?;
        let reply = read_reply(&mut stream).map_err(|e| IpcError::Transport(e.to_string()))?;
        match reply {
            Reply::Result { result, .. } => {
                let proto = result
                    .get("proto")
                    .and_then(Value::as_u64)
                    .unwrap_or(PROTO_VERSION);
                if proto != PROTO_VERSION {
                    return Err(IpcError::IncompatibleProto(proto));
                }
            }
            Reply::Error { error, .. } => return Err(IpcError::Daemon(error.code)),
        }
        Ok(Client { stream })
    }

    /// Send one request and read its single response line (Section 6).
    pub fn call(&mut self, method: Method, params: Value) -> Result<Reply, IpcError> {
        write_line(&mut self.stream, &Request::new(1, method, params)).map_err(io_err_to_ipc)?;
        read_reply(&mut self.stream).map_err(io_err_to_ipc)
    }
}

/// Write a single JSON request line (Section 6), terminated by `\n`.
fn write_line<S>(stream: &mut S, req: &Request) -> std::io::Result<()>
where
    S: Write,
{
    let line =
        serde_json::to_string(req).map_err(|e| std::io::Error::other(format!("json: {e}")))?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

/// Read a single JSON response line and parse it into a [`Reply`].
fn read_reply<S>(stream: &mut S) -> Result<Reply, std::io::Error>
where
    S: Read,
{
    let mut rdr = BufReader::new(stream);
    let mut line = String::new();
    let n = rdr.read_line(&mut line)?;
    if n == 0 {
        return Err(std::io::Error::other("socket closed"));
    }
    serde_json::from_str(line.trim()).map_err(|e| std::io::Error::other(format!("json: {e}")))
}

fn io_err_to_ipc(e: std::io::Error) -> IpcError {
    if matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        IpcError::Timeout(e.to_string())
    } else {
        IpcError::Transport(e.to_string())
    }
}
