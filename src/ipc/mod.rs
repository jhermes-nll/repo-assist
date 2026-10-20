//! In-process IPC protocol (Section 6): a Unix domain socket, newline-delimited JSON, one
//! request per line, with a required `hello` handshake at the start of each connection.
//!
//! Wire format (each message is a single JSON line, newline-terminated):
//! ```text
//! {"id":1,"method":"status","params":{}}
//! {"id":1,"result":{…}}           or
//! {"id":1,"error":{"code":"…","message":"…"}}
//! ```

pub mod client;
pub mod server;

/// Re-exported so integration tests (and later CLI commands) reach the client in one place.
pub use client::{Client, IpcError};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Current protocol version the daemon speaks (Section 6).
pub const PROTO_VERSION: u64 = 1;
/// Default client identifier reported in the `hello` handshake.
pub const DEFAULT_CLIENT: &str = "ra-cli 0.1";

/// A request sent to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Monotonic request id, echoed back in the response.
    pub id: u64,
    /// Method name.
    pub method: Method,
    /// Free-form method parameters.
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// Build a request with an explicit id and parameters.
    pub fn new(id: u64, method: Method, params: Value) -> Self {
        Request { id, method, params }
    }

    /// A `hello` request (Section 6).
    pub fn hello(client: &str) -> Self {
        Request::new(
            1,
            Method::Hello,
            json!({ "proto": PROTO_VERSION, "client": client }),
        )
    }

    /// A `ping` request.
    pub fn ping() -> Self {
        Request::new(1, Method::Ping, Value::Null)
    }

    /// A `status` request.
    pub fn status() -> Self {
        Request::new(1, Method::Status, Value::Null)
    }

    /// A `shutdown` request (Section 6).
    pub fn shutdown() -> Self {
        Request::new(1, Method::Shutdown, Value::Null)
    }
}

/// The methods the daemon implements (Task 1.2: `hello`, `ping`, `status`; plus `shutdown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Hello,
    Ping,
    Status,
    Worktrees,
    Plans,
    Plan,
    Findings,
    Ack,
    Conflicts,
    WhoTouches,
    Drift,
    Sessions,
    Subscribe,
    RenderViews,
    Doctor,
    Shutdown,
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Method::Hello => "hello",
            Method::Ping => "ping",
            Method::Status => "status",
            Method::Worktrees => "worktrees",
            Method::Plans => "plans",
            Method::Plan => "plan",
            Method::Findings => "findings",
            Method::Ack => "ack",
            Method::Conflicts => "conflicts",
            Method::WhoTouches => "who-touches",
            Method::Drift => "drift",
            Method::Sessions => "sessions",
            Method::Subscribe => "subscribe",
            Method::RenderViews => "render_views",
            Method::Doctor => "doctor",
            Method::Shutdown => "shutdown",
        };
        f.write_str(s)
    }
}

/// A response line: either a result or an error (Section 6).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Reply {
    /// `{"id":N,"result":{…}}`.
    Result { id: u64, result: Value },
    /// `{"id":N,"error":{"code":"…","message":"…"}}`.
    Error { id: u64, error: ErrorReply },
}

/// An error response payload (Section 6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReply {
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}
