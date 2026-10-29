//! Agent session source (Task 4 / Section 8.2): parse pi session JSONL files into
//! `sessions` + `tool_calls` aggregates. Files are only ever READ — the daemon never
//! writes inside any work tree, and that includes session files.
//!
//! Header placement: `ingest_file` accepts the first `type:"session"` header line
//! anywhere in the file; `sync` separately enforces "header on line 1" via
//! `read_session_id` and skips files lacking it without writing a byte offset.

use std::path::{Path, PathBuf};

use crate::paths::{self, Env};
use crate::store::Store;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The agent this source ingests (`sessions.agent`).
pub const AGENT: &str = "pi";

#[derive(Debug, Deserialize)]
struct SessionHeader {
    #[serde(rename = "type")]
    kind: String,
    version: i64,
    id: String,
    timestamp: String,
    cwd: Option<String>,
}

#[derive(Debug)]
pub struct CallInsert {
    pub ts_ms: i64,
    pub tool: String,
    pub args_hash: String,
    pub is_error: Option<bool>,
    pub result_hash: Option<String>,
}

#[derive(Debug)]
pub struct Ingested {
    pub id: String,
    pub cwd: Option<String>,
    pub started_ms: Option<i64>,
    pub last_event_ms: Option<i64>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache: i64,
    pub cost_usd: Option<f64>,
    pub parse_errors: i64,
    pub calls: Vec<CallInsert>,
}

fn epoch_ms(ts: &str) -> Option<i64> {
    let dt =
        time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).ok()?;
    Some(dt.unix_timestamp() * 1000 + i64::from(dt.millisecond()))
}

fn hash16(v: serde_json::Value) -> String {
    let digest = Sha256::digest(serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()));
    let hex = hex::encode(digest);
    hex[..16].to_string()
}

pub fn ingest_file(path: &Path) -> Option<Ingested> {
    let raw = std::fs::read_to_string(path).ok()?;
    let complete_end = raw.rfind('\n').map(|i| i + 1).unwrap_or(0);
    parse_file_lines(&raw[..complete_end])
}

fn parse_file_lines(complete: &str) -> Option<Ingested> {
    let mut id: Option<String> = None;
    let mut have_header = false;
    let mut cwd: Option<String> = None;
    let mut started_ms: Option<i64> = None;
    let mut last_event_ms: Option<i64> = None;
    let mut tokens_in = 0;
    let mut tokens_out = 0;
    let mut tokens_cache = 0;
    let mut cost_usd: Option<f64> = None;
    let mut parse_errors = 0;
    let mut calls: Vec<CallInsert> = Vec::new();
    let mut pending: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for line in complete.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if let Some(ts) = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(epoch_ms)
        {
            last_event_ms = Some(last_event_ms.map_or(ts, |m: i64| m.max(ts)));
        }
        match kind {
            "session" => {
                if let Ok(h) = serde_json::from_value::<SessionHeader>(v.clone()) {
                    if !have_header && h.kind == "session" && h.version == 3 {
                        have_header = true;
                        id = Some(h.id);
                        cwd = h.cwd;
                        started_ms = epoch_ms(&h.timestamp);
                    }
                }
            }
            "message" => {
                let msg = &v["message"];
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let ts = v
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(epoch_ms)
                    .or_else(|| {
                        msg.get("timestamp")
                            .and_then(|t| t.as_str())
                            .and_then(epoch_ms)
                    })
                    .unwrap_or(0);
                if role == "assistant" {
                    if let Some(u) = msg.get("usage").and_then(|u| u.as_object()) {
                        tokens_in += u.get("input").and_then(|x| x.as_i64()).unwrap_or(0);
                        tokens_out += u.get("output").and_then(|x| x.as_i64()).unwrap_or(0);
                        tokens_cache += u.get("cacheRead").and_then(|x| x.as_i64()).unwrap_or(0);
                        if let Some(total) = u
                            .get("cost")
                            .and_then(|c| c.get("total"))
                            .and_then(|x| x.as_f64())
                        {
                            cost_usd = Some(cost_usd.unwrap_or(0.0) + total);
                        }
                    }
                    if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                        for b in blocks {
                            if b.get("type").and_then(|t| t.as_str()) != Some("toolCall") {
                                continue;
                            }
                            let call_id = b
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string();
                            pending.insert(call_id, calls.len());
                            calls.push(CallInsert {
                                ts_ms: ts,
                                tool: b
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("?")
                                    .to_string(),
                                args_hash: hash16(
                                    b.get("arguments")
                                        .cloned()
                                        .unwrap_or(serde_json::Value::Null),
                                ),
                                is_error: None,
                                result_hash: None,
                            });
                        }
                    }
                } else if role == "toolResult" {
                    if let Some(cid) = msg.get("toolCallId").and_then(|c| c.as_str()) {
                        if let Some(&i) = pending.get(cid) {
                            calls[i].is_error = msg.get("isError").and_then(|e| e.as_bool());
                            calls[i].result_hash = Some(hash16(
                                msg.get("content")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null),
                            ));
                        }
                    }
                }
            }
            "compaction" | "branch_summary" => {}
            "model_change" | "thinking_level_change" => {}
            "" => parse_errors += 1,
            _ => parse_errors += 1,
        }
    }

    id.map(|id| Ingested {
        id,
        cwd,
        started_ms,
        last_event_ms,
        tokens_in,
        tokens_out,
        tokens_cache,
        cost_usd,
        parse_errors,
        calls,
    })
}

pub fn resolve_sessions_root(env: &Env) -> PathBuf {
    let expand = |dir: String| {
        if let Some(rest) = dir.strip_prefix("~/") {
            PathBuf::from(format!("{}/{}", env.home.trim_end_matches('/'), rest))
        } else {
            PathBuf::from(dir)
        }
    };
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_SESSION_DIR") {
        if !dir.trim().is_empty() {
            return expand(dir);
        }
    }
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        if !dir.trim().is_empty() {
            return expand(dir).join("sessions");
        }
    }
    expand(env.home.clone()).join(".pi/agent/sessions")
}

pub fn discover_session_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    let mut dirs = vec![root.to_path_buf()];
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.push(entry.path());
        }
    }
    for d in dirs {
        let Ok(sub) = std::fs::read_dir(&d) else {
            continue;
        };
        for f in sub.flatten() {
            let jsonl = f.path().extension().map(|e| e == "jsonl").unwrap_or(false);
            if jsonl && f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                files.push(f.path());
            }
        }
    }
    files.sort();
    files
}

fn match_worktree(cwd: Option<&str>, worktrees: &[(i64, String)]) -> Option<i64> {
    let cwd = cwd?;
    let cwd = paths::canonicalize(Path::new(cwd));
    worktrees
        .iter()
        .filter(|(_, path)| {
            let wt = paths::canonicalize(Path::new(path));
            cwd.starts_with(&wt)
        })
        .max_by_key(|(_, path)| paths::canonicalize(Path::new(path)).as_os_str().len())
        .map(|(id, _)| *id)
}

pub fn sync_sessions(store: &Store, repo_root: &Path, sessions_root: &Path) {
    if let Err(e) = crate::sources::git::sync_worktrees(store, repo_root) {
        eprintln!("sessions sync: git source failed: {e}");
    }
    let worktrees: Vec<(i64, String)> = store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT id, path FROM worktrees WHERE present = 1")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_default();

    let offsets = store.session_file_offsets().unwrap_or_default();

    for file in discover_session_files(sessions_root) {
        let Ok(meta) = std::fs::metadata(&file) else {
            eprintln!("sessions sync: cannot stat {}: skipped", file.display());
            continue;
        };
        let raw = match std::fs::read_to_string(&file) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "sessions sync: cannot read {}: {e}: skipped",
                    file.display()
                );
                continue;
            }
        };
        let size = meta.len() as i64;
        let Some(id) = read_session_id(&file) else {
            if !raw.trim().is_empty() {
                eprintln!(
                    "sessions sync: cannot parse session header in {}: skipped",
                    file.display()
                );
            }
            continue;
        };
        if offsets.get(&id) == Some(&size) {
            continue;
        }
        let Some(ing) = ingest_file(&file) else {
            continue;
        };
        let file_s = file.display().to_string();
        let row = crate::store::SessionRow {
            id: &ing.id,
            agent: AGENT,
            file: &file_s,
            cwd: ing.cwd.as_deref(),
            worktree_id: match_worktree(ing.cwd.as_deref(), &worktrees),
            started_ms: ing.started_ms,
            last_event_ms: ing.last_event_ms,
            tokens_in: ing.tokens_in,
            tokens_out: ing.tokens_out,
            tokens_cache: ing.tokens_cache,
            cost_usd: ing.cost_usd,
            tool_calls: ing.calls.len() as i64,
            byte_offset: size,
            parse_errors: ing.parse_errors,
        };
        let all_calls: Vec<crate::store::ToolCallRow<'_>> = ing
            .calls
            .iter()
            .map(|c| crate::store::ToolCallRow {
                ts_ms: c.ts_ms,
                tool: &c.tool,
                args_hash: &c.args_hash,
                is_error: c.is_error,
                result_hash: c.result_hash.as_deref(),
            })
            .collect();
        // Retention ring: persist only the newest TOOL_CALL_RING_SIZE calls.
        let start = all_calls
            .len()
            .saturating_sub(crate::store::TOOL_CALL_RING_SIZE);
        let calls = &all_calls[start..];
        if let Err(e) = store.upsert_session_with_calls(&row, calls) {
            eprintln!("sessions sync: store write failed for {file_s}: {e}");
        }
    }
}

fn read_session_id(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(f).read_line(&mut line).ok()?;
    match serde_json::from_str::<SessionHeader>(&line) {
        Ok(h) if h.kind == "session" && h.version == 3 => Some(h.id),
        _ => None,
    }
}
