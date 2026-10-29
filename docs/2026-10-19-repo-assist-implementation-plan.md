This is the original upstream implementation plan, recovered 2026-10-19.
It was NOT previously in the repo.

**The task numbering in this document does not match what was actually built.**
For example this plan's `Task 2.3` is "Findings engine", but the implemented repo's
`Task 2.3` is "worktree event bus (watcher -> store -> status)". The implementer
restructured as it went.

Therefore, for "what actually landed", `docs/status.md` is the
authority, NOT this document.

Known deliberate divergences from this document:
- The pi session adapter was built at `src/sources/sessions.rs` (this doc's Task 4.2 specifies `src/agents/{mod.rs,pi.rs}`).
- The ingest model uses skip-cursor + full re-parse rather than the "incremental tail by byte offset" specified here — because the `tool_calls` table has no call-id column to join a later-arriving `toolResult` to its call row.

# repo-assist Implementation Plan

> **For the implementing agent:** work through the tasks in order. Each task lists the files it touches and an acceptance check. Use TDD, commit after each task, and keep `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` green at every commit. Section 13 lists facts that were NOT verified and must be checked against real data on this machine before the code that depends on them is written.

## 1. Goal

`repo-assist` is a local, always-on, **advisory** daemon that watches one git repository (all of its worktrees) while coding agents work in it. It tracks what agents and plans are in flight, detects conflicts and problems with deterministic checks, and exposes the result to humans (a light TUI) and to agents (a CLI and generated markdown).

It is written in Rust, ships as a single binary `ra`, and uses **no LLM** anywhere in this plan.

## 2. Non-goals and hard rules

- **Advisory only.** The daemon never edits, commits, kills, or resolves anything. It never writes inside any work tree. All output goes to its own state directory.
- **No nudging.** Nothing is pushed into an agent's context. Agents pull (CLI) or read generated files.
- **No MCP server, no LLM calls, no network access** in this plan.
- **Agent-agnostic and framework-agnostic.** The core model and all checks know nothing about pi, Claude Code, superpowers, etc. Those live in adapters (Section 8).
- **Inferred is not explicit.** Anything derived by heuristic is marked `inferred` and must never alone produce an `error`-severity finding.
- Out of scope for now (see Section 17): the `ra-report` pi extension, CodeGraphContext and mcp-memory-service integration, a snapshot test runner, research artifacts, Jev, native tasks (`ra task`).

## 3. Conventions

- Rust stable, pinned in `rust-toolchain.toml`. Edition 2021 or later.
- Errors: `thiserror` in library modules, `anyhow` only in `main.rs` and CLI glue. No `unwrap()` or `expect()` outside tests.
- Time: unix milliseconds, UTC, stored as `INTEGER`. Use the `time` crate.
- Logging: `tracing`, to a rolling file in the state dir (and stderr with `--foreground`).
- Shell out to `git` (do not use libgit2). Always pass `-C <path>`, use `-z` / `--porcelain` forms, set `LC_ALL=C`, and never run `git` commands that take the index lock more than needed (`GIT_OPTIONAL_LOCKS=0` for status).
- Dependencies (do not add others without a note in the commit message): `tokio`, `serde`, `serde_json`, `clap` (derive), `rusqlite` (feature `bundled`), `notify`, `notify-debouncer-full`, `ignore`, `globset`, `ratatui`, `crossterm`, `sysinfo`, `toml`, `tracing`, `tracing-subscriber`, `tracing-appender`, `thiserror`, `anyhow`, `time`, `sha2`, `hex`, `directories`, `fs2` (flock). Dev: `tempfile`, `assert_cmd`, `predicates`, `insta`.

## 4. Architecture

```
 sources ──events──▶ bus ──▶ store (SQLite, WAL)
 (git, fs,                    │
  plans, pi sessions,         ├──▶ checks ──▶ findings (deduped, lifecycle)
  procs)                      │
                              ├──▶ views (markdown, atomic writes)
                              └──▶ ipc server (Unix socket, NDJSON)
                                         ▲
                           CLI (`ra …`) ─┴─ TUI (`ra tui`)   (both thin clients)
```

- One daemon per repository. The **repo key** is the first 12 hex chars of `sha256(canonicalized git common dir)`, so every worktree of one repo shares one daemon.
- **Sources** emit `Event`s onto an in-process `tokio::sync::broadcast` bus. Sources never write findings.
- **Checks** are pure-ish functions over store state, triggered by relevant events (debounced) and by a slow backstop timer. They output the **complete set of findings they currently believe true** for their scope; the findings engine diffs that against stored findings (open new, refresh existing, resolve vanished). Clients see deltas via `subscribe`.
- **Store** is the single source of truth. CLI, TUI and markdown views are all projections of it.

### State directory layout

```
$XDG_STATE_HOME/repo-assist/<repo-key>/      (default ~/.local/state/repo-assist/<repo-key>/)
  db.sqlite   daemon.pid   daemon.log
  views/STATUS.md  views/PLANS.md  views/FINDINGS.md
$XDG_RUNTIME_DIR/repo-assist/<repo-key>.sock  (fallback: <state dir>/daemon.sock)
```

## 5. Data model (SQLite, WAL, `PRAGMA user_version` migrations)

Use `src/store/schema.sql` plus ordered migrations; refuse to open a DB with a newer `user_version` than the binary knows.

```sql
CREATE TABLE worktrees (
  id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, branch TEXT, head TEXT,
  base_ref TEXT, merge_base TEXT, is_main INTEGER NOT NULL DEFAULT 0,
  present INTEGER NOT NULL DEFAULT 1, last_activity_ms INTEGER, updated_ms INTEGER NOT NULL);

CREATE TABLE worktree_files (            -- changed-file set per worktree
  worktree_id INTEGER NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
  path TEXT NOT NULL, origin TEXT NOT NULL,   -- committed|staged|unstaged|untracked
  PRIMARY KEY (worktree_id, path, origin));

CREATE TABLE plans (
  id TEXT PRIMARY KEY,                    -- sha256(source|worktree_path|relative_path)[..16]
  source TEXT NOT NULL,                   -- superpowers|checklist|native
  title TEXT NOT NULL, file TEXT, worktree_id INTEGER REFERENCES worktrees(id),
  content_hash TEXT, state TEXT NOT NULL, -- active|done|gone
  first_seen_ms INTEGER NOT NULL, last_seen_ms INTEGER NOT NULL);

CREATE TABLE tasks (
  id TEXT PRIMARY KEY,                    -- plan_id + ':' + ordinal
  plan_id TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL, title TEXT NOT NULL,
  declared TEXT NOT NULL,                 -- open|done   (from the plan text)
  expected_paths TEXT NOT NULL DEFAULT '[]',   -- JSON array; empty = unknown
  depends_on TEXT NOT NULL DEFAULT '[]',
  observed TEXT NOT NULL DEFAULT 'unknown',    -- not_started|in_progress|committed|unknown
  observed_reason TEXT);

CREATE TABLE evidence (
  id INTEGER PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,                     -- commit|dirty_file|agent_event
  ref TEXT NOT NULL, ts_ms INTEGER NOT NULL,
  confidence TEXT NOT NULL);              -- explicit|inferred

CREATE TABLE sessions (
  id TEXT PRIMARY KEY, agent TEXT NOT NULL, file TEXT NOT NULL, cwd TEXT,
  worktree_id INTEGER REFERENCES worktrees(id), started_ms INTEGER, last_event_ms INTEGER,
  tokens_in INTEGER NOT NULL DEFAULT 0, tokens_out INTEGER NOT NULL DEFAULT 0,
  tokens_cache INTEGER NOT NULL DEFAULT 0, cost_usd REAL, tool_calls INTEGER NOT NULL DEFAULT 0,
  byte_offset INTEGER NOT NULL DEFAULT 0, parse_errors INTEGER NOT NULL DEFAULT 0);

CREATE TABLE tool_calls (                 -- bounded ring per session (keep newest 500)
  id INTEGER PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  ts_ms INTEGER NOT NULL, tool TEXT NOT NULL, args_hash TEXT NOT NULL,
  is_error INTEGER, result_hash TEXT);

CREATE TABLE findings (
  id INTEGER PRIMARY KEY, dedupe_key TEXT NOT NULL UNIQUE, kind TEXT NOT NULL,
  severity TEXT NOT NULL,                 -- info|warn|error
  subject TEXT NOT NULL, summary TEXT NOT NULL, evidence TEXT NOT NULL DEFAULT '{}',
  confidence TEXT NOT NULL DEFAULT 'explicit',
  state TEXT NOT NULL,                    -- open|acked|resolved
  first_seen_ms INTEGER NOT NULL, last_seen_ms INTEGER NOT NULL, resolved_ms INTEGER);

CREATE TABLE events (                      -- append-only, pruned to last 10k rows
  id INTEGER PRIMARY KEY, ts_ms INTEGER NOT NULL, source TEXT NOT NULL,
  kind TEXT NOT NULL, subject TEXT, payload TEXT NOT NULL DEFAULT '{}');
```

Finding lifecycle: `open` → `acked` (user, suppresses notification but still shown dimmed) → `resolved` (check no longer reports it). A resolved finding that reappears becomes `open` again with the same row. Acks do not survive resolution.

## 6. IPC protocol

Unix domain socket, newline-delimited JSON, one request per line.

```
→ {"id":1,"method":"status","params":{}}
← {"id":1,"result":{…}}            or   {"id":1,"error":{"code":"…","message":"…"}}
```

- First message from every client must be `{"method":"hello","params":{"proto":1,"client":"ra-cli 0.1"}}`; the daemon replies with its version and proto version and rejects incompatible protos.
- Methods: `hello`, `ping`, `status`, `worktrees`, `plans`, `plan {id}`, `findings {state?, kind?}`, `ack {id}`, `conflicts`, `who_touches {path}`, `drift`, `sessions`, `subscribe {topics}`, `render_views`, `shutdown`.
- `subscribe` turns the connection into a stream of `{"event":"finding_opened"|"finding_resolved"|"worktree_changed"|"plan_changed"|"session_changed","data":{…}}` lines. Clients must reconnect on EOF.
- Socket mode `0600`. Refuse to start if another live daemon holds the pidfile flock for the same repo key.

## 7. CLI (`ra`)

Every read command supports `--json`. Human output must be compact and stable.

| Command | Purpose |
|---|---|
| `ra daemon [--foreground]` | Run the daemon for the repo containing the cwd. |
| `ra status` | One-screen summary: worktrees, active plans, open findings, sessions. |
| `ra worktrees` | Worktrees with branch, ahead/behind base, changed-file counts, last activity. |
| `ra plans` / `ra plan <id>` | Plans with declared vs observed progress; detail lists tasks and evidence. |
| `ra drift` | Files changed in a plan's worktree that no task expects. |
| `ra conflicts` | Overlaps and merge conflicts between worktrees and between in-flight plans. |
| `ra who-touches <path>` | Worktrees, plans, sessions currently touching a path. |
| `ra findings [--open\|--all] [--kind K]` | List findings. |
| `ra ack <id>` | Acknowledge a finding. |
| `ra sessions` | Known agent sessions. |
| `ra tui` | Open the TUI. |
| `ra doctor` | Environment checks (git version, state dir writable, socket, adapters' roots). |
| `ra render` | Force-render markdown views now. |

Client commands auto-start the daemon in the background (detached, `setsid`) unless `--no-spawn` is passed. Guard the race with the pidfile flock.

## 8. Adapters

Adapters are the only place that knows about a specific framework or agent. They convert external formats into the canonical model and must degrade gracefully (unknown fields ignored, unknown entry kinds preserved as `Unknown`, parse failures counted not fatal).

### 8.1 Plan sources (framework side)

Trait: `PlanSource { fn name(); fn discover(&self, worktrees:&[Worktree]) -> Vec<PlanFile>; fn parse(&self, file:&PlanFile, text:&str) -> Result<ParsedPlan>; }`. `ParsedPlan` = title + ordered tasks (title, declared state, optional `expected_paths`, optional `depends_on`).

- **superpowers**: glob `docs/superpowers/plans/*.md` inside **every** worktree (a plan lives on its feature branch until merged; also scan the main checkout). Parse leniently: title from first `# ` heading; a task is a heading matching `^#{2,4}\s+Task\s+\d+` (fallback: top-level checkbox groups); declared state from `- [x]`/`- [ ]` (a task is `done` only if all of its checkboxes are checked and there is at least one); `expected_paths` from lines like `Create:`, `Modify:`, `Test:`, `Delete:` containing backticked paths (strip `:line` / `:line-line` suffixes and any `(…)` notes). Globs are configurable.
- **checklist** (generic): configurable globs (default `TODO.md`, `PLAN.md`, `docs/plans/**/*.md`); a plan per file, a task per checkbox item, `expected_paths` empty. Checks that need `expected_paths` skip these tasks and say so.
- **native** (later): `ra task add`; not in this plan.

A plan's `state`: `active` if it has any open task and its worktree is present; `done` if all tasks declared done; `gone` if its file disappeared (keep the row for one day so findings can resolve cleanly).

### 8.2 Agent activity (agent side)

Trait: `AgentSource { fn name(); fn discover(&self) -> Vec<SessionFile>; fn read_new(&mut self, file:&SessionFile, offset:u64) -> Result<(Vec<AgentEvent>, u64)>; }`. `AgentEvent` kinds: `SessionStart{cwd,id,ts}`, `Usage{tokens_in,tokens_out,tokens_cache,cost_usd?,ts}`, `ToolCall{tool,args_hash,ts}`, `ToolResult{tool,is_error,result_hash,ts}`, `Message{role,ts}`, `Compaction{ts}`, `Unknown{kind}`.

- **pi** (implement now): details in Section 12.
- Anything else: later, same trait. Agents that can later push events will use `ra report` (out of scope here).

## 9. Findings catalog

Thresholds live in config (Section 10) with these defaults. Each check lists its `kind`, trigger, rule, severity, `dedupe_key`.

| Kind | Rule | Severity | dedupe_key |
|---|---|---|---|
| `worktree.overlap` | Changed-file sets of two worktrees intersect after `ignore_paths`. Changed set = committed-vs-merge-base ∪ staged ∪ unstaged ∪ untracked. | warn | `worktree.overlap:<wtA>:<wtB>` (ids sorted) |
| `worktree.merge_conflict` | `git merge-tree --write-tree` between the two branch heads exits 1. Cache by `(headA, headB)`. Needs git ≥ 2.38, else the check is disabled and `ra doctor` says why. | error | `worktree.merge_conflict:<wtA>:<wtB>` |
| `worktree.abandoned` | No activity for `abandoned_hours` (48) AND (uncommitted changes OR an active plan with open tasks). | info | `worktree.abandoned:<wt>` |
| `plan.overlap` | Two **active** plans in different worktrees have intersecting `expected_paths` (after `ignore_paths`). Predictive: fires before any edit. | warn | `plan.overlap:<planA>:<planB>` |
| `plan.drift` | A file in the plan's worktree changed set is in no task's `expected_paths`. Skipped for plans with no `expected_paths`. One finding per plan listing up to 20 paths. | info (warn if > 10 paths) | `plan.drift:<plan>` |
| `plan.stalled` | Active plan with open tasks and no worktree activity for `stalled_hours` (4). | info | `plan.stalled:<plan>` |
| `plan.checkbox_mismatch` | Task `declared=open` but `observed=committed`; or `declared=done` and `observed=not_started` with `expected_paths` known. | warn | `plan.checkbox_mismatch:<task>` |
| `agent.loop` | In a session's last `loop_window` (20) tool calls, the same `(tool, args_hash)` occurs ≥ `loop_repeats` (5) times and (≥ 80% are errors OR `result_hash` is identical). | warn | `agent.loop:<session>:<tool>:<args_hash>` |
| `agent.tokens` | Session token total exceeds `session_token_budget` (default unset = disabled). | warn | `agent.tokens:<session>` |

**Task `observed` derivation** (in `src/plans/derive.rs`, pure function, table-tested):
- `committed`: for tasks with `expected_paths`, every expected path appears in a commit on the plan's branch made after the plan file's first commit (`git log <merge_base>..HEAD --name-only`); evidence rows are those commits. Tasks without `expected_paths`: `unknown`.
- `in_progress`: not `committed`, and at least one expected path is in the worktree's staged/unstaged/untracked set or in some commit on the branch.
- `not_started`: has `expected_paths`, and none of them touched.
- Evidence from inference carries `confidence=inferred`. Only `explicit` evidence (a future explicit task id) may justify `error` severity.

## 10. Config (TOML)

Precedence: built-in defaults < `~/.config/repo-assist/config.toml` < `<repo root>/.repo-assist.toml` (read-only for the daemon) < CLI flags. Unknown keys are an error with a clear message (`deny_unknown_fields`).

```toml
base_branch = "auto"            # auto = origin/HEAD, else main, else master
ignore_paths = ["Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "**/*.snap"]

[thresholds]
abandoned_hours = 48
stalled_hours = 4
loop_window = 20
loop_repeats = 5
session_token_budget = 0        # 0 = disabled
session_quiet_secs = 120

[sources.git]
poll_secs = 5                   # backstop; fs events drive most updates
[sources.plans]
superpowers_globs = ["docs/superpowers/plans/*.md"]
checklist_globs   = ["TODO.md", "PLAN.md", "docs/plans/**/*.md"]
[sources.pi]
enabled = true
sessions_dir = ""               # empty = auto (see 12)
[views]
enabled = true
debounce_ms = 1000
```

## 11. Markdown views and agent integration

- Render `STATUS.md`, `PLANS.md`, `FINDINGS.md` into `views/`, atomically (write temp file in the same dir, then rename), only when content changed, debounced. Cap each at ~150 lines, open findings first.
- First lines of every view: `generated_at: <iso>`, `daemon: <version>`, and a note that this is read-only and may be stale.
- Provide `docs/agents-snippet.md` (a short block users paste into their `AGENTS.md`/`CLAUDE.md`): "Repo coordination status: run `ra status`, `ra who-touches <path>` before editing shared files, `ra conflicts` before merging. Read-only. Check `generated_at`." The daemon never installs it.

## 12. pi session adapter details

**Verify each bullet against real files in `~/.pi/agent/sessions` on this machine before coding (Task 4.1).**

- Root: `$PI_CODING_AGENT_SESSION_DIR` if set (used directly); else `${PI_CODING_AGENT_DIR:-~/.pi/agent}/sessions`; config `sources.pi.sessions_dir` overrides. Scan `*.jsonl` recursively. Layout is expected to be `--<encoded-cwd>--/<timestamp>_<uuid>.jsonl`.
- Line 1 is a header `{"type":"session","version":3,"id":…,"timestamp":…,"cwd":…,"parentSession"?:…}`. Link a session to a worktree by canonicalizing `cwd` and finding the worktree path that is equal to or a prefix of it. Support version 3 fully; for other versions parse best-effort and warn once in the log.
- Subsequent entries have `type`, `id`, `parentId`, `timestamp`. Entries form a tree. Expected kinds: `message`, `compaction`, `branch_summary`, `model_change`, `thinking_level_change`, `session_info`, `custom`. Unknown kinds become `Unknown`.
- Token usage: `message` entries where `message.role == "assistant"` carry `message.usage`; `compaction` and `branch_summary` entries carry the usage of their summarizing call. Sum whatever of input/output/cache fields exist; use a cost field if present, else leave `cost_usd` null.
- Tool calls and results: expected as assistant content blocks of kind tool-call (name, arguments) and messages with a tool-result role (tool name, is-error flag). Hash arguments via canonical JSON → sha256 (first 16 hex).
- **Tree handling:** one file may hold several branches. Use the entry that was appended last as the active leaf, walk `parentId` to the root for the active branch, and count only active-branch entries for loop detection. Count all entries for token totals. Confirm "last appended = active leaf" against fixtures; if wrong, fix in one place (`active_leaf()`).
- Incremental reading: remember byte offset per file; read only complete lines; if the file shrinks, reset to 0. Never hold a file open between polls.
- Session activity state shown in the UI is `recent` (last event within `session_quiet_secs`) or `quiet`. Do not claim "idle/running/waiting": the log cannot prove it.

## 13. Unverified assumptions (resolve first, record the answers in `docs/verified-facts.md`)

1. Real pi session entry shapes for usage, tool calls, tool results, branching (Section 12).
2. The exact markup of a real superpowers plan (task headings, `Create:`/`Modify:` lines, checkbox placement). Capture 2–3 real plan files as fixtures under `tests/fixtures/plans/`; the sample in Task 3.1 is representative only.
3. That pi's session `cwd` equals the worktree path when an agent runs inside a worktree.
4. Installed git version supports `merge-tree --write-tree` (≥ 2.38).
5. Whether a plan checkbox update is committed on the plan's branch in practice (affects `plan.checkbox_mismatch` noise; tune the rule, don't suppress silently).

## 14. File map

```
Cargo.toml  rust-toolchain.toml  README.md  .github/workflows/ci.yml
docs/agents-snippet.md  docs/verified-facts.md
src/main.rs  cli.rs  config.rs  paths.rs  model.rs
src/store/{mod.rs,schema.sql,migrations.rs,queries.rs}
src/ipc/{mod.rs,proto.rs,client.rs,server.rs}
src/daemon/{mod.rs,bus.rs,scheduler.rs,findings.rs,lock.rs}
src/sources/{mod.rs,git.rs,fswatch.rs}
src/plans/{mod.rs,superpowers.rs,checklist.rs,derive.rs}
src/agents/{mod.rs,pi.rs}
src/checks/{mod.rs,worktree_overlap.rs,merge_conflict.rs,abandoned.rs,plan_overlap.rs,plan_drift.rs,plan_stalled.rs,plan_checkbox.rs,agent_loop.rs,agent_tokens.rs}
src/views/{mod.rs,markdown.rs}
src/tui/{mod.rs,app.rs,ui.rs,tabs/{worktrees.rs,plans.rs,findings.rs,sessions.rs}}
tests/{common/mod.rs,cli.rs,git_repo.rs,findings_engine.rs,pi_adapter.rs,plans_parse.rs,derive.rs,views.rs}
tests/fixtures/{pi/,plans/}
```

## 15. Tasks

### Phase 0 — Scaffold

### Task 0.1: Project skeleton and CI
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.github/workflows/ci.yml`, `src/main.rs`, `src/cli.rs`
- [ ] `ra --version` and `ra --help` work; CI runs fmt, clippy `-D warnings`, test.
- Acceptance: `cargo test` passes on an empty test suite; CI file valid.

### Task 0.2: Paths, repo key, config, logging
- Create: `src/paths.rs`, `src/config.rs`
- Test: `tests/git_repo.rs`, `tests/common/mod.rs`
- [ ] Resolve git common dir from any cwd inside any worktree; compute the repo key; compute state/socket paths.
- [ ] Load layered TOML config with `deny_unknown_fields` and the defaults in Section 10.
- [ ] Test helper that builds temp git repos with N worktrees, commits and dirty files.
- Acceptance: two worktrees of one repo yield the same key; a different repo yields a different key; bad config gives a readable error.

### Phase 1 — Daemon skeleton

### Task 1.1: Store and migrations
- Create: `src/store/{mod.rs,schema.sql,migrations.rs,queries.rs}`, `src/model.rs`
- [ ] Open DB in WAL mode; apply schema via `user_version`; refuse newer DBs.
- [ ] Single writer thread (or mutex) with read connections for queries; all DB calls off the async runtime threads.
- Acceptance: migration test creates a fresh DB and reopens it; a DB with higher `user_version` is rejected.

### Task 1.2: IPC server/client and `ra daemon`, `ra status`
- Create: `src/ipc/*`, `src/daemon/{mod.rs,bus.rs,lock.rs}`
- Test: `tests/cli.rs`
- [ ] Pidfile flock; stale-socket cleanup; socket `0600`; graceful shutdown on SIGINT/SIGTERM and `shutdown`.
- [ ] `hello`/`ping`/`status` working end to end; `ra status --json` output is stable.
- [ ] Auto-spawn on first client command (`--no-spawn` disables); concurrent first calls start exactly one daemon.
- Acceptance: `assert_cmd` test starts the daemon in a temp repo, queries it, shuts it down; running a second daemon for the same repo fails fast with a clear message.

### Phase 2 — Git, worktrees, findings engine

### Task 2.1: Git source
- Create: `src/sources/git.rs`
- [ ] Discover worktrees via `git worktree list --porcelain -z`; detect base ref; compute merge-base, ahead/behind, and the changed-file set (Section 5) per worktree with `git status --porcelain=v2 -z` and `git diff --name-only <merge-base>`.
- [ ] Coalesce updates per worktree; at most one in-flight git process per worktree; poll backstop `poll_secs`.
- Acceptance: tests with temp repos cover committed, staged, unstaged, untracked, renamed, and deleted files, a removed worktree, and detached HEAD.

### Task 2.2: Filesystem watcher
- Create: `src/sources/fswatch.rs`
- [ ] Watch each worktree (respect `.gitignore` via `ignore`; never watch `.git` internals except `HEAD`/refs) with 500 ms debounce; map events to `worktree_dirty{id}`; update `last_activity_ms`.
- Acceptance: touching a file triggers exactly one refresh within ~1 s in a test; ignored paths trigger none.

### Task 2.3: Findings engine
- Create: `src/daemon/findings.rs`, `src/checks/mod.rs`
- Test: `tests/findings_engine.rs`
- [ ] Check trait: `fn run(&self, ctx:&CheckCtx) -> Vec<FindingCandidate>`; engine does open/refresh/resolve diffing, reopen-on-reappear, ack handling, and emits delta events.
- Acceptance: table tests for every lifecycle transition, including ack-then-resolve-then-reappear.

### Task 2.4: Worktree checks
- Create: `src/checks/{worktree_overlap.rs,merge_conflict.rs,abandoned.rs}`
- [ ] Implement the three kinds from Section 9; cache merge-tree results by head pair; apply `ignore_paths`.
- Acceptance: temp-repo scenarios produce exactly the expected findings and resolve them after the overlap disappears.

### Task 2.5: Query commands, views, TUI v0
- Create: `src/views/*`, `src/tui/*` (worktrees + findings tabs), CLI commands `worktrees`, `conflicts`, `who-touches`, `findings`, `ack`, `render`
- [ ] Atomic markdown rendering per Section 11, snapshot-tested with `insta`.
- [ ] TUI: header (repo, daemon version, counts), tabs, `j/k` move, `Enter` detail, `a` ack, `Tab`/`1-4` switch, `/` filter, `r` refresh, `q` quit; subscribes and reconnects.
- Acceptance: `ratatui` `TestBackend` snapshot tests for each tab; killing and restarting the daemon makes the TUI reconnect without crashing.

### Phase 3 — Plans

### Task 3.1: Plan model, superpowers and checklist adapters
- Create: `src/plans/{mod.rs,superpowers.rs,checklist.rs}`
- Test: `tests/plans_parse.rs`, `tests/fixtures/plans/`
- [ ] Implement parsers per 8.1 against the real fixtures captured in Section 13 item 2, plus this representative sample:

```markdown
# Rate Limit Implementation Plan
### Task 1: Add limiter
- Create: `src/limiter.rs`
- Test: `tests/limiter.rs`
- [ ] Write failing test
- [x] Implement
### Task 2: Wire into router
- Modify: `src/router.rs:40-80`
- [ ] Add middleware
```
Expected: 2 tasks; task 1 `open` with paths `src/limiter.rs`, `tests/limiter.rs`; task 2 `open` with `src/router.rs`.
- Acceptance: parsers never panic on malformed files (property or fuzz-lite test); unknown formats yield a plan with zero tasks and a logged warning.

### Task 3.2: Plan discovery, linking, derivation
- Create: `src/plans/derive.rs`; extend `src/sources/git.rs` with per-path commit lookup
- Test: `tests/derive.rs`
- [ ] Discover plan files per worktree on `worktree_dirty` events touching plan globs and on a slow timer; upsert plans/tasks; mark vanished plans `gone`.
- [ ] Implement `observed` derivation exactly as in Section 9, as a pure function with a truth-table test.
- Acceptance: temp-repo scenario: committing a task's files flips `observed` to `committed` and records commit evidence.

### Task 3.3: Plan checks and commands
- Create: `src/checks/{plan_overlap.rs,plan_drift.rs,plan_stalled.rs,plan_checkbox.rs}`, `src/tui/tabs/plans.rs`; CLI `plans`, `plan`, `drift`
- [ ] Implement the four kinds from Section 9; plans without `expected_paths` skip the path-based checks and `ra plan` says "no path data".
- [ ] Plans tab: title, worktree, declared vs observed progress bars, stalled flag; detail pane lists tasks and evidence.
- Acceptance: two worktrees with plans listing the same file produce `plan.overlap` before any edit exists.

### Phase 4 — pi sessions

### Task 4.1: Capture fixtures and verify facts
- Create: `tests/fixtures/pi/`, `docs/verified-facts.md`
- [ ] Copy 3–4 real, redacted pi session files (include one with branching and one with a compaction) and record in `docs/verified-facts.md` the answers to Section 13 items 1 and 3.
- Acceptance: file exists and states each item as verified/not-verified with evidence. Do not proceed to 4.2 on unverified shapes; adjust Section 12 first.

### Task 4.2: pi adapter
- Create: `src/agents/{mod.rs,pi.rs}`
- Test: `tests/pi_adapter.rs`
- [ ] Discovery, header parsing, worktree linking, incremental tail by byte offset, active-branch resolution, usage and tool-call extraction per Section 12.
- [ ] Corrupt or partial last line is skipped and retried next poll; increments `parse_errors` only for complete bad lines.
- Acceptance: fixtures parse with expected token totals and tool-call counts; appending lines to a fixture copy yields only the new events.

### Task 4.3: Agent checks and sessions tab
- Create: `src/checks/{agent_loop.rs,agent_tokens.rs}`, `src/tui/tabs/sessions.rs`; CLI `sessions`
- [ ] Implement `agent.loop` and `agent.tokens` per Section 9; sessions tab shows agent, worktree, `recent/quiet`, last event age, tokens.
- Acceptance: a synthetic session with 6 identical failing calls opens `agent.loop`; a varied session does not.

### Phase 5 — Hardening

### Task 5.1: doctor, performance, docs
- Create: `README.md`, `docs/agents-snippet.md`; extend CLI with `doctor`
- [ ] `ra doctor` checks git version, state dir, socket, adapter roots, config validity.
- [ ] Measure and record: idle daemon CPU < 1% and RSS < 50 MB with 5 worktrees; `ra status` < 100 ms warm; cap events table and tool-call ring sizes.
- [ ] README: install, quick start, config reference, how to add a plan source or agent adapter.
- Acceptance: numbers recorded in README; all CI checks green.

## 16. Definition of done

- All tasks above checked; CI green; no `unwrap()` in non-test code.
- Running the daemon in a repo with three worktrees and two active plans surfaces a worktree overlap, a plan overlap, plan drift, and a stalled plan correctly in the CLI, the TUI, and the markdown views.
- The daemon has made zero writes inside any work tree (verify with a test that snapshots each worktree's file tree and `git status` before and after a run).
- Section 13 is fully resolved in `docs/verified-facts.md`.

## 17. Deferred (do not build now)

`ra-report` extension for pi and equivalents for other agents (push events, explicit task ids via env var or custom session entry); CodeGraphContext (index freshness, dependents-based semantic conflicts); mcp-memory-service (digests, failure memory); snapshot-checkout test runner; pre-task research artifacts; native tasks (`ra task`); Jev-based filtering in shadow mode; any form of nudging.
