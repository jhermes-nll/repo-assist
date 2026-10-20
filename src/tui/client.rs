//! Data source for the TUI: pulls the five RPCs and reconnects on failure.

use crate::ipc::client::{Client, IpcError};
use crate::ipc::Method;
use crate::tui::app::{Counts, Row, Tab};

#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    #[error("not a git repository")]
    NotGit,
    #[error("ipc: {0}")]
    Ipc(#[from] IpcError),
    #[error("daemon error [{code}]: {message}")]
    Daemon { code: String, message: String },
    #[error("transport: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub repo_key: String,
    pub daemon_version: String,
    pub counts: Counts,
    pub rows: [Vec<Row>; 4],
}

pub struct DataSource {
    sock: std::path::PathBuf,
    client: Option<Client>,
}

impl DataSource {
    pub fn new(sock: std::path::PathBuf) -> Self {
        Self { sock, client: None }
    }

    fn connect(&mut self) -> Result<(), TuiError> {
        let c = Client::connect(self.sock.clone())?;
        self.client = Some(c);
        Ok(())
    }

    fn ensure(&mut self) -> Result<(), TuiError> {
        if self.client.is_none() {
            self.connect()?;
        }
        Ok(())
    }

    fn call(
        &mut self,
        method: Method,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, TuiError> {
        self.ensure()?;
        let c = match self.client.as_mut() {
            Some(c) => c,
            None => return Err(TuiError::Transport("not connected".into())),
        };
        match c.call(method, params) {
            Ok(crate::ipc::Reply::Result { result, .. }) => Ok(result),
            Ok(crate::ipc::Reply::Error { error, .. }) => Err(TuiError::Daemon {
                code: error.code,
                message: error.message,
            }),
            Err(e) => {
                // reconnect next time
                self.client = None;
                Err(TuiError::Ipc(e))
            }
        }
    }

    pub fn fetch_all(&mut self) -> Result<Snapshot, TuiError> {
        // First try; on failure drop connection and retry once (reconnect).
        let first = self.fetch_all_once();
        match first {
            Ok(s) => Ok(s),
            Err(_) => {
                self.client = None;
                self.fetch_all_once()
            }
        }
    }

    fn fetch_all_once(&mut self) -> Result<Snapshot, TuiError> {
        let status = self.call(Method::Status, serde_json::Value::Null)?;
        let worktrees = self.call(Method::Worktrees, serde_json::Value::Null)?;
        let plans = self.call(Method::Plans, serde_json::Value::Null)?;
        let findings = self.call(Method::Findings, serde_json::Value::Null)?;
        let sessions = self.call(Method::Sessions, serde_json::Value::Null)?;

        let repo_key = status
            .get("repo_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let daemon_version = status
            .get("daemon_version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.1.0")
            .to_string();

        let mut snap = Snapshot {
            repo_key,
            daemon_version,
            counts: Counts::default(),
            rows: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        };
        snap.counts.worktrees = worktrees
            .get("worktrees")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        // status may carry counts
        snap.counts.worktrees = status
            .get("worktree_count")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(snap.counts.worktrees);
        snap.counts.plans = status
            .get("active_plans")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or_else(|| {
                plans
                    .get("plans")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0)
            });
        snap.counts.findings = status
            .get("open_findings")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        snap.counts.sessions = status
            .get("sessions")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);

        // rows
        let mut wrows = Vec::new();
        if let Some(arr) = worktrees.get("worktrees").and_then(|v| v.as_array()) {
            for w in arr {
                let name = w.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                wrows.push(Row::new(name.to_string()));
            }
        }
        let mut prows = Vec::new();
        if let Some(arr) = plans.get("plans").and_then(|v| v.as_array()) {
            for p in arr {
                let t = p.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                prows.push(Row::new(t.to_string()));
            }
        }
        let mut frows = Vec::new();
        if let Some(arr) = findings.get("findings").and_then(|v| v.as_array()) {
            for f in arr {
                let t = f.get("summary").and_then(|v| v.as_str()).unwrap_or("?");
                frows.push(Row::new(t.to_string()));
            }
        }
        let mut srows = Vec::new();
        if let Some(arr) = sessions.get("sessions").and_then(|v| v.as_array()) {
            for s in arr {
                let t = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                srows.push(Row::new(t.to_string()));
            }
        }
        snap.rows[Tab::Worktrees.index()] = wrows;
        snap.rows[Tab::Plans.index()] = prows;
        snap.rows[Tab::Findings.index()] = frows;
        snap.rows[Tab::Sessions.index()] = srows;
        Ok(snap)
    }

    pub fn ack_finding(&mut self, id: &str) -> Result<(), TuiError> {
        let r = self.call(Method::Ack, serde_json::json!({ "id": id }))?;
        let _ = r;
        Ok(())
    }

    pub fn sock(&self) -> &std::path::Path {
        &self.sock
    }
}
