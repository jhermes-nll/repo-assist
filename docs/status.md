# repo-assist — Spec & Status

Local, advisory, always-on daemon (`ra`) that watches a git repository across all worktrees and surfaces agent/plan **conflicts**, **drift**, and **stalls**.

- Repo: https://github.com/jhermes-nll/repo-assist
- Crate: `repo-assist` 0.1.0, edition 2021, rustc 1.85+
- Binary: `ra`

## Architecture

    fs watcher (notify) -> git source (changed-file sets) -> SQLite (WAL, user_version 3)
                                                    -> ipc socket (Unix domain)
                                                    -> plan CLI

- **Store** (`src/store/`): SQLite, WAL journaling, `PRAGMA user_version = 3`. Auto-migrates on `Store::open`.
- **Sources** (`src/sources/`): `fswatch` (fs notify) + `git` (worktree discovery, changed-file sets, conflict/merge detection).
- **IPC** (`src/ipc/`): Unix-domain socket; a foreground daemon binds one socket per repo; client commands connect (auto-spawning a detached daemon on first use).
- **Daemon** (`src/daemon/`): `run_foreground`, `spawn_detached`, socket + pidfile locking.
- **Checks** (`src/checks/mod.rs`): pure finding candidates over a `CheckWorkspace`; kinds `plan.drift`, `plan.overlap`, `plan.stalled`, `plan.checkbox_mismatch`, `worktree.overlap`, `worktree.merge_conflict`, `worktree.abandoned`, `agent.loop`, `agent.tokens`.
- **Findings reconcile** (`src/daemon/findings.rs`): daemon runs all checks, reconciles candidates against persisted `findings` via `Store::sync_findings` (open → acknowledged/resolved → reopen).

## Modules (recent)

- `src/views/`: view rendering (`render_status`, `render_plans`, `render_findings`) + atomic writes (`write::write_views`, `VIEW_CAP_LINES`); renders `views/STATUS.md`, `views/PLANS.md`, `views/FINDINGS.md` under the state dir (`ra render` on demand; daemon debounces when `[views] enabled`).
- `src/doctor/`: environment checks (`run_all`, `Check`, `CheckStatus`, `git_version_from_output`, `parse_git_version`); run locally by `ra doctor` (not an RPC).
- `src/tui/`: four-tab TUI (`run`, `app::{App, state}`, `client`, `ui`; depends on `ratatui` 0.29 and `crossterm` 0.28). Detail pane persists across the ~1 s refresh tick and re-resolves by row id (closing on tab switch or when the referenced row disappears); rows are tabular with headers (`path | branch | changed` for worktrees, `id | agent | last_event_ms` for sessions, etc.).

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
| `ra tui` | yes | 5 |
| `ra doctor` | yes | 5 |
| `ra render` | yes | 2 |

## RPC methods (per `src/ipc/server.rs`)

Implemented: `Hello`, `Ping`, `Status`, `Shutdown`, `Plans`, `Plan`, `Drift`, `Sessions`, `Worktrees`, `Conflicts`, `WhoTouches`, `Findings`, `Ack`, `RenderViews`, `Subscribe`.
Not exposed as RPC (run locally by CLI): `Doctor`.

## Test suite (all green)

| File | Tests | Covers |
|---|---:|---|
| `cli.rs` | 1 | CLI wiring |
| `config.rs` | 7 | config load |
| `daemon_zero_writes.rs` | 1 | daemon does no writes on read-only paths |
| `derive.rs` | 6 | plan derivation |
| `doctor.rs` | 7 | environment checks |
| `findings_engine.rs` | 5 | findings lifecycle (open → acked/resolved → reopen) |
| `findings_kind.rs` | 1 | findings `--kind` filter over real IPC path |
| `findings_spec_source.rs` | 6 | findings spec-source tracking |
| `fswatch.rs` | 2 | fs watcher |
| `git_repo.rs` | 3 | git repo ops |
| `git_source.rs` | 8 | changed-file sets |
| `lib` (unit) | 8 | in-crate unit tests |
| `null_is_error.rs` | 1 | a NULL `tool_calls.is_error` does not break the findings workspace build |
| `plans_discover.rs` | 1 | plan file discovery |
| `plans_parse.rs` | 18 | plan file parsers |
| `plans_spec.rs` | 10 | plans spec acceptance |
| `stale_socket.rs` | 1 | a present-but-unlistened socket is recovered from by auto-spawning a daemon |
| `store.rs` | 5 | store CRUD |
| `store_helpers.rs` | 2 | store helpers |
| `subscribe_rpc.rs` | 6 | subscribe RPC |
| `task_2_3.rs` | 2 | worktree event bus |
| `task_2_4.rs` | 3 | plans data model + IPC |
| `task_3.rs` | 3 | **drift** (drifts when a task doesn't expect it, reports nothing otherwise, unknown plan -> None) |
| `task_4.rs` | 12 | Task 4 acceptance: pi session source, ingest, worktree mapping, `ra sessions` end-to-end |
| `tui.rs` | 18 | four-tab TUI |
| `views_render.rs` | 12 | view rendering + atomic writes |

The three gates are re-verified on this machine for this task: `cargo fmt --check` clean, `cargo clippy --all-targets --all-features -- -D warnings` exit 0, and `cargo test` = `test result: ok. 149 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out` (per-suite: lib 8, cli 1, config 7, daemon_zero_writes 1, derive 6, doctor 7, findings_engine 5, findings_kind 1, findings_spec_source 6, fswatch 2, git_repo 3, git_source 8, null_is_error 1, plans_discover 1, plans_parse 18, plans_spec 10, stale_socket 1, store 5, store_helpers 2, subscribe_rpc 6, task_2_3 2, task_2_4 3, task_3 3, task_4 12, tui 18, views_render 12; `performance` 0 passed / 1 ignored), green on 3 consecutive runs. Note: run with a short `TMPDIR` (e.g. `/tmp`) in deep worktrees, or the daemon unix socket path exceeds the 108-byte limit.

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
| `aebde9e` | Task 1 & 2: `ra render` and `ra doctor` implementation |
| `99b3921` | fix(doctor): remove unwrap; config validity from caller result |
| `5db0cd0` | Task 3: `ra tui` (four tabs, keybindings, reconnect via polling) |
| `007b80d` | test(tui): integration and state tests for `ra tui` |
| `d457ff5` | fix(tui): make `a` ack findings end-to-end |
| `396760a` | fix(tui): address review findings (M1-M3, S1-S5, nits) |

| `350b8df` | fix: stale socket auto-recovery; NULL is_error in findings |
| `1c08b3b` | test: regression tests for stale socket + NULL is_error |
| `ac5e1b5` | fix(tui): detail survives refresh; tabular rows with headers |
| `e41085e` | fix(test): repair tui detail-clears test |

Latest `main` commit: `d8844d7` (this doc refresh lands one commit later). The seven `overnight-*` branches plus the two in-flight cezar branches (`cez/148278d7`, `cez/52509991`) have landed on `main` (101 -> 126 tests).

## Status summary

**Done:** daemon + `status` (T1), plans data model + `ra plans`/`ra plan` (T2), **drift** — `ra drift [--json]` (T3), **sessions** — pi session source → sessions store → RPC `Sessions` → `ra sessions [--json]` (T4), and **findings engine** — `src/checks`, `src/daemon/findings`, store sync + CLI `ra findings`/`ra ack` (T5).

**All 14 commands implemented** (`ra tui`, `ra doctor`, `ra render` landed; see modules below).

**Repaired before Task 4:** the three baseline fixes (clippy guards; lock-first pidfile ordering + portable test wait; canonicalized tmpdir key) — `002b59c`, `950d996`, `292c026`.

**Plan:** `docs/superpowers/plans/2026-10-19-status-refresh-and-smoke-run.md`.

## Known limitations

### Fixed

- ~~bare `ra findings` (neither `--open` nor `--all`) applies no state filter, so resolved rows appear alongside open ones~~ — fixed in `fd09782`: bare `ra findings` now excludes resolved rows (`tests/findings_engine.rs::bare_findings_excludes_resolved`).

- ~~stale-socket brick~~ — fixed in `350b8df`: `UnixStream::connect` failures with `ErrorKind::ConnectionRefused` are now classified as `IpcError::Stale`, and `resolve_sock` removes the stale socket and pidfile before spawning. Other connect errors still surface.
- ~~`is_error` NULL~~ — fixed in `350b8df`: `TC.is_error` is now `Option<bool>`; `checks/mod.rs` coerces with `unwrap_or(false)`, so NULL rows no longer break `check_workspace`.

## TUI verification

The TUI was verified in a real terminal as well as by tests: four tabs rendering real data, the detail pane surviving multiple refresh ticks, `Esc`, `/` filter, tab switching, and `q` restoring the terminal cleanly. The unit tests alone had missed two real defects.
