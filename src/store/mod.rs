//! SQLite-backed single source of truth.
//!
//! A [`Store`] opens a database in WAL mode, applies schema migrations via
//! [`crate::store::migrations`], and guards a single writer connection with a [`std::sync::Mutex`].
//! Read queries take additional read handles. All DB work happens on these blocking handles
//! and must stay off the async runtime threads (Section 4).

pub(crate) mod migrations;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// Errors surfaced by the store layer (thiserror in library modules).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Database file is missing when a migration was required.
    #[error("database file {0:?} does not exist")]
    MissingDatabase(String),
    /// Existing database has a newer `user_version` than this crate can open.
    #[error("database schema version {0} is newer than supported version {1}")]
    NewerVersion(u32, u32),
    /// SQLite error.
    #[error("sqlite error: {0}")]
    Database(#[from] rusqlite::Error),
    /// The single writer connection mutex was poisoned by a previous panic.
    #[error("store writer mutex poisoned")]
    Poisoned,
}

/// The schema version applied to every freshly created database.
pub const SCHEMA_VERSION: u32 = migrations::CURRENT_VERSION;

/// Options controlling how a [`Store`] opens a database.
#[derive(Debug, Clone, Default)]
pub struct StoreOptions {
    /// When true, a missing file is treated as "nothing to open" rather than an error.
    pub allow_missing: bool,
}

/// A read/write database connection handle.
#[derive(Debug, Clone)]
pub struct ConnectionHandle {
    /// Absolute path of the database file.
    pub path: PathBuf,
    /// Whether the file existed before opening.
    pub existed: bool,
    /// `user_version` after opening (== [`SCHEMA_VERSION`] for a normal handle).
    pub version: u32,
}

/// A database connection (single writer behind a [`Mutex`], plus read handles).
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    writer: Arc<Mutex<rusqlite::Connection>>,
    version: u32,
    existed: bool,
}

/// A synchronous write transaction against a [`Store`].
pub type WriteTxn<'a> = rusqlite::Transaction<'a>;

/// One row to upsert into `plans` (Task 2.4). Keyed on `id`.
#[derive(Debug, Clone)]
pub struct PlanRow<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub title: &'a str,
    pub file: Option<&'a str>,
    pub worktree_id: Option<i64>,
    pub content_hash: Option<&'a str>,
    pub state: &'a str,
}

/// One row to upsert into `tasks` (Task 2.4). Keyed on `id`.
#[derive(Debug, Clone)]
pub struct TaskRow<'a> {
    pub id: &'a str,
    pub plan_id: &'a str,
    pub ordinal: i64,
    pub title: &'a str,
    pub declared: &'a str,
    /// Serialized to the `expected_paths` JSON array column (see `upsert_task`).
    pub expected_paths: &'a [String],
    /// Serialized to the `depends_on` JSON array column.
    pub depends_on: &'a [String],
}

/// One row to append to `evidence` (Task 2.4). No natural key — always inserted, never
/// upserted, so the full history is preserved under its `ON DELETE CASCADE` FK.
#[derive(Debug, Clone)]
pub struct EvidenceRow<'a> {
    pub task_id: &'a str,
    pub kind: &'a str,
    pub reference: &'a str, // `ref` is a Rust keyword; the column is still named `ref`
    pub ts_ms: i64,
    pub confidence: &'a str,
}

/// One row to upsert into `sessions` (Task 4). Keyed on `id`; a re-ingest replaces every
/// column (the session source fully recomputes aggregates on parse).
#[derive(Debug, Clone)]
pub struct SessionRow<'a> {
    pub id: &'a str,
    pub agent: &'a str,
    pub file: &'a str,
    pub cwd: Option<&'a str>,
    pub worktree_id: Option<i64>,
    pub started_ms: Option<i64>,
    pub last_event_ms: Option<i64>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache: i64,
    pub cost_usd: Option<f64>,
    pub tool_calls: i64,
    pub byte_offset: i64,
    pub parse_errors: i64,
}

/// One `tool_calls` row belonging to one session (Task 4): a single assistant tool-call
/// block, optionally enriched with its result matched during the same parse pass.
#[derive(Debug, Clone)]
pub struct ToolCallRow<'a> {
    pub ts_ms: i64,
    pub tool: &'a str,
    pub args_hash: &'a str,
    pub is_error: Option<bool>,
    pub result_hash: Option<&'a str>,
}

impl Store {
    /// Open (or create and migrate) the database at `path`.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, StoreError> {
        Self::open_with(path, StoreOptions::default())
    }

    /// Open (or create and migrate) the database, honouring [`StoreOptions`].
    pub fn open_with(
        path: impl AsRef<std::path::Path>,
        opts: StoreOptions,
    ) -> Result<Self, StoreError> {
        let path: PathBuf = path.as_ref().to_path_buf();
        let existed = path.exists();
        if !existed {
            if opts.allow_missing {
                return Err(StoreError::MissingDatabase(path.display().to_string()));
            }
            // Default behavior: create the file (empty Connection::open makes it exist with
            // user_version 0), then migrate. migrate sees user_version 0 < current and applies
            // schema.sql, so tables are present and user_version becomes SCHEMA_VERSION.
            let conn = rusqlite::Connection::open(&path).map_err(StoreError::Database)?;
            migrations::migrate(&path)?;
            let version: u32 = migrations::current_user_version(&path)?;
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
                .map_err(StoreError::Database)?;
            return Ok(Store {
                path,
                version,
                existed: false,
                writer: Arc::new(Mutex::new(conn)),
            });
        }
        match migrations::migrate(&path)? {
            migrations::InitResult::Created => {}
            migrations::InitResult::UpToDate => {}
        }
        let conn = rusqlite::Connection::open(&path).map_err(StoreError::Database)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(StoreError::Database)?;
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(StoreError::Database)?;
        Ok(Store {
            path,
            writer: Arc::new(Mutex::new(conn)),
            version,
            existed: true,
        })
    }

    /// Absolute path of the database file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// `user_version` of the opened database.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Whether the database file pre-existed this connection.
    pub fn existed(&self) -> bool {
        self.existed
    }

    /// A read/write connection handle describing this store.
    pub fn handle(&self) -> ConnectionHandle {
        ConnectionHandle {
            path: self.path.clone(),
            existed: self.existed,
            version: self.version,
        }
    }

    /// Run a read query callback using a separate read-only connection handle.
    pub fn read<F, T>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error>,
    {
        let conn = rusqlite::Connection::open_with_flags(
            &self.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(StoreError::Database)?;
        f(&conn).map_err(StoreError::Database)
    }

    /// Run a write callback against the guarded writer connection.
    pub fn write<F, T>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error>,
    {
        let lock = self.writer.lock().map_err(|_| StoreError::Poisoned)?;
        // Centralize atomicity (F1/F2): wrap the entire closure in a single transaction.
        // `unchecked_transaction` (BEGIN DEFERRED) is safe here — one writer behind the
        // mutex in WAL mode; readers use separate handles. No closure nests `store.write`
        // and none runs VACUUM/PRAGMA journal_mode, so nesting is never attempted.
        let tx = lock.unchecked_transaction().map_err(StoreError::Database)?;
        match f(&tx) {
            Ok(v) => {
                tx.commit().map_err(StoreError::Database)?;
                Ok(v)
            }
            Err(e) => Err(StoreError::Database(e)), // tx drops -> rollback
        }
    }

    /// Persist one row to the `events` table and return its row id.
    ///
    /// `source`/`kind` categorise the event; `subject` is a free-form subject (e.g. a
    /// worktree path); `payload` is a JSON string.
    pub fn append_event(
        &self,
        ts_ms: i64,
        source: &str,
        kind: &str,
        subject: Option<&str>,
        payload: &str,
    ) -> Result<i64, StoreError> {
        self.write(|conn| {
            conn.execute(
                "INSERT INTO events(ts_ms, source, kind, subject, payload)                  VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![ts_ms, source, kind, subject, payload],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// Recent per-worktree dirty status derived from the `events` table: for each worktree
    /// path that has a `worktree_dirty` event, `(event_count, last_event_ms)`.
    pub fn worktree_dirty_view(
        &self,
    ) -> Result<std::collections::HashMap<String, (i64, Option<i64>)>, StoreError> {
        self.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT subject, COUNT(*), MAX(ts_ms) FROM events                  WHERE source = 'worktree' AND kind = 'worktree_dirty'                  GROUP BY subject",
            )?;
            let mut out = std::collections::HashMap::new();
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, i64>(1)?, row.get::<_, Option<i64>>(2)?),
                ))
            })?;
            for r in rows {
                let (path, (count, last)) = r?;
                out.insert(path, (count, last));
            }
            Ok(out)
        })
    }

    /// Persist one `plans` row, keyed on `id` (Task 2.4).
    ///
    /// `first_seen_ms` is set on first insert and left untouched on conflict; `last_seen_ms`
    /// is always refreshed to `now_ms`.
    pub fn upsert_plan(&self, row: &PlanRow<'_>, now_ms: i64) -> Result<(), StoreError> {
        self.write(|conn| {
            conn.execute(
                "INSERT INTO plans(id, source, title, file, worktree_id, content_hash, state, first_seen_ms, last_seen_ms)                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(id) DO UPDATE SET source = excluded.source, title = excluded.title, file = excluded.file, worktree_id = excluded.worktree_id, content_hash = excluded.content_hash, state = excluded.state, last_seen_ms = excluded.last_seen_ms",
                rusqlite::params![
                    row.id,
                    row.source,
                    row.title,
                    row.file,
                    row.worktree_id,
                    row.content_hash,
                    row.state,
                    now_ms,
                    now_ms,
                ],
            )?;
            Ok(())
        })
    }

    /// Persist one `tasks` row, keyed on `id` (Task 2.4).
    ///
    /// Deliberately does NOT touch `observed`/`observed_reason` on conflict — those columns
    /// are owned by the (future) drift/derive step, not by whatever calls `upsert_task`.
    pub fn upsert_task(&self, row: &TaskRow<'_>) -> Result<(), StoreError> {
        let expected =
            serde_json::to_string(row.expected_paths).unwrap_or_else(|_| "[]".to_string());
        let depends_on = serde_json::to_string(row.depends_on).unwrap_or_else(|_| "[]".to_string());
        self.write(|conn| {
            conn.execute(
                "INSERT INTO tasks(id, plan_id, ordinal, title, declared, expected_paths, depends_on)                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO UPDATE SET ordinal = excluded.ordinal, title = excluded.title, declared = excluded.declared, expected_paths = excluded.expected_paths, depends_on = excluded.depends_on",
                rusqlite::params![
                    row.id,
                    row.plan_id,
                    row.ordinal,
                    row.title,
                    row.declared,
                    expected,
                    depends_on,
                ],
            )?;
            Ok(())
        })
    }

    /// Append one `evidence` row (Task 2.4); returns its row id. No natural key — always
    /// inserted, so re-importing a plan must not overwrite existing evidence.
    pub fn append_evidence(&self, row: &EvidenceRow<'_>) -> Result<i64, StoreError> {
        self.write(|conn| {
            conn.execute(
                "INSERT INTO evidence(task_id, kind, ref, ts_ms, confidence)                  VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    row.task_id,
                    row.kind,
                    row.reference,
                    row.ts_ms,
                    row.confidence,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// Set a task's `observed` (and `observed_reason`) value. Single-writer transaction.
    ///
    /// Owns the `tasks.observed` / `tasks.observed_reason` columns — callers that recompute
    /// drift should use this rather than poking at the row directly.
    pub fn set_task_observed(
        &self,
        task_id: &str,
        observed: &str,
        reason: Option<&str>,
    ) -> Result<(), StoreError> {
        self.write(|conn| {
            conn.execute(
                "UPDATE tasks SET observed = ?2, observed_reason = ?3 WHERE id = ?1",
                rusqlite::params![task_id, observed, reason],
            )?;
            Ok(())
        })
    }

    /// Remove evidence rows for `task_id` of the given `kind`, keeping all other kinds.
    /// Single-writer transaction. Used before re-appending derived commit evidence so re-sync
    /// does not accumulate duplicates.
    pub fn clear_evidence(&self, task_id: &str, kind: &str) -> Result<(), StoreError> {
        self.write(|conn| {
            conn.execute(
                "DELETE FROM evidence WHERE task_id = ?1 AND kind = ?2",
                rusqlite::params![task_id, kind],
            )?;
            Ok(())
        })
    }
    /// Upsert one `sessions` row and REPLACE its `tool_calls` rows.
    ///
    /// All statements run inside a single `unchecked_transaction` on the writer
    /// connection (supplied by `Store::write`), committed only at the end: a failure mid-way rolls back the session
    /// row (including byte_offset) together with its tool_calls, so readers never see
    /// a session marked up-to-date with stale/missing tool_calls. A file rewritten
    /// with a different session id drops the previous row (`file` match).
    pub fn upsert_session_with_calls(
        &self,
        row: &SessionRow<'_>,
        calls: &[ToolCallRow<'_>],
    ) -> Result<(), StoreError> {
        self.write(|conn| {
            conn.execute(
                "INSERT INTO sessions(id, agent, file, cwd, worktree_id, started_ms, last_event_ms, tokens_in, tokens_out, tokens_cache, cost_usd, tool_calls, byte_offset, parse_errors) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(id) DO UPDATE SET agent=excluded.agent, file=excluded.file, cwd=excluded.cwd, worktree_id=excluded.worktree_id, started_ms=excluded.started_ms, last_event_ms=excluded.last_event_ms, tokens_in=excluded.tokens_in, tokens_out=excluded.tokens_out, tokens_cache=excluded.tokens_cache, cost_usd=excluded.cost_usd, tool_calls=excluded.tool_calls, byte_offset=excluded.byte_offset, parse_errors=excluded.parse_errors",
                rusqlite::params![
                    row.id, row.agent, row.file, row.cwd, row.worktree_id,
                    row.started_ms, row.last_event_ms, row.tokens_in, row.tokens_out,
                    row.tokens_cache, row.cost_usd, row.tool_calls, row.byte_offset,
                    row.parse_errors,
                ],
            )?;
            // A rewritten file may carry a different session id: clean up stale rows' calls first.
            conn.execute(
                "DELETE FROM tool_calls WHERE session_id IN (SELECT id FROM sessions WHERE file = ?1 AND id <> ?2)",
                rusqlite::params![row.file, row.id],
            )?;
            conn.execute(
                "DELETE FROM sessions WHERE file = ?1 AND id <> ?2",
                rusqlite::params![row.file, row.id],
            )?;
            conn.execute("DELETE FROM tool_calls WHERE session_id = ?1", [row.id])?;
            for c in calls {
                conn.execute(
                    "INSERT INTO tool_calls(session_id, ts_ms, tool, args_hash, is_error, result_hash) VALUES (?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![row.id, c.ts_ms, c.tool, c.args_hash, c.is_error, c.result_hash],
                )?;
            }
            Ok(())
        })
    }

    /// `sessions.id -> byte_offset` map — the skip cursor the session source consults.
    pub fn session_file_offsets(
        &self,
    ) -> Result<std::collections::HashMap<String, i64>, StoreError> {
        self.read(|conn| {
            let mut stmt = conn.prepare("SELECT id, byte_offset FROM sessions")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            rows.collect::<Result<std::collections::HashMap<_, _>, _>>()
        })
    }

    /// One drifted file (Task 3): a path that changed in the plan's worktree but that no
    /// task *expects* (`tasks.expected_paths`).
    ///
    /// See [`DriftFile`].
    ///
    /// The drift set for one plan (Task 3): the changed paths that no task expects.
    ///
    /// `changed` is the plan's worktree changed-file set (the `(path, origin)` pairs the git
    /// source produces, e.g. [`crate::sources::git::changed_file_set`]). A path drifts when it
    /// is not in the union of the plan's tasks' `expected_paths` (`tasks.expected_paths`, each
    /// stored as a JSON array). Returns `None` when the plan does not exist; otherwise the
    /// drifted rows, ordered by path (stable output for tests).
    pub fn plan_drift_from_changed(
        &self,
        plan_id: &str,
        changed: &[(String, String)],
    ) -> Result<Option<Vec<DriftFile>>, StoreError> {
        self.read(|conn| {
            // The plan must exist; a plan row with no id of its own cannot drift.
            let exists: i64 =
                conn.query_row("SELECT COUNT(*) FROM plans WHERE id = ?1", [plan_id], |r| {
                    r.get(0)
                })?;
            if exists == 0 {
                return Ok(None);
            }

            // Union of the paths this plan's tasks expect (each `expected_paths` is a JSON
            // array stored in `tasks.expected_paths`).
            let mut expected: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            let mut task_stmt =
                conn.prepare("SELECT expected_paths FROM tasks WHERE plan_id = ?1")?;
            // Flatten the read handle: rows that fail to decode as a string are skipped.
            for s in task_stmt
                .query_map([plan_id], |r| r.get::<_, String>(0))?
                .flatten()
            {
                if let Ok(paths) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(arr) = paths.as_array() {
                        for v in arr {
                            if let Some(p) = v.as_str() {
                                expected.insert(p.to_string());
                            }
                        }
                    }
                }
            }

            // Drift = changed paths not in the expected set, ordered by path.
            let mut changed = changed.to_vec();
            changed.sort_by(|a, b| a.0.cmp(&b.0));
            let drift = changed
                .into_iter()
                .filter(|(path, _)| !expected.contains(path))
                .map(|(path, origin)| DriftFile { path, origin })
                .collect();
            Ok(Some(drift))
        })
    }

    /// Every known worktree as `(id, path)`. Plan discovery scans these (not only worktrees
    /// that already own a plan), otherwise the first plan could never be discovered.
    pub fn worktree_id_paths(&self) -> Result<Vec<(i64, String)>, StoreError> {
        self.read(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, path FROM worktrees WHERE present = 1 ORDER BY id")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        })
    }

    /// The plan-to-worktree references the daemon drifts against (Task 3): one entry per
    /// plan, paired with the worktree `plans.worktree_id` points at (path, branch, base ref).
    ///
    /// The daemon recomputes each worktree's changed-file set from live `git` output
    /// (`crate::sources::git::changed_file_set`), then asks [`Store::plan_drift_from_changed`]
    /// which of those paths no task expects. Plans with no worktree still appear here (with a
    /// `None` path) so the daemon can skip them cheaply.
    pub fn plan_worktrees(&self) -> Result<Vec<PlanWorktreeRef>, StoreError> {
        self.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT p.id, p.worktree_id, wt.path, wt.branch, wt.base_ref
                 FROM plans p
                 LEFT JOIN worktrees wt ON wt.id = p.worktree_id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(PlanWorktreeRef {
                    plan_id: row.get::<_, String>(0)?,
                    worktree_id: row.get::<_, Option<i64>>(1)?,
                    path: row.get::<_, Option<String>>(2)?,
                    branch: row.get::<_, Option<String>>(3)?,
                    base_ref: row.get::<_, Option<String>>(4)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        })
    }
    /// The complete check workspace (§9): everything a run of checks needs, loaded
    /// from the store. `worktrees` carry their changed-file set (computed live from
    /// `git`), `sessions` carry their `tool_calls`, and per-worktree last activity is
    /// derived from the `events` table (the `last_activity_ms` column stays NULL because
    /// the event bus only persists `worktree_dirty` events).
    pub fn check_workspace(&self) -> Result<CheckWorkspace, StoreError> {
        self.read(|conn| {
            let mut wts = Vec::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT id, path, branch, head, base_ref, merge_base, is_main, present                      FROM worktrees ORDER BY path",
                )?;
                let rows = stmt.query_map([], |row| Ok(WtInfo {
                    id: row.get::<_, i64>(0)?,
                    path: row.get::<_, String>(1)?,
                    branch: row.get(2)?,
                    head: row.get(3)?,
                    base_ref: row.get(4)?,
                    merge_base: row.get(5)?,
                    is_main: row.get(6)?,
                    present: row.get(7)?,
                    changed: Vec::new(),
                }))?;
                for r in rows { wts.push(r?); }
            }
            // Recompute each worktree's changed-file set from live git (as sync_worktrees).
            for wt in &mut wts {
                wt.changed = crate::sources::git::changed_file_set(
                    std::path::Path::new(&wt.path),
                    wt.branch.as_deref(),
                    wt.base_ref.as_deref(),
                );
            }
            let mut plans = Vec::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT id, source, title, worktree_id, state FROM plans ORDER BY id",
                )?;
                let rows = stmt.query_map([], |row| Ok(PlanInfo {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    title: row.get(2)?,
                    worktree_id: row.get(3)?,
                    state: row.get(4)?,
                }))?;
                for r in rows { plans.push(r?); }
            }
            let mut tasks = Vec::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT id, plan_id, ordinal, title, declared, expected_paths, depends_on, observed, observed_reason FROM tasks ORDER BY plan_id, ordinal",
                )?;
                let rows = stmt.query_map([], |row| {
                    let expected: String = row.get(5)?;
                    let depends: String = row.get(6)?;
                    Ok(TaskInfo {
                        id: row.get(0)?,
                        plan_id: row.get(1)?,
                        ordinal: row.get(2)?,
                        title: row.get(3)?,
                        declared: row.get(4)?,
                        expected_paths: serde_json::from_str(&expected).unwrap_or(Vec::new()),
                        depends_on: serde_json::from_str(&depends).unwrap_or(Vec::new()),
                        observed: row.get(7)?,
                        observed_reason: row.get(8)?,
                    })
                })?;
                for r in rows { tasks.push(r?); }
            }
            let mut sessions = Vec::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT id, agent, last_event_ms, tokens_in, tokens_out, tokens_cache, cost_usd, tool_calls                      FROM sessions ORDER BY id",
                )?;
                let rows = stmt.query_map([], |row| Ok(SessionInfo {
                    id: row.get(0)?,
                    agent: row.get(1)?,
                    last_event_ms: row.get(2)?,
                    tokens_in: row.get(3)?,
                    tokens_out: row.get(4)?,
                    tokens_cache: row.get(5)?,
                    cost_usd: row.get(6)?,
                    tool_calls: Vec::new(),
                }))?;
                for mut s in rows.flatten() {
                    let sid = s.id.clone();
                    let mut cs = Vec::new();
                    let mut cs_stmt = conn.prepare(
                        "SELECT tool, args_hash, is_error, ts_ms FROM tool_calls WHERE session_id = ?1 ORDER BY ts_ms"
                    )?;
                    for c in cs_stmt.query_map([sid.as_str()], |row| Ok(TC {
                        tool: row.get(0)?,
                        args_hash: row.get(1)?,
                        is_error: row.get(2)?,
                        ts_ms: row.get(3)?,
                    }))? { cs.push(c?); }
                    s.tool_calls = cs;
                    sessions.push(s);
                }
            }
            // Per-worktree last activity from the events table (last_activity_ms is always NULL).
            let mut activity = std::collections::HashMap::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT subject, MAX(ts_ms) FROM events WHERE source='worktree' AND kind='worktree_dirty' GROUP BY subject"
                )?;
                let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))?;
                for row in rows.flatten() { activity.insert(row.0, row.1); }
            }
            Ok(CheckWorkspace { worktrees: wts, plans, tasks, sessions, activity })
        })
    }

    /// Reconcile a fresh run of all checks (`candidates`) against the persisted findings,
    /// applying the lifecycle (§5): open new findings, refresh open/ack findings, resolve
    /// findings that no longer fire, reopen resolved findings that reappear. Returns the
    /// opened/resolved deltas so the daemon can emit finding_opened/finding_resolved events.
    pub fn sync_findings(
        &self,
        candidates: Vec<FindingCandidate>,
        now_ms: i64,
    ) -> Result<Vec<FindingDelta>, StoreError> {
        let mut best: std::collections::HashMap<String, FindingCandidate> =
            std::collections::HashMap::new();
        // De-duplicate candidates on `dedupe_key` (the column is UNIQUE): when several checks
        // produce the same key (e.g. overlap<wtA>:<wtB> computed from either order, or a
        // merge-tree pair given in either head order), keep the most-severe view.
        for c in candidates {
            match best.get(&c.dedupe_key) {
                Some(existing)
                    if severity_rank(&existing.severity) >= severity_rank(&c.severity) => {}
                _ => {
                    best.insert(c.dedupe_key.clone(), c);
                }
            }
        }
        self.write(|conn| {
            let mut stmt = conn.prepare("SELECT id, dedupe_key, kind, severity, subject, summary, evidence, confidence, state, first_seen_ms, last_seen_ms, resolved_ms FROM findings")?;
            let mut current: std::collections::HashMap<String, FindingRow> = std::collections::HashMap::new();
            for r in stmt.query_map([], |row| Ok(FindingRow {
                id: row.get(0)?, dedupe_key: row.get(1)?, kind: row.get(2)?, severity: row.get(3)?,
                subject: row.get(4)?, summary: row.get(5)?,
                evidence: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(Value::Null),
                confidence: row.get(7)?, state: row.get(8)?, first_seen_ms: row.get(9)?,
                last_seen_ms: row.get(10)?, resolved_ms: row.get(11)?,
            }))? {
                let f = r?;
                current.insert(f.dedupe_key.clone(), f);
            }
            let mut deltas: Vec<FindingDelta> = Vec::new();
            for (key, cand) in &best {
                let key = key.clone();
                match current.get(key.as_str()) {
                    None => {
                        conn.execute(
                            "INSERT INTO findings(dedupe_key, kind, severity, subject, summary, evidence, confidence, state, first_seen_ms, last_seen_ms, resolved_ms)                              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?8, NULL)",
                            rusqlite::params![key.clone(), cand.kind, cand.severity, cand.subject, cand.summary,
                                serde_json::to_string(&cand.evidence).unwrap_or_else(|_| "{}".to_string()),
                                cand.confidence, now_ms])?;
                        deltas.push(FindingDelta { opened: true, dedupe_key: key, kind: cand.kind.clone(), subject: cand.subject.clone(), severity: cand.severity.clone() });
                    }
                    _ => {
                        conn.execute(
                            "UPDATE findings SET kind = ?2, severity = ?3, subject = ?4, summary = ?5,                              evidence = ?6, confidence = ?7, state = 'open', last_seen_ms = ?8, resolved_ms = NULL                              WHERE dedupe_key = ?1",
                            rusqlite::params![key.clone(), cand.kind, cand.severity, cand.subject, cand.summary,
                                serde_json::to_string(&cand.evidence).unwrap_or_else(|_| "{}".to_string()),
                                cand.confidence, now_ms])?;
                        deltas.push(FindingDelta { opened: true, dedupe_key: key, kind: cand.kind.clone(), subject: cand.subject.clone(), severity: cand.severity.clone() });
                    }
                }
            }
            for (key, row) in &current {
                if best.contains_key(key.as_str()) { continue; }
                let was_open = row.state != "acked" && row.state != "resolved";
                conn.execute("UPDATE findings SET state = 'resolved', last_seen_ms = ?2, resolved_ms = ?2 WHERE dedupe_key = ?1", rusqlite::params![key.clone(), now_ms])?;
                if was_open {
                    deltas.push(FindingDelta { opened: false, dedupe_key: key.clone(), kind: row.kind.clone(), subject: row.subject.clone(), severity: row.severity.clone() });
                }
            }
            Ok(deltas)
        })
    }

    /// Persist one finding candidate (used by tests and by non-transactional callers).
    pub fn upsert_finding(&self, cand: &FindingCandidate, now_ms: i64) -> Result<(), StoreError> {
        self.write(|conn| {
            conn.execute(
                "INSERT INTO findings(dedupe_key, kind, severity, subject, summary, evidence, confidence, state, first_seen_ms, last_seen_ms, resolved_ms)                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?8, NULL)                  ON CONFLICT(dedupe_key) DO UPDATE SET kind = excluded.kind, severity = excluded.severity,                  subject = excluded.subject, summary = excluded.summary, evidence = excluded.evidence,                  confidence = excluded.confidence, state = 'open', last_seen_ms = excluded.last_seen_ms, resolved_ms = NULL",
                rusqlite::params![cand.dedupe_key, cand.kind, cand.severity, cand.subject, cand.summary,
                    serde_json::to_string(&cand.evidence).unwrap_or_else(|_| "{}".to_string()),
                    cand.confidence, now_ms])?;
            Ok(())
        })
    }

    /// List findings (§9), optionally restricted to `--open` (only state='open') or `--all`
    /// (any state); with neither flag, resolved rows are excluded (open + acked remain).
    pub fn list_findings(&self, filter: &FindingsFilter) -> Result<Vec<FindingRow>, StoreError> {
        self.read(|conn| {
            let kind_filter = filter.kind.as_deref().unwrap_or("");
            let mut sql = String::from(
                "SELECT id, dedupe_key, kind, severity, subject, summary, evidence, confidence,                  state, first_seen_ms, last_seen_ms, resolved_ms FROM findings");
            let mut cond = String::from("1 = 1");
            if filter.all {} else if filter.open { cond.push_str(" AND state = 'open'"); }
            else { cond.push_str(" AND state != 'resolved'"); }
            if !kind_filter.is_empty() { cond.push_str(" AND kind = ?1"); }
            sql.push_str(&format!(" WHERE {cond} ORDER BY CASE severity WHEN 'error' THEN 0 WHEN 'warn' THEN 1 ELSE 2 END, dedupe_key"));
            let mut stmt = conn.prepare(&sql)?;
            let params = if kind_filter.is_empty() { rusqlite::params![] } else { rusqlite::params![kind_filter] };
            let rows = stmt.query_map(params, |row| Ok(FindingRow {
                id: row.get(0)?, dedupe_key: row.get(1)?, kind: row.get(2)?, severity: row.get(3)?,
                subject: row.get(4)?, summary: row.get(5)?,
                evidence: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(Value::Null),
                confidence: row.get(7)?, state: row.get(8)?, first_seen_ms: row.get(9)?,
                last_seen_ms: row.get(10)?, resolved_ms: row.get(11)?,
            }))?;
            rows.collect::<Result<Vec<_>, _>>()
        })
    }

    /// Acknowledge one finding by row id (§7): sets state='acked' and refreshes last_seen_ms.
    pub fn ack_finding(&self, id: &str, now_ms: i64) -> Result<bool, StoreError> {
        self.write(|conn| {
            let n = conn.execute(
                "UPDATE findings SET state = 'acked', last_seen_ms = ?2 WHERE id = ?1",
                rusqlite::params![id, now_ms],
            )?;
            Ok(n > 0)
        })
    }
}
/// Find the row id of one finding on its `dedupe_key` (§9); returns None when it does not exist.
pub fn finding_id(
    conn: &rusqlite::Connection,
    dedupe_key: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    Ok(conn
        .query_row(
            "SELECT id FROM findings WHERE dedupe_key = ?1",
            [dedupe_key],
            |row| row.get(0),
        )
        .ok())
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "error" => 2,
        "warn" => 1,
        _ => 0,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FindingRow {
    pub id: i64,
    pub dedupe_key: String,
    pub kind: String,
    pub severity: String,
    pub subject: String,
    pub summary: String,
    pub evidence: Value,
    pub confidence: String,
    pub state: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    pub resolved_ms: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FindingCandidate {
    pub kind: String,
    pub dedupe_key: String,
    pub subject: String,
    pub summary: String,
    pub severity: String,
    pub evidence: Value,
    pub confidence: String,
}

impl FindingCandidate {
    pub fn new(
        kind: &str,
        dedupe_key: &str,
        subject: &str,
        summary: &str,
        severity: &str,
        evidence: Value,
        confidence: &str,
    ) -> Self {
        FindingCandidate {
            kind: kind.to_string(),
            dedupe_key: dedupe_key.to_string(),
            subject: subject.to_string(),
            summary: summary.to_string(),
            severity: severity.to_string(),
            evidence,
            confidence: confidence.to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FindingDelta {
    pub opened: bool,
    pub dedupe_key: String,
    pub kind: String,
    pub subject: String,
    pub severity: String,
}

#[derive(Debug, Clone, Default)]
pub struct FindingsFilter {
    pub open: bool,
    pub all: bool,
    pub kind: Option<String>,
}

/// One worktree, as seen by the check context (its changed set is filled in by `check_workspace`).
#[derive(Debug, Clone)]
pub struct WtInfo {
    /// The worktree PK (`worktrees.id`).
    pub id: i64,
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub base_ref: Option<String>,
    pub merge_base: Option<String>,
    pub is_main: bool,
    pub present: bool,
    pub changed: Vec<(String, String)>,
}

/// One plan row.
#[derive(Debug, Clone)]
pub struct PlanInfo {
    pub id: String,
    pub source: String,
    pub title: String,
    pub worktree_id: Option<i64>,
    pub state: String,
}

/// One task row (expected/depends parsed as JSON arrays).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// The task PK (`tasks.id`).
    pub id: String,
    pub plan_id: String,
    pub ordinal: i64,
    pub title: String,
    pub declared: String,
    pub expected_paths: Vec<String>,
    pub depends_on: Vec<String>,
    pub observed: String,
    pub observed_reason: Option<String>,
}

/// One session with its tool_calls.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub agent: String,
    pub last_event_ms: Option<i64>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache: i64,
    pub cost_usd: Option<f64>,
    pub tool_calls: Vec<TC>,
}

#[derive(Debug, Clone)]
pub struct TC {
    pub tool: String,
    pub args_hash: String,
    pub is_error: Option<bool>,
    pub ts_ms: i64,
}

/// Everything a run of checks needs, assembled by the daemon.
#[derive(Debug, Clone)]
pub struct CheckWorkspace {
    pub worktrees: Vec<WtInfo>,
    pub plans: Vec<PlanInfo>,
    pub tasks: Vec<TaskInfo>,
    pub sessions: Vec<SessionInfo>,
    /// per-worktree-path last activity (unix-ms) from `worktree_dirty` events.
    pub activity: std::collections::HashMap<String, Option<i64>>,
}

/// One plan and the worktree it drifts against (Task 3).
#[derive(Debug, Clone)]
pub struct PlanWorktreeRef {
    /// The plan id (`plans.id`).
    pub plan_id: String,
    /// The worktree id (`plans.worktree_id`); `None` when the plan has no worktree.
    pub worktree_id: Option<i64>,
    /// The worktree absolute path (`worktrees.path`). `None` when `worktree_id` is `None`.
    pub path: Option<String>,
    /// The worktree branch (`worktrees.branch`). `None` when detached.
    pub branch: Option<String>,
    /// The base ref used to compute the changed-file set (`worktrees.base_ref`). `None`
    /// when unset.
    pub base_ref: Option<String>,
}

/// One drifted file (Task 3): a path that changed in the plan's worktree but that no
/// task *expects* (`tasks.expected_paths`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DriftFile {
    /// The changed path, relative to the worktree.
    pub path: String,
    /// Where the change was found: committed | staged | unstaged | untracked.
    pub origin: String,
}
