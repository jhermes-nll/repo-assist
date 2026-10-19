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
        f(&lock).map_err(StoreError::Database)
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
}
