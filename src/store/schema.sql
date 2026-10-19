-- repo-assist store schema (PRAGMA user_version = 2). See plan section 5.
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS worktrees (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  branch TEXT,
  head TEXT,
  base_ref TEXT,
  merge_base TEXT,
  is_main INTEGER NOT NULL DEFAULT 0,
  present INTEGER NOT NULL DEFAULT 1,
  last_activity_ms INTEGER,
  updated_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS worktree_files (
  worktree_id INTEGER NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  origin TEXT NOT NULL,
  PRIMARY KEY (worktree_id, path, origin)
);

CREATE TABLE IF NOT EXISTS plans (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  title TEXT NOT NULL,
  file TEXT,
  worktree_id INTEGER,
  content_hash TEXT,
  state TEXT NOT NULL,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL,
  FOREIGN KEY (worktree_id) REFERENCES worktrees(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  title TEXT NOT NULL,
  declared TEXT NOT NULL,
  expected_paths TEXT NOT NULL DEFAULT '[]',
  depends_on TEXT NOT NULL DEFAULT '[]',
  observed TEXT NOT NULL DEFAULT 'unknown',
  observed_reason TEXT
);

CREATE TABLE IF NOT EXISTS evidence (
  id INTEGER PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  ref TEXT NOT NULL,
  ts_ms INTEGER NOT NULL,
  confidence TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  agent TEXT NOT NULL,
  file TEXT NOT NULL,
  cwd TEXT,
  worktree_id INTEGER,
  started_ms INTEGER,
  last_event_ms INTEGER,
  tokens_in INTEGER NOT NULL DEFAULT 0,
  tokens_out INTEGER NOT NULL DEFAULT 0,
  tokens_cache INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL,
  tool_calls INTEGER NOT NULL DEFAULT 0,
  byte_offset INTEGER NOT NULL DEFAULT 0,
  parse_errors INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tool_calls (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  ts_ms INTEGER NOT NULL,
  tool TEXT NOT NULL,
  args_hash TEXT NOT NULL,
  is_error INTEGER,
  result_hash TEXT
);

CREATE TABLE IF NOT EXISTS findings (
  id INTEGER PRIMARY KEY,
  dedupe_key TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL,
  severity TEXT NOT NULL,
  subject TEXT NOT NULL,
  summary TEXT NOT NULL,
  evidence TEXT NOT NULL DEFAULT '{}',
  confidence TEXT NOT NULL DEFAULT 'explicit',
  state TEXT NOT NULL,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL,
  resolved_ms INTEGER
);

CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY,
  ts_ms INTEGER NOT NULL,
  source TEXT NOT NULL,
  kind TEXT NOT NULL,
  subject TEXT,
  payload TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_worktree_files_worktree ON worktree_files(worktree_id);
CREATE INDEX IF NOT EXISTS idx_plans_state ON plans(state);
CREATE INDEX IF NOT EXISTS idx_tasks_plan ON tasks(plan_id);
CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);
CREATE INDEX IF NOT EXISTS idx_findings_state ON findings(state);
CREATE INDEX IF NOT EXISTS idx_evidence_task ON evidence(task_id);
