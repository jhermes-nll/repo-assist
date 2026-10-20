# repo-assist — Spec & Status

Local, advisory, always-on daemon (`ra`) that watches a git repository across all worktrees and surfaces agent/plan **conflicts**, **drift**, and **stalls**.

- Repo: https://github.com/jhermes-nll/repo-assist
- Crate: `repo-assist` 0.1.0, edition 2021, rustc 1.85+
- Binary: `ra`

## Architecture

    fs watcher (notify) -> git source (changed-file sets) -> SQLite (WAL, user_version 2)
                                                    -> ipc socket (Unix domain)
                                                    -> plan CLI

- **Store** (`src/store/`): SQLite, WAL journaling, `PRAGMA user_version = 2`. Auto-migrates on `Store::open`.
- **Sources** (`src/sources/`): `fswatch` (fs notify) + `git` (worktree discovery, changed-file sets, conflict/merge detection).
- **IPC** (`src/ipc/`): Unix-domain socket; a foreground daemon binds one socket per repo; client commands connect (auto-spawning a detached daemon on first use).
- **Daemon** (`src/daemon/`): `run_foreground`, `spawn_detached`, socket + pidfile locking.
- **Checks** (`src/checks/mod.rs`): pure finding candidates over a `CheckWorkspace`; kinds `plan.drift`, `plan.overlap`, `plan.stalled`, `plan.checkbox_mismatch`, `worktree.overlap`, `worktree.merge_conflict`, `worktree.abandoned`, `agent.loop`, `agent.tokens`.
- **Findings reconcile** (`src/daemon/findings.rs`): daemon runs all checks, reconciles candidates against persisted `findings` via `Store::sync_findings` (open → acknowledged/resolved → reopen).

## Data model (schema.sql)

| Table | Purpose |
|---|---|
| `worktrees` | One row per worktree (path, branch, head, base_ref, is_main, present). |
| `worktree_files` | Changed-file set per worktree: (worktree_id, path, origin). origin in {committed,staged,unstaged,untracked}. |
| `plans` | Plans (id, source, title, file, worktree_id, content_hash, state, first/last_seen). |
| `tasks` | Plan tasks; `expected_paths` (JSON array), `depends_on` (JSON array), `observed`, `declared`. |
| `evidence` | Per-task evidence (kind, ref, ts, confidence). |
| `sessions` / `tool_calls` | Agent session telemetry (tokens, cost, tool calls, byte offset, parse errors). |
| `findings` | Conflicts / drift / stalls (dedupe_key, kind, severity, subject, summary, state, first/last_seen, resolved_ms). |
| `events` | Generic audit log (source, kind, subject, payload). |

Ingest model (sessions/tool_calls): root discovered from `PI_CODING_AGENT_SESSION_DIR` / `PI_CODING_AGENT_DIR` / `$HOME/.pi/agent/sessions`; daemon ticker rescan every 60s (first pass immediately); changed files (by byte size) are fully re-parsed, unchanged files skip on the size cursor; a session whose cwd falls inside no known worktree maps to no worktree (NULL `worktree_id`), never force-joined; session files are read only.

Key public types:

- `PlanRow { id, source, title, file, worktree_id, content_hash, state }`
- `TaskRow { id, plan_id, ordinal, title, declared, expected_paths, depends_on }`
- `EvidenceRow { task_id, kind, reference, ts_ms, confidence }`
- `DriftFile { path, origin }`
- `PlanWorktreeRef { plan_id, worktree_id, path, branch, base_ref }`

Public store methods (subset): `open` / `open_with`, `read`, `write`, `append_event`, `worktree_dirty_view`, `upsert_plan`, `upsert_task`, `append_evidence`, `plan_drift_from_changed(plan_id, &changed) -> Option<Vec<DriftFile>>`, `plan_worktrees() -> Vec<PlanWorktreeRef>`, `sync_findings`, `upsert_finding`, `list_findings`, `ack_finding`, `finding_id`.

## `ra` CLI (per `src/cli.rs` `Spec` table)

| Command | Implemented | Task |
|---|---|---|
| `ra daemon` | yes | 1 |
| `ra status` | yes | 1 |
| `ra worktrees` | yes | 5 |
| `ra plans` | yes | 2 |
| `ra plan <id>` | yes | 2 |
| `ra drift` | yes | 3 |
| `ra conflicts` | yes | 5 |
| `ra who-touches` | yes | 5 |
| `ra findings` | yes | 5 |
| `ra ack` | yes | 5 |
| `ra sessions` | yes | 4 |
| `ra tui` | no | 5 |
| `ra doctor` | no | 5 |
| `ra render` | no | 2 |

## RPC methods (per `src/ipc/server.rs`)

Implemented: `Hello`, `Ping`, `Status`, `Shutdown`, `Plans`, `Plan`, `Drift`, `Sessions`, `Worktrees`, `Conflicts`, `WhoTouches`, `Findings`, `Ack`.
Not yet: `Subscribe`, `RenderViews`.

## Test suite (all green)

| File | Tests | Covers |
|---|---:|---|
| `cli.rs` | 1 | CLI wiring |
| `config.rs` | 7 | config load |
| `findings_engine.rs` | 4 | findings lifecycle (open → acked/resolved → reopen) |
| `findings_kind.rs` | 1 | findings `--kind` filter over real IPC path |
| `fswatch.rs` | 2 | fs watcher |
| `git_repo.rs` | 3 | git repo ops |
| `git_source.rs` | 8 | changed-file sets |
| `store.rs` | 3 | store CRUD |
| `task_2_3.rs` | 2 | worktree event bus |
| `task_2_4.rs` | 3 | plans data model + IPC |
| `task_3.rs` | 3 | **drift** (drifts when a task doesn't expect it, reports nothing otherwise, unknown plan -> None) |
| `task_4.rs` | 12 | Task 4 acceptance: pi session source, ingest, worktree mapping, `ra sessions` end-to-end |

The three gates are re-verified on this machine for this task: `cargo fmt --check` clean, `cargo clippy --all-targets --all-features -- -D warnings` exit 0, and `cargo test` = `test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (per-suite: cli 1, config 7, findings_engine 4, findings_kind 1, fswatch 2, git_repo 3, git_source 8, store 3, task_2_3 2, task_2_4 3, task_3 3, task_4 12).

## Commits on `main`

| SHA | Task |
|---|---|
| `d016b97` | Task 1.1 - SQLite store (WAL + migrations) |
| `764d7be` | Task 1.2 - IPC server/client, `ra` daemon, `ra status` |
| `a33b0e3` | Task 2.1 - git source (worktree discovery + changed-file sets) |
| `91fdcc4` | Task 2.2 - fs watcher over worktree changes |
| `5163673` | Task 2.3 - worktree event bus (watcher -> store -> status) |
| `9201ce8` | Task 2.4 - plans data model (store CRUD -> IPC -> `ra plans`/`ra plan`) |
| `1e1915b` | **Task 3.5 - `plan_drift_from_changed` store method; RPC `Drift` handler; `ra drift [--json]` CLI** |
| `002b59c` | fix(clippy): `.is_err()` for the sqlite3 availability guards (nonminimal_bool) |
| `950d996` | fix(daemon): take the flock before writing the pidfile; drop `/proc` pre-check |
| `292c026` | fix(test): canonicalize the worktree key (macOS /private tmpdir paths) |
| `75f7434` | Task 4: pi session source -> sessions store -> RPC Sessions -> ra sessions [--json] |
| `a28b553` | fix(task-4): atomic session upsert + review follow-ups |
| `e8f2516` | Task 2.3: findings-engine spec acceptance test (lifecycle incl. ack→resolve→reappear) |
| `8e04e4f` | Task 2.3: findings engine implementation (store + daemon + CLI + IPC) |

Latest commit: `8e04e4f` (2026-10-19).

## Status summary

**Done:** daemon + `status` (T1), plans data model + `ra plans`/`ra plan` (T2), **drift** — `ra drift [--json]` (T3), **sessions** — pi session source → sessions store → RPC `Sessions` → `ra sessions [--json]` (T4), and **findings engine** — `src/checks`, `src/daemon/findings`, store sync + CLI `ra findings`/`ra ack` (T5).

**Backlog (per `Spec`):** `ra tui`, `ra doctor`, `ra render` (unimplemented stubs).

**Repaired before Task 4:** the three baseline fixes (clippy guards; lock-first pidfile ordering + portable test wait; canonicalized tmpdir key) — `002b59c`, `950d996`, `292c026`.

**Plan:** `docs/superpowers/plans/2026-10-19-status-refresh-and-smoke-run.md`.

## Known limitations

- Known limitation: bare `ra findings` (neither `--open` nor `--all`) applies no state filter, so resolved rows appear alongside open ones.
