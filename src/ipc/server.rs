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

use crate::checks::CheckCtx;
use crate::config::Config;
use crate::ipc::{Method, Request, PROTO_VERSION};
use crate::store::{FindingsFilter, Store, WtInfo};
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
    config: Config,
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
                let config = config.clone();
                let handle = tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(30),
                        handle_connection(&mut stream, task_rx, stop_tx, &store, &repo_key, config),
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
    config: Config,
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
            &config,
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
    config: &Config,
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
        Method::Drift => {
            let drift = tokio::task::block_in_place(|| plan_drift_value(store))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": drift })).await?;
        }
        Method::Sessions => {
            let result = tokio::task::block_in_place(|| sessions_json(store))
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
        Method::Worktrees => {
            let result = tokio::task::block_in_place(|| worktrees_json(store))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": result })).await?;
        }
        Method::Conflicts => {
            let result = tokio::task::block_in_place(|| conflicts_json(store, config))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": result })).await?;
        }
        Method::WhoTouches => {
            let target = req.params.get("path").and_then(Value::as_str).unwrap_or("");
            let result = tokio::task::block_in_place(|| who_touches_json(store, target))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": result })).await?;
        }
        Method::Findings => {
            match tokio::task::block_in_place(|| findings_json(store, &req.params)) {
                Ok(result) => write_reply(stream, &json!({ "id": req.id, "result": result })).await?,
                Err(e) => write_reply(stream, &json!({ "id": req.id, "error": { "code": "store_error", "message": e.to_string() } })).await?,
            }
        }
        Method::Ack => {
            match tokio::task::block_in_place(|| ack_json(store, &req.params)) {
                Ok(result) => write_reply(stream, &json!({ "id": req.id, "result": result })).await?,
                Err(e) => write_reply(stream, &json!({ "id": req.id, "error": { "code": "store_error", "message": e.to_string() } })).await?,
            }
        }
        Method::RenderViews => {
            let render_result = {
                let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                tokio::task::block_in_place(|| render_views_json(store, repo_key, config))
            };
            match render_result {
                Ok(result) => write_reply(stream, &json!({ "id": req.id, "result": result })).await?,
                Err(e) => {
                    let code = match e {
                        crate::views::write::ViewError::Io(_) => "io_error",
                        crate::views::write::ViewError::Store(_) => "store_error",
                    };
                    write_reply(stream, &json!({ "id": req.id, "error": { "code": code, "message": e.to_string() } })).await?
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

/// Serialises view renders across connections (M2: prevent interleaved tmp writes).
static RENDER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Render views from store data and write atomically; return written paths.
pub fn render_views_json(
    store: &Store,
    repo_key: &str,
    config: &Config,
) -> Result<Value, crate::views::write::ViewError> {
    use crate::views::write::write_views;
    use crate::views::{render_findings, render_plans, render_status, ViewData};
    if !config.views.enabled {
        return Ok(json!({ "written": [] }));
    }
    let status = status_json(store, repo_key)
        .map_err(|e| crate::views::write::ViewError::Store(e.to_string()))?;
    let plans =
        plans_json(store).map_err(|e| crate::views::write::ViewError::Store(e.to_string()))?;
    let findings = findings_json(store, &Value::Null)
        .map_err(|e| crate::views::write::ViewError::Store(e.to_string()))?;
    let sessions =
        sessions_json(store).map_err(|e| crate::views::write::ViewError::Store(e.to_string()))?;
    let d = ViewData {
        status,
        plans,
        findings,
        sessions,
    };
    let now = crate::sources::git::now_ms();
    let rendered = vec![
        ("STATUS.md", render_status(&d, now)),
        ("PLANS.md", render_plans(&d, now)),
        ("FINDINGS.md", render_findings(&d, now)),
    ];
    let dir = crate::paths::Env::from_real().views_dir(repo_key);
    let written = write_views(&dir, &rendered)?;
    Ok(json!({ "written": written }))
}

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
                if let Some(obj) = row.as_object_mut() {
                    obj.insert("dirty".into(), json!(count > 0));
                    obj.insert("dirty_events".into(), json!(count));
                    obj.insert(
                        "last_dirty_ms".into(),
                        Value::from(last),
                    );
                }
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

/// Build the `Method::Sessions` result (Task 4): every session with its worktree path (when
/// mapped), most recently active first. Pure store read — the daemon's sessions ticker
/// keeps the rows fresh, mirroring how `plans_json` reads what the daemon ingested.
fn sessions_json(store: &Store) -> Result<serde_json::Value, crate::store::StoreError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.agent, s.file, s.cwd, s.worktree_id, wt.path,              s.started_ms, s.last_event_ms, s.tokens_in, s.tokens_out, s.tokens_cache,              s.cost_usd, s.tool_calls, s.byte_offset, s.parse_errors              FROM sessions s LEFT JOIN worktrees wt ON wt.id = s.worktree_id              ORDER BY COALESCE(s.last_event_ms, s.started_ms, 0) DESC, s.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "agent": row.get::<_, String>(1)?,
                "file": row.get::<_, String>(2)?,
                "cwd": row.get::<_, Option<String>>(3)?,
                "worktree_id": row.get::<_, Option<i64>>(4)?,
                "worktree": row.get::<_, Option<String>>(5)?,
                "started_ms": row.get::<_, Option<i64>>(6)?,
                "last_event_ms": row.get::<_, Option<i64>>(7)?,
                "tokens_in": row.get::<_, i64>(8)?,
                "tokens_out": row.get::<_, i64>(9)?,
                "tokens_cache": row.get::<_, i64>(10)?,
                "cost_usd": row.get::<_, Option<f64>>(11)?,
                "tool_calls": row.get::<_, i64>(12)?,
                "byte_offset": row.get::<_, i64>(13)?,
                "parse_errors": row.get::<_, i64>(14)?,
            }))
        })?;
        let mut sessions = Vec::new();
        for r in rows {
            sessions.push(r?);
        }
        Ok(serde_json::json!({ "sessions": sessions }))
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

/// Build the `Method::Drift` result (Task 3): every drifted file across all plans.
///
/// For each plan the daemon recomputes its worktree's changed-file set from live `git`
/// output (via [`crate::sources::git::changed_file_set`]) and asks the store which changed
/// paths no task expects (see [`Store::plan_drift_from_changed`]). Plans with no worktree are
/// skipped. Output is ordered by plan id, then path (stable for tests).
fn plan_drift_value(store: &Store) -> Result<Value, crate::store::StoreError> {
    use crate::sources::git::changed_file_set;
    let refs = store.plan_worktrees()?;

    // Drift per plan: (plan_id, worktree_path, drifted_rows). Ordering is by plan id.
    let mut per_plan: Vec<(String, String, Vec<crate::store::DriftFile>)> = Vec::new();
    for r in refs {
        // Skip plans that own no worktree: there is nothing to drift against.
        let Some(path) = r.path else {
            continue;
        };
        let changed = changed_file_set(
            std::path::Path::new(&path),
            r.branch.as_deref(),
            r.base_ref.as_deref(),
        );
        if changed.is_empty() {
            continue;
        }
        match store.plan_drift_from_changed(&r.plan_id, &changed) {
            Ok(Some(drift)) => {
                if !drift.is_empty() {
                    per_plan.push((r.plan_id, path, drift));
                }
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }
    per_plan.sort_by(|a, b| a.0.cmp(&b.0));

    // Emit the drifted files as a flat array, each annotated with its owning plan + worktree.
    let rows: Vec<Value> = per_plan
        .into_iter()
        .flat_map(|(plan_id, worktree, drift)| {
            drift.into_iter().map(move |f| {
                json!({
                    "plan": plan_id.clone(),
                    "worktree": worktree.clone(),
                    "path": f.path,
                    "origin": f.origin,
                })
            })
        })
        .collect();
    Ok(json!({ "drift": rows }))
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

/// Build the `Method::Worktrees` result: every worktree with its branch, head, presence, and
/// changed-file count (recomputed live from `git` by `check_workspace`).
fn worktrees_json(store: &Store) -> Result<Value, crate::store::StoreError> {
    let ws = store.check_workspace()?;
    let out: Vec<Value> = ws
        .worktrees
        .iter()
        .map(|w| {
            json!({
                "path": w.path,
                "branch": w.branch,
                "head": w.head,
                "is_main": w.is_main,
                "present": w.present,
                "changed": w.changed.len(),
            })
        })
        .collect();
    Ok(json!({ "worktrees": out }))
}

/// Build the `Method::Conflicts` result: worktree overlaps, branch-head merge conflicts, and
/// overlaps between active plans. Severity `error` (merge conflict) or `warn` (overlap).
/// Read-only: runs the checks and keeps only the conflict kinds, never writes to the store.
fn conflicts_json(store: &Store, config: &Config) -> Result<Value, crate::store::StoreError> {
    let ws = store.check_workspace()?;
    let now = crate::sources::git::now_ms();
    let ctx = CheckCtx {
        workspace: &ws,
        config,
        now,
    };
    let candidates = crate::checks::run_all_checks(&ctx);
    let conflicts: Vec<Value> = candidates
        .iter()
        .filter(|c| {
            matches!(
                c.kind.as_str(),
                "worktree.overlap" | "worktree.merge_conflict" | "plan.overlap"
            )
        })
        .map(|c| {
            json!({
                "kind": c.kind,
                "severity": c.severity,
                "subject": c.subject,
                "summary": c.summary,
                "dedupe_key": c.dedupe_key,
                "evidence": c.evidence,
            })
        })
        .collect();
    Ok(json!({ "conflicts": conflicts }))
}

/// Build the `Method::WhoTouches` result: worktrees, plans, and sessions currently touching
/// `target` (a repo-relative path). Read-only: rebuilds the workspace and matches each
/// entity's changed set / expected paths / worktree against the target.
fn who_touches_json(store: &Store, target: &str) -> Result<Value, crate::store::StoreError> {
    let ws = store.check_workspace()?;
    let by_path: std::collections::HashMap<String, &WtInfo> =
        ws.worktrees.iter().map(|w| (w.path.clone(), w)).collect();
    let norm = target.trim_end_matches('/').to_string();
    let dir = if norm.is_empty() {
        String::new()
    } else {
        format!("{}/", norm)
    };
    let path_touches = |p: &str| *p == *target || (!dir.is_empty() && p.starts_with(&dir));
    let touches =
        |changed: &[(String, String)]| -> bool { changed.iter().any(|(p, _)| path_touches(p)) };

    let worktrees: Vec<Value> = ws
        .worktrees
        .iter()
        .filter(|w| touches(&w.changed))
        .map(|w| {
            json!({
                "kind": "worktree",
                "path": w.path,
                "branch": w.branch,
                "changed": w.changed.len(),
            })
        })
        .collect();

    // Plans touch the target when any of their tasks' expected paths includes it, or when
    // the plan's own worktree changed set includes it.
    let plans: Vec<Value> = ws
        .plans
        .iter()
        .filter_map(|plan| {
            let expected: Vec<String> = ws
                .tasks
                .iter()
                .filter(|t| t.plan_id == plan.id)
                .flat_map(|t| t.expected_paths.iter().cloned())
                .collect();
            let touched_by_expected = expected.iter().any(|p| path_touches(p));
            let touched_by_wt = ws
                .worktrees
                .iter()
                .any(|w| plan.worktree_id == Some(w.id) && touches(&w.changed));
            if touched_by_expected || touched_by_wt {
                Some(json!({ "kind": "plan", "id": plan.id, "title": plan.title, "state": plan.state }))
            } else {
                None
            }
        })
        .collect();

    // Sessions touch the target when their own worktree's changed set includes it.
    let sessions_out: Vec<Value> = store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.agent, wt.path FROM sessions s LEFT JOIN worktrees wt ON wt.id = s.worktree_id",
        )?;
        let rows: Vec<(String, String, Option<String>)> = stmt
            .query_map(
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get(2)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (id, agent, wpath) in rows {
            if let Some(wp) = wpath {
                if let Some(w) = by_path.get(&wp) {
                    if touches(&w.changed) {
                        out.push(json!({ "kind": "session", "id": id, "agent": agent }));
                    }
                }
            }
        }
        Ok(out)
    })?;

    Ok(json!({
        "worktrees": worktrees,
        "plans": plans,
        "sessions": sessions_out,
    }))
}

/// Build the `Method::Findings` result: the persisted findings filtered by state and kind.
/// The daemon's findings runner keeps the table current, so this only reads.
fn findings_json(store: &Store, params: &Value) -> Result<Value, crate::store::StoreError> {
    let open = params.get("open").and_then(Value::as_bool).unwrap_or(false);
    let all = params.get("all").and_then(Value::as_bool).unwrap_or(false);
    let kind = params
        .get("kind")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let filter = FindingsFilter { open, all, kind };
    let rows = store.list_findings(&filter)?;
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "kind": r.kind,
                "severity": r.severity,
                "subject": r.subject,
                "summary": r.summary,
                "state": r.state,
                "dedupe_key": r.dedupe_key,
                "confidence": r.confidence,
                "evidence": r.evidence,
                "first_seen_ms": r.first_seen_ms,
                "last_seen_ms": r.last_seen_ms,
                "resolved_ms": r.resolved_ms,
            })
        })
        .collect();
    Ok(json!({ "findings": out }))
}

/// Build the `Method::Ack` result: mark one finding `acked` (by store id).
fn ack_json(store: &Store, params: &Value) -> Result<Value, crate::store::StoreError> {
    let id = if let Some(s) = params.get("id").and_then(Value::as_str) {
        s.to_string()
    } else if let Some(n) = params.get("id").and_then(Value::as_i64) {
        n.to_string()
    } else {
        String::new()
    };
    let now = crate::sources::git::now_ms();
    let ok = store.ack_finding(&id, now)?;
    Ok(json!({ "ok": ok, "id": id }))
}
