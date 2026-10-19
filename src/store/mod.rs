//! SQLite-backed single source of truth.
//!
//! A [`Store`] opens a database in WAL mode, applies schema migrations via
//! [`crate::store::migrations`], and guards a single writer connection with a [`std::sync::Mutex`].
//! Read queries take additional read handles. All DB work happens on these blocking handles
//! and must stay off the async runtime threads (Section 4).

pub(crate) mod migrations;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    /// Upsert one `sessions` row and REPLACE its `tool_calls` rows.
    ///
    /// All statements run inside a single `unchecked_transaction` on the writer
    /// connection, committed only at the end: a failure mid-way rolls back the session
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
