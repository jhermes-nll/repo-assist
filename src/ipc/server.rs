//! Per-connection handling for the daemon's IPC layer (Section 6).
//!
//! Each accepted connection reads newline-delimited JSON requests. The first message must be a
//! `hello` handshake; the daemon then handles `ping`, `status`, and `shutdown`. All other
//! methods receive a `not_implemented` error.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

use crate::ipc::{Method, Request, PROTO_VERSION};
use crate::store::Store;
use crate::{TOOL_NAME, VERSION};

/// The shutdown signal handed to the accept loop: the watch **Sender**. Any connection task
/// (or the daemon's process-shutdown path) flips it, which the accept loop observes and the
/// connection tasks react to (Section 6: graceful shutdown).
pub type StopSignal = watch::Sender<Option<u32>>;

/// Run the accept loop until stopped (Section 6: graceful shutdown).
///
/// Binds the Unix socket at `sock_path`, then accepts connections, spawning a
/// [`handle_connection`] task for each. Returns when `stop` flips (from a process shutdown or
/// a client `shutdown` request), or after a hard `hard_timeout` elapses (that only guards
/// against hangs; normal shutdown is via signal/`shutdown`).
///
/// The caller supplies the [`StopSignal`] *sender* (`stop_tx`) and a matching *receiver*
/// (`stop_rx`); the receiver is monitored here, the sender is forwarded to connection tasks.
pub async fn run_accept_loop(
    sock_path: PathBuf,
    mut stop_rx: watch::Receiver<Option<u32>>,
    stop_tx: StopSignal,
    store: Arc<Store>,
    repo_key: String,
    hard_timeout: Duration,
) -> std::io::Result<()> {
    // Monitor shutdown by watching the receiver (Section 6: graceful shutdown).

    let listener = UnixListener::bind(&sock_path)?;
    // Socket mode 0600 (Section 6): only the owning user may talk to the daemon.
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
    let mut pending: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    loop {
        tokio::select! {
            _ = stop_rx.changed() => break, // stop requested (Section 6: graceful shutdown).
            _ = tokio::time::sleep(hard_timeout) => {
                pending.clear(); // hard timeout reached: drop pending connections.
                break;
            }
            result = listener.accept() => {
                let (mut stream, _addr) = result?;
                let stop_tx = stop_tx.clone();
                let task_rx = stop_rx.clone();
                let store = store.clone();
                let repo_key = repo_key.clone();
                let handle = tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(30),
                        handle_connection(&mut stream, task_rx, stop_tx, &store, &repo_key),
                    )
                    .await;
                    let _ = stream.flush().await;
                });
                pending.push(handle);
                // Keep `stop_changed` fresh after handling one accept.
            }
        }
    }

    // Close the listener so clients that connect after shutdown get a clean broken pipe, then
    // drop pending connections (Section 6: graceful shutdown).
    drop(listener);
    let _ = std::fs::remove_file(&sock_path);
    for pending in pending {
        let _ = tokio::time::timeout(Duration::from_millis(200), pending).await;
    }
    Ok(())
}

/// Accept one connection and service requests from it until EOF or a `shutdown` request
/// (Section 6).
async fn handle_connection(
    stream: &mut UnixStream,
    stop_rx: watch::Receiver<Option<u32>>,
    stop_tx: StopSignal,
    store: &Store,
    repo_key: &str,
) -> std::io::Result<()> {
    // React to a `shutdown` request (or process shutdown) mid-connection by stopping after the
    // current request is answered. `stop_rx.changed()` fires when `stop_tx` flips.
    let mut stop_rx = stop_rx;
    loop {
        // Read a request line, but break early if we've been told to stop.
        let line = tokio::select! {
            r = read_line(stream) => r,
            _ = stop_rx.changed() => break,
        };
        let line = line?;
        let Some(line) = line else { break }; // EOF: the client closed the connection.

        let req: Request = match serde_json::from_str::<Request>(&line) {
            Ok(req) => req,
            Err(e) => {
                write_reply(
                    stream,
                    &json!({ "id": 1, "error": { "code": "invalid_json", "message": format!("malformed request: {e}") } }),
                )
                .await?;
                continue;
            }
        };
        if dispatch_request(
            stream,
            &req,
            stop_rx.clone(),
            stop_tx.clone(),
            store,
            repo_key,
        )
        .await?
        {
            break; // the client asked to shut down: answer and stop.
        }
    }
    Ok(())
}

/// Dispatch a single request, writing either a `result` or an `error` reply (Section 6).
///
/// Returns `true` when the daemon should stop servicing the connection (Section 6), `false`
/// otherwise.
async fn dispatch_request(
    stream: &mut UnixStream,
    req: &Request,
    _stop_rx: watch::Receiver<Option<u32>>,
    stop_tx: StopSignal,
    store: &Store,
    repo_key: &str,
) -> std::io::Result<bool> {
    match req.method {
        Method::Hello => {
            let proto = req.params.get("proto").and_then(Value::as_u64);
            match proto {
                Some(PROTO_VERSION) => {
                    write_reply(
                        stream,
                        &json!({ "id": req.id, "result": json!({ "proto": PROTO_VERSION, "daemon": VERSION }) }),
                    )
                    .await?;
                }
                Some(other) => {
                    write_reply(
                        stream,
                        &json!({ "id": req.id, "error": { "code": "incompatible_proto", "message": format!("daemon supports proto {PROTO_VERSION}; client sent {other}") } }),
                    )
                    .await?;
                }
                None => {
                    // No proto supplied: accept the current protocol (Section 6).
                    write_reply(
                        stream,
                        &json!({ "id": req.id, "result": json!({ "proto": PROTO_VERSION, "daemon": VERSION }) }),
                    )
                    .await?;
                }
            }
        }
        Method::Ping => write_reply(
            stream,
            &json!({ "id": req.id, "result": json!({ "pong": true }) }),
        )
        .await?,
        Method::Status => {
            let status = tokio::task::block_in_place(|| status_json(store, repo_key))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": status })).await?;
        }
        Method::Shutdown => {
            write_reply(stream, &json!({ "id": req.id, "result": json!({ "ok": true }) })).await?;
            // Signal the accept loop to exit (Section 6) and stop servicing this connection.
            stop_tx.send(Some(0)).ok();
            return Ok(true);
        }
        Method::Plans => {
            let result = tokio::task::block_in_place(|| plans_json(store))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": result })).await?;
        }
        Method::Plan => {
            match req.params.get("id").and_then(Value::as_str) {
                None => write_reply(
                    stream,
                    &json!({ "id": req.id, "error": {
                        "code": "invalid_params", "message": "missing 'id'" } }),
                )
                .await?,
                Some(id) => {
                    let found = tokio::task::block_in_place(|| plan_json(store, id))
                        .map_err(|e| std::io::Error::other(e.to_string()))?;
                    match found {
                        Some(result) => {
                            write_reply(
                                stream,
                                &json!({ "id": req.id, "result": result }),
                            )
                            .await?;
                        }
                        None => write_reply(
                            stream,
                            &json!({ "id": req.id, "error": {
                                "code": "not_found",
                                "message": format!("no plan '{}'", id) } }),
                        )
                        .await?,
                    }
                }
            }
        }
        other => write_reply(
            stream,
            &json!({ "id": req.id, "error": { "code": "not_implemented", "message": format!("method '{other}' is not yet implemented") } }),
        )
        .await?,
    }
    Ok(false)
}

/// Build the status object for `ra status` (Section 6) by querying the store synchronously.
fn status_json(store: &Store, repo_key: &str) -> Result<Value, crate::store::StoreError> {
    store.read(|conn| {
        let mut worktrees: Vec<Value> = {
            let mut stmt = conn.prepare(
                "SELECT wt.path, wt.branch, wt.head, wt.is_main, wt.present, \
                 COALESCE((SELECT COUNT(*) FROM worktree_files wf WHERE wf.worktree_id = wt.id), 0) AS changed, \
                 wt.last_activity_ms \
                 FROM worktrees wt ORDER BY wt.path",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(json!({
                    "path": row.get::<_, String>(0)?,
                    "branch": row.get::<_, Option<String>>(1)?,
                    "head": row.get::<_, Option<String>>(2)?,
                    "is_main": row.get::<_, bool>(3)?,
                    "present": row.get::<_, bool>(4)?,
                    "changed": row.get::<_, i64>(5)?,
                    "last_activity_ms": row.get::<_, Option<i64>>(6)?,
                }))
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };
        let active_plans: i64 = conn
            .query_row("SELECT COUNT(*) FROM plans WHERE state='active'", [], |r| r.get(0))?;
        let open_findings: i64 = conn
            .query_row("SELECT COUNT(*) FROM findings WHERE state='open'", [], |r| r.get(0))?;
        let sessions: i64 =
            conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        // Task 2.3: overlay per-worktree dirty status from the events table (persisted by
        // the bus). `store.worktree_dirty_view` opens its own read handle, so it is safe to
        // call here inside the read closure.
        let dirty: std::collections::HashMap<String, (i64, Option<i64>)> =
            store.worktree_dirty_view().unwrap_or_default();
        for row in worktrees.iter_mut() {
            let p: Option<&str> = row.get("path").and_then(|v| v.as_str());
            if let Some((count, last)) = p.and_then(|s| dirty.get(s)).copied() {
                let obj = row.as_object_mut().expect("worktree row is an object");
                obj.insert("dirty".into(), json!(count > 0));
                obj.insert("dirty_events".into(), json!(count));
                obj.insert(
                    "last_dirty_ms".into(),
                    Value::from(last),
                );
            }
        }
        Ok(json!({
            "daemon": TOOL_NAME,
            "version": VERSION,
            "repo_key": repo_key,
            "worktrees": worktrees,
            "worktree_count": worktrees.len(),
            "active_plans": active_plans,
            "open_findings": open_findings,
            "sessions": sessions,
        }))
    })
}

/// Build the `Method::Plans` result: one summary row per plan (Task 2.4).
fn plans_json(store: &Store) -> Result<Value, crate::store::StoreError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.source, p.title, p.file, p.worktree_id, p.state, p.first_seen_ms,              p.last_seen_ms, COALESCE(COUNT(t.id), 0) AS task_count,              COALESCE(SUM(CASE WHEN t.declared = 'done' THEN 1 ELSE 0 END), 0) AS declared_done,              COALESCE(SUM(CASE WHEN t.observed = 'done' THEN 1 ELSE 0 END), 0) AS observed_done              FROM plans p LEFT JOIN tasks t ON t.plan_id = p.id              GROUP BY p.id ORDER BY p.last_seen_ms DESC",
        )?;
        let mut plans: Vec<Value> = Vec::new();
        for r in stmt.query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "title": row.get::<_, String>(2)?,
                "file": row.get::<_, Option<String>>(3)?,
                "worktree_id": row.get::<_, Option<i64>>(4)?,
                "state": row.get::<_, String>(5)?,
                "first_seen_ms": row.get::<_, Option<i64>>(6)?,
                "last_seen_ms": row.get::<_, Option<i64>>(7)?,
                "task_count": row.get::<_, i64>(8)?,
                "declared_done": row.get::<_, i64>(9)?,
                "observed_done": row.get::<_, i64>(10)?,
            }))
        })? {
            plans.push(r?);
        }
        Ok(json!({ "plans": plans }))
    })
}

/// Build the `Method::Plan` result: the plan by id with its tasks (ordered by ordinal) and
/// each task's evidence (ordered by ts_ms). Returns `None` when no plan has that id.
fn plan_json(store: &Store, id: &str) -> Result<Option<Value>, crate::store::StoreError> {
    store.read(|conn| {
        let plan = conn.query_row(
            "SELECT id, source, title, file, worktree_id, state, first_seen_ms, last_seen_ms              FROM plans WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )?;
        let (id, source, title, file, worktree_id, state, first_seen_ms, last_seen_ms) =
            plan;
        // Tasks for this plan, ordered by ordinal.
        let mut task_stmt = conn.prepare(
            "SELECT id, ordinal, title, declared, expected_paths, depends_on, observed, observed_reason \
             FROM tasks WHERE plan_id = ?1 ORDER BY ordinal",
        )?;
        let mut tasks: Vec<Value> = Vec::new();
        for t in task_stmt.query_map([id.to_string()], |row| {
            let expected: String = row.get(4)?;
            let depends: String = row.get(5)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "ordinal": row.get::<_, i64>(1)?,
                "title": row.get::<_, String>(2)?,
                "declared": row.get::<_, String>(3)?,
                "expected_paths": serde_json::from_str(&expected).unwrap_or(Value::Array(Vec::new())),
                "depends_on": serde_json::from_str(&depends).unwrap_or(Value::Array(Vec::new())),
                "observed": row.get::<_, String>(6)?,
                "observed_reason": row.get::<_, Option<String>>(7)?,
            }))
        })? {
            let mut task = t?;
            let task_id: String = task["id"]
                .as_str()
                .unwrap_or("")
                .to_string();
            // Evidence for this task, ordered by ts_ms.
            let mut ev_stmt = conn.prepare(
                "SELECT id, kind, ref, ts_ms, confidence FROM evidence WHERE task_id = ?1 ORDER BY ts_ms",
            )?;
            let mut evidence: Vec<Value> = Vec::new();
            for e in ev_stmt.query_map([task_id.as_str()], |row| {
                Ok(json!({
                    "id": row.get::<_, i64>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "ref": row.get::<_, String>(2)?,
                    "ts_ms": row.get::<_, i64>(3)?,
                    "confidence": row.get::<_, String>(4)?,
                }))
            })? {
                evidence.push(e?);
            }
            if let Some(obj) = task.as_object_mut() {
                obj.insert("evidence".into(), json!(evidence));
            }
            tasks.push(task);
        }
        Ok(Some(json!({
            "id": id,
            "source": source,
            "title": title,
            "file": file,
            "worktree_id": worktree_id,
            "state": state,
            "first_seen_ms": first_seen_ms,
            "last_seen_ms": last_seen_ms,
            "tasks": tasks,
        })))
    })
}

/// Read a single JSON response line (Section 6), terminated by `\n`.
async fn write_reply(stream: &mut UnixStream, value: &Value) -> std::io::Result<()> {
    let line =
        serde_json::to_string(value).map_err(|e| std::io::Error::other(format!("json: {e}")))?;
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    Ok(())
}

/// Read one newline-delimited line from the stream (Section 6: one request per line).
///
/// Returns `Ok(None)` on EOF, `Ok(Some(line))` for a complete line, and `Err` on transport
/// failure. Lines are read in 1 KiB chunks; the first `\n` terminates the line. Multi-byte
/// UTF-8 never crosses a `\n`, so searching for the byte `0x0A` is safe.
async fn read_line(stream: &mut UnixStream) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        match buf[..n].iter().position(|&b| b == b'\n') {
            Some(pos) => {
                line.push_str(std::str::from_utf8(&buf[..pos]).unwrap_or_default());
                return Ok(Some(line));
            }
            None => line.push_str(std::str::from_utf8(&buf[..n]).unwrap_or_default()),
        }
    }
}
