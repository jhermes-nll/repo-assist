# repo-assist Task 4 (`ra sessions`) + Baseline Repair — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ra sessions [--json]` real end to end (pi session files → parser → SQLite `sessions`/`tool_calls` → RPC `Sessions` → CLI rendering), after repairing the three baseline defects that currently make clippy/test red on the working machine and clippy red in CI.

**Architecture:** A new pure-parsing source module `src/sources/sessions.rs` discovers and parses pi's JSONL session logs into aggregates; a store method writes one session plus its tool calls atomically; a daemon-side ticker thread (`src/daemon/sessions.rs`, started in `run_foreground` beside the worktree event bus [E46]) keeps rows fresh; the IPC `Sessions` handler is a pure store read mirroring `plans_json` [E43]; `run_sessions` in the CLI mirrors `run_plans` [E44].

**Tech Stack:** Rust (edition 2021, rust-version 1.85, `rust-toolchain.toml` channel=stable), rusqlite 0.32 bundled (WAL), serde/serde_json, sha2+hex (already direct deps [E57]), clap 4, tokio (daemon), assert_cmd/tempfile (tests). One dependency-file change: enable the `parsing` feature of the existing `time` crate for RFC3339 timestamps. No new crates.

**Spec:** this plan + `docs/verified-facts.md` (Items 1 & 3) + `status.md` (stale; corrections enumerated in Task 5) + `docs/ra.1.md` (documents `ra sessions [--json]`; "never writes inside any work tree").

**Evidence:** `docs/superpowers/plans/artifacts/evidence-repo-assist-task-4-sessions.md` (HEAD `962b980`). Every claim about existing code below carries `[E<n>]`/`[A<n>]` matching that ledger.

## Global Constraints

- **Local commits only.** Never `git push`, never create branches/worktrees/remotes; commits stay on `main` in this checkout without explicit user approval.
- **Gates:** every task leaves `cargo fmt --check` clean and `cargo clippy --all-targets --all-features -- -D warnings` exit 0 (CI's has no `--all-features`; the crate defines no features). `cargo test` fully green from Task 3 onward.
- **Daemon is advisory, read-only w.r.t. work trees** [E51]; sources never write findings [E48]. Session files are opened read-only.
- **Test convention:** all tests in `tests/`, zero inline `#[test]` in `src/` [E55]; one test file per task; `mod common` [E53].
- **Commit convention:** one commit per task [E60], message body carries the verify command + result.
- **Blast radius / ordering:** Tasks 1–3 touch disjoint files (`tests/store.rs` | `src/daemon/lock.rs`+`tests/cli.rs` | `tests/task_2_3.rs`), disjoint call graphs (`is_alive` has exactly one caller [E9]; the lint guards are test-local [E3]). They are file-parallel in principle, but the `-D warnings` gate compiles ALL targets — including `tests/store.rs` — so **execute strictly in order 1 → 2 → 3** (single worker); Task 3's full-suite gate is the checkpoint proving the baseline green. Task 4 touches none of these files.
- **Fixture discipline:** parser tests use only the committed `.jsonl.redacted.jsonl` variants [E29, E66], never the raw ones.
- **Env names must match pi itself:** `PI_CODING_AGENT_SESSION_DIR` / `PI_CODING_AGENT_DIR`, verified from pi's shipped source [E63, E64].

---

## Position on the baseline (required decision)

**Decision: fix-baseline-first.** Tasks 1–3 (≈15 changed lines, two test-only) land before Task 4.

**CI status on main today: RED.** The clippy step (`cargo clippy --all-targets -- -D warnings`, ubuntu-latest + latest stable from dtolnay/rust-action) hits the same two `nonminimal_bool` errors reproduced locally on stable-1.94 [E2, E5, E6]; a failed step aborts the job (test step unreached). Strong inference, not an observed run [A2]. The two test failures do NOT reproduce on CI's OS: the cli failure requires macOS (no `/proc` → second daemon reports a raw flock error, not "already running" [E7–E11]); the task_2_3 failure requires macOS tmpdir symlinks (`/var/folders/...` vs git's `/private/var/...` path form [E14–E18]).

**Why fix-first:**
1. Task 4's verification is `cargo test`; with two known-red tests, new regressions are un-attributable. Green baseline → every red is Task 4's own.
2. `cargo clippy -D warnings` gates every task below and is already failing before anyone writes a line (and per CI's command, on CI's OS).
3. The fixes are tiny and two are pre-verified: the task_2_3 fix was run twice in a scratch copy — suite passes, 2.84s, no flakiness [E19]; the lock fix is a traced root cause with the existing red test as its red→green proof [E9, E10, E62].

**Risks of fix-first (accepted):** the lock change is the only product-code change (removes a redundant, Linux-only pre-check; wire-visible behavior on both platforms is preserved per the trace [E62]); two commits are test-only. **Risks of task4-first (rejected):** un-attributable test failures, a permanently red clippy gate + CI, and Task-4 commits growing atop a lint-failing tree.

**Advisor cross-examination (pre-plan; dispositions recorded in the ledger):** adopted — torn-tail rule, per-file single transactions, no-offset-on-no-header-skip, per-file error isolation, Drop-guard ticker, orphan/duplicate-call rules, documented ~60s staleness, component-wise path matching. Rebutted with citations — no pidfile-unlink hazard exists [E61]; `libc::kill` is unnecessary because reordering already yields the correct message on all platforms [E62]; fixture numbers come from the committed redacted fixtures [E66].

## File structure

- `src/sources/sessions.rs` (**new**) — pi session discovery + parsing + `sync_sessions`. Pure: no daemon types, no tokio. (The module's existence is already declared by `sources/mod.rs`'s docstring [E48].)
- `src/sources/mod.rs` — add `pub mod sessions;`.
- `src/daemon/sessions.rs` (**new**) — `SessionsTicker` thread (lifecycle only).
- `src/daemon/mod.rs` — add `pub mod sessions;` + start the ticker in `run_foreground` beside `start_bus` [E46].
- `src/store/mod.rs` — `SessionRow`, `ToolCallRow`, `upsert_session_with_calls`, `session_file_offsets` (mirroring `upsert_plan`/`append_evidence` patterns [E26, E27]).
- `src/ipc/server.rs` — `Method::Sessions` dispatch arm + `sessions_json(store)` (mirroring `Method::Plans`/`plans_json` [E42, E43]).
- `src/cli.rs` — spec flip `Sessions → implemented: true` [E41], `run_sessions`, `print_sessions_human` [E44].
- `Cargo.toml` — `time` features become `["formatting", "macros", "parsing"]` (dedupes the duplicated `"macros"` while touching the line).
- `tests/task_4.rs` (**new**) — all Task 4 tests [E55].
- Baseline: `tests/store.rs` [E2], `src/daemon/lock.rs` + `tests/cli.rs` [E9, E12, E61, E62], `tests/task_2_3.rs` [E18, E19].
- Docs: `status.md` (Task 5) [E58]. `docs/ra.1.md` needs **no** changes — it already documents `ra sessions [--json]` exactly [E52].

---

### Task 1 — Clippy baseline: `.is_err()` for the sqlite3 availability guards

**Owner:** @worker · **Parallel group:** PG1 · **Depends on:** nothing

**Files:**
- Modify: `tests/store.rs:11` and `tests/store.rs:92` — both currently `if !Command::new("sqlite3").arg("--version").output().is_ok()` inside `set_user_version` and the guard of `newer_schema_version_is_refused` [E2, E3, E4].

**Interfaces:** none (test-local).

- [ ] **Step 1: Fix both sites** — change each to:

```rust
if Command::new("sqlite3").arg("--version").output().is_err() {
```

Identical semantics (missing sqlite3 → `.output()` errors → skip); this is exactly clippy's `help:` suggestion [E2].

- [ ] **Step 2: Verify**
Run: `cargo clippy --all-targets -- -D warnings && echo CLIPPY-OK` (CI's exact form [E5]) and `cargo clippy --all-targets --all-features -- -D warnings && echo CLIPPY-ALL-OK`
Expected: both print OK, exit 0.
Run: `cargo test --test store`
Expected: `3 passed` (store suite was green in the baseline run apart from clippy [caller-verified]).

- [ ] **Step 3: Commit**

```bash
git add tests/store.rs
git commit -m "fix(clippy): .is_err() for the sqlite3 availability guards (nonminimal_bool)

cargo clippy --all-targets -- -D warnings failed on tests/store.rs:11 and :92.
Verified: clippy exit 0 (both forms); cargo test --test store 3/3."
```

### Task 2 — Second-daemon detection: lock first, then write the pidfile; drop the /proc pre-check

**Owner:** @worker · **Parallel group:** PG1 (disjoint files, but run after Task 1: the `-D warnings` gate still compiles `tests/store.rs` until Task 1 lands) · **Depends on:** Task 1 (gates)

**Files:**
- Modify: `src/daemon/lock.rs:32-79` — `acquire_lock` body rewrite + delete `is_alive`; update the module doc [E8, E9, E10, E62].
- Modify: `tests/cli.rs:113-137` — replace the `/proc/{pid}` exit-wait with a socket-gone poll; the pidfile read that feeds it is removed [E12, E13].

**Interfaces:** `acquire_lock(pid_path: &Path) -> Result<File, LockError>` unchanged [E8]; `LockError::{Busy, Flock}` unchanged [E8]; `is_alive` deleted (single caller [E9]).

**Behavior trace the implementer must hold to ([E62]):** after reordering, a failed `try_lock_exclusive()` means the pidfile still contains the **real** holder's pid (contenders no longer clobber it [E10]), so the existing fallback branch (`held != 0 && held != std::process::id()` → `LockError::Busy`) prints "a daemon is already running for this repository (pid N)" on Linux **and** macOS. Deleting the pre-check also removes the only `/proc` dependence in production code [E13] and a latent false-refusal (stale pidfile naming a recycled-live pid with the flock actually free). No code path deletes the pidfile [E61]; takeover semantics are unchanged since the OS releases a dead holder's flock.

- [ ] **Step 1: Confirm the red** — Run: `cargo test --test cli 2>&1 | tail -4`
Expected: FAIL — `second-daemon error should mention the live daemon, got: … could not take the pidfile lock: Resource temporarily unavailable (os error 35)` [E7]. (On CI's OS this test is currently green [E11]; the fix must not regress it.)

- [ ] **Step 2: Rewrite `acquire_lock`** (doc comment on `acquire_lock` updated to describe the new ordering):

```rust
pub fn acquire_lock(pid_path: &Path) -> Result<File, LockError> {
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LockError::StateDir(e.to_string()))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(pid_path)
        .map_err(|e| LockError::Io(e.to_string()))?;

    // Take the exclusive lock BEFORE recording our pid: the pidfile then always names
    // the process that actually holds the lock, and a failed contender reads the real
    // holder's pid instead of clobbering the file.
    match file.try_lock_exclusive() {
        Ok(()) => {
            std::fs::write(pid_path, format!("{}", std::process::id()))
                .map_err(|e| LockError::Io(e.to_string()))?;
            Ok(file)
        }
        Err(e) => {
            let held = std::fs::read_to_string(pid_path)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if held != 0 && held != std::process::id() {
                Err(LockError::Busy(held))
            } else {
                Err(LockError::Flock(e.to_string()))
            }
        }
    }
}
```

Delete `is_alive` (lines 74-79) [E9, E13] and adjust the module doc: replace "If the recorded PID is dead, the lock is considered stale and taken over." with "A stale lock is released by the OS when its holder dies, so acquiring the flock is the authority on liveness; the pidfile only names the holder for friendly messages."

- [ ] **Step 3: Portable shutdown check in the test.** In `tests/cli.rs`, replace the pidfile-read + `/proc/{pid}` poll block (lines ~113-137) with a socket-removal poll — the accept loop removes the socket at exit (`src/ipc/server.rs:80` [E61]):

```rust
// The daemon exits on its own shortly after the shutdown request; the accept loop
// removes the socket file at exit, so its disappearance is the cross-platform signal
// (a /proc/<pid> existence check does not work on macOS).
let mut gone = false;
for _ in 0..50 {
    if !sock.exists() {
        gone = true;
        break;
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
}
assert!(gone, "daemon did not shut down (socket still present) after the shutdown request");
```

`sock` is already in scope (bound at tests/cli.rs:65-71 [E70]); remove the now-unused pidfile read + `pid` variable.

- [ ] **Step 4: Verify**
Run: `cargo test --test cli`
Expected: `1 passed` — red→green proof of this task on macOS; the same assertion ("already running") remains the expected text on Linux.
Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && echo GATES-OK`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/lock.rs tests/cli.rs
git commit -m "fix(daemon): take the flock before writing the pidfile; drop /proc pre-check

A second daemon now reports the real holder (Busy) on every platform. Previously
is_alive probed /proc (Linux-only), so on macOS the loser clobbered the pidfile
and leaked the raw flock error; it also falsely refused startup when a stale
pidfile named a recycled pid while the lock was free. Lock acquisition itself is
the liveness authority. tests/cli.rs waits for socket removal (portable).
Verified: cargo test --test cli 1/1 (was failing on macOS); gates clean."
```

### Task 3 — Worktree event-bus test: canonicalize the lookup key

**Owner:** @worker · **Parallel group:** PG1 (disjoint files, but run after Task 2: its Step 3 gate runs the whole suite) · **Depends on:** Tasks 1–2

**Files:**
- Modify: `tests/task_2_3.rs:67-68` [E18] — the line `let got = wait_dirty(&store, &rt.join("wt-a"), Duration::from_secs(8));`.

**Interfaces:** none (test-local). The `events.subject` keys written by the bus are the git-reported worktree path strings [E14, E16, E17]; the product is self-consistent on every platform and is NOT changed — only the test's assumption about path form is.

- [ ] **Step 1: Confirm the red** — Run: `cargo test --test task_2_3 -- --test-threads=1 2>&1 | tail -3`
Expected: `a_worktree_change_is_persisted_by_the_event_bus` FAILS ("no worktree_dirty event persisted within 8s; view was empty") [E15]. Root cause: `git worktree list --porcelain` reports resolved `/private/var/...` paths for a `$TMPDIR`-created repo while `TempDir` hands out the `/var/folders/...` form [E14, E18]. On CI's OS the strings coincide (tmpdir not symlinked), which is why CI is green [E18].

- [ ] **Step 2: Canonicalize the expected key** (fix pre-verified twice in a scratch copy: 2/2 in 2.84s [E19]):

```rust
// git reports worktree paths resolved (e.g. /private/var/... under a macOS TMPDIR
// symlink) while GitRepo hands out the unresolved form; compare canonicalized paths.
let wt_a = std::fs::canonicalize(rt.join("wt-a")).unwrap_or_else(|_| rt.join("wt-a"));
let got = wait_dirty(&store, &wt_a, Duration::from_secs(8));
```

- [ ] **Step 3: Full baseline gate** (the "baseline is green" checkpoint)
Run: `cargo test`
Expected: all suites green — 31 passed, 0 failed (29 already-green [caller-verified] + the 2 repaired here).
Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && echo GATES-OK`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add tests/task_2_3.rs
git commit -m "fix(test): canonicalize the worktree key (macOS /private tmpdir paths)

git worktree list reports resolved paths; the lookup used the tempfile form.
Verified: cargo test --test task_2_3 -- --test-threads=1 2/2 (twice); full
cargo test 31/31; fmt/clippy gates clean."
```

---

### Task 4 — `ra sessions` vertical slice: parser → store → daemon ticker → RPC → CLI

**Owner:** @worker · **Depends on:** Tasks 1–3 (green baseline for attribution). Internally strictly ordered 4.1→4.7. Run `cargo fmt` after every code step below — the gates check it.

**Data design — rows.**

- `sessions` row (schema exists, NO migration [E22]): `id` = header uuid (36 chars, e.g. `01a1249c-7c2c-75b4-9cf9-0a646cce0ffa` — not a 12-char short id); `agent` = `"pi"`; `file` = absolute session-file path; `cwd` = header cwd verbatim (may be absent); `worktree_id` = mapped worktree or NULL; `started_ms` = header timestamp; `last_event_ms` = max entry timestamp; `tokens_in/out/cache` = sums over assistant messages of `message.usage.{input,output,cacheRead}` [E36 token rule: ignore `reasoning`/`totalTokens`; `cacheWrite` remains 0 in all fixtures, still summed]; `cost_usd` = sum of `usage.cost.total` when present else NULL [E36]; `tool_calls` = count of `toolCall` blocks [E34]; `parse_errors` = per rules below; `byte_offset` = file size at last successful ingest.
- `tool_calls` row [E23] (one per assistant `toolCall` block): `ts_ms` = entry timestamp (fallback `message.timestamp` [E34]); `tool` = block `name`; `args_hash` = first 16 hex of sha256 over canonical JSON of `arguments` [E57 + E36-bonus]; `is_error` / `result_hash` = filled only when the matching `toolResult` is parsed in the same pass (matched by `toolCallId`; `result_hash` = same scheme over the result's `message.content`) [E34, E36]. There is deliberately NO `call_id` column [E23] — see the ingest model.

**Ingest model (byte-offset semantics), decided with the rejected alternative in mind.** Skip-cursor + full re-parse: per file, if stored `byte_offset == current file size` → skip (unchanged); else re-parse the whole file, recompute all aggregates from scratch, and atomically replace the session row and its tool_calls. Rejected: incremental tail-append — `tool_calls` has no call-id [E23], so a `toolResult` arriving in a later chunk could not be joined to its call row through SQL, and partial-chunk pairing would strand results. Full re-parse of only-changed files makes pairing + dedupe trivial, self-heals truncation/rewrite, and is milliseconds at this machine's real corpus scale (363M total; unchanged files skip on the size check) [E39, A7]. Rules adopted from the advisor: **torn-tail rule** — only `\n`-terminated lines are parsed; a partial final line is ignored and is NOT a parse error (else an actively-written session would flap `parse_errors` every tick); **no-header skip writes no byte_offset**; **one write transaction per file**; **per-file error isolation** (unreadable/unstat-able file: `eprintln!` warn + continue; a panic must never kill the pass). Known accepted gaps (documented, tested where cheap): same-size rewrites are treated as unchanged; a deleted session file leaves its row until a future retention task (out of scope); a file rewritten with a different session id leaves one row per id only because sync deletes stale sessions rows with the same `file` and different `id`.

**Parse-error rules (exact).** `parse_errors` += 1 for a line that (a) fails JSON decoding, or (b) decodes but has no `type`, or (c) has a `type` string unknown to this parser (honest error count: repo-assist says what it doesn't understand). `compaction` / `branch_summary` are known-but-unsupported: skipped silently, NOT errors [E36]. `model_change` / `thinking_level_change` / non-first `session` lines: no aggregates [E36]. A file whose first line is not a `type:"session"` header, or is a `version != 3` header, is skipped whole: no row, no offset write [E36]. Rules for calls: duplicate `toolCallId`s each produce a row; its `toolResult` updates the **last** row carrying that id in the pass; an orphan `toolResult` (no call in the same pass) is dropped — both asserted in tests. A file may contain a `session` header only on line 1.

**Sessions→worktrees mapping (the verified-facts Item 3 posture).** Rule [E37, A4]: candidates = `worktrees` rows with `present = 1`, refreshed at the start of every sync by `git::sync_worktrees` [E59] (run_foreground itself does not populate worktrees [E46]); canonicalize both sides via `crate::paths::canonicalize` (falls back to the original for non-existent paths such as the redacted `<home>` cwd); a worktree matches when its canonical path equals or is a **component-wise prefix** of the canonical session cwd (`Path::starts_with` — `/wt/foo` can never match `/wt/foobar`); longest matching path wins; **no match → `worktree_id` NULL — never force-joined** [E37]. The redacted fixtures all carry cwd `<home>` (E35), so they MUST land NULL, and the tests assert exactly that; a synthetic session whose cwd is the worktree path (and one nested deeper) exercises the matched path. This is the head-on answer to Item 3 being unverified: unmapped sessions are expected, acceptable, and visible (NULL + `-` in output).

**Sessions root resolution (verified against pi's own source [E63, E64]):** `PI_CODING_AGENT_SESSION_DIR` if set/non-empty → else `$PI_CODING_AGENT_DIR/sessions` if set → else `$ENV_HOME/.pi/agent` + `/sessions` where `ENV_HOME` comes from `crate::paths::Env::home` [E50]. (`~`-prefixed values are expanded against `env.home` like pi's `expandTildePath`.) Tests inject fixture roots via `PI_CODING_AGENT_SESSION_DIR` on the spawned daemon child (children inherit env [E45]); this also proves test isolation — no test ever scans the real `~/.pi`.

**Discovery:** scan `root` itself and each **immediate** subdirectory for top-level `*.jsonl` files; skip `*.meta.json` (different extension), skip directories like `subagents/`, non-recursive [E38]; result sorted for determinism.

**Files (all sub-tasks), plus a blast-radius note:** flipping `Sessions` to `implemented: true` and starting a ticker in every `run_foreground` cannot break the existing suites — no test asserts the not-yet-implemented stub or any session-count value, and every existing daemon-spawning test isolates HOME/XDG env (its sessions root therefore resolves into its own temp state dir, never the real corpus) [E68].

- Create: `src/sources/sessions.rs`, `src/daemon/sessions.rs`, `tests/task_4.rs`
- Modify: `src/sources/mod.rs:5` (+`pub mod sessions;`), `src/daemon/mod.rs:15` (+`pub mod sessions;`) and `:86-89` (start ticker after `start_bus` [E46]), `src/store/mod.rs` (new types + methods placed after `append_evidence` near line 312, inside `impl Store` and the types below it [E26, E27]), `src/ipc/server.rs` (arm after the `Method::Drift` arm in `dispatch_request` [E43-region :226-231]; `sessions_json` after `plans_json` [E43 :311-331]), `src/cli.rs` (`:248-252` spec arm [E41]; run-match arm [E44]; `run_sessions` after `run_drift`; `print_sessions_human` after `print_plan_human`), `Cargo.toml:33` (time features).

**Interfaces this task produces (exact signatures the tests rely on):**

```rust
// src/sources/sessions.rs
pub const AGENT: &str = "pi";
pub fn resolve_sessions_root(env: &crate::paths::Env) -> std::path::PathBuf;
pub fn discover_session_files(root: &std::path::Path) -> Vec<std::path::PathBuf>;
pub fn ingest_file(path: &std::path::Path) -> Option<Ingested>;
pub fn sync_sessions(store: &crate::store::Store, repo_root: &std::path::Path, sessions_root: &std::path::Path);
pub struct Ingested { pub id: String, pub cwd: Option<String>, pub started_ms: Option<i64>,
    pub last_event_ms: Option<i64>, pub tokens_in: i64, pub tokens_out: i64, pub tokens_cache: i64,
    pub cost_usd: Option<f64>, pub parse_errors: i64, pub calls: Vec<CallInsert> }
pub struct CallInsert { pub ts_ms: i64, pub tool: String, pub args_hash: String,
    pub is_error: Option<bool>, pub result_hash: Option<String> }

// src/store/mod.rs
pub struct SessionRow<'a> { pub id: &'a str, pub agent: &'a str, pub file: &'a str,
    pub cwd: Option<&'a str>, pub worktree_id: Option<i64>, pub started_ms: Option<i64>,
    pub last_event_ms: Option<i64>, pub tokens_in: i64, pub tokens_out: i64, pub tokens_cache: i64,
    pub cost_usd: Option<f64>, pub tool_calls: i64, pub byte_offset: i64, pub parse_errors: i64 }
pub struct ToolCallRow<'a> { pub ts_ms: i64, pub tool: &'a str, pub args_hash: &'a str,
    pub is_error: Option<bool>, pub result_hash: Option<&'a str> }
impl Store {
    pub fn upsert_session_with_calls(&self, row: &SessionRow<'_>, calls: &[ToolCallRow<'_>]) -> Result<(), StoreError>;
    pub fn session_file_offsets(&self) -> Result<std::collections::HashMap<String, i64>, StoreError>;
}

// src/daemon/sessions.rs
pub const TICK_INTERVAL: std::time::Duration; // 60s
pub struct SessionsTicker;
impl SessionsTicker { pub fn start(store: Arc<Store>, repo_root: PathBuf, sessions_root: PathBuf) -> SessionsTicker; }
impl Drop for SessionsTicker;
```

#### Sub-task 4.1 — Parser: `ingest_file` over fixtures (TDD)

- [ ] **Step 1: Write the failing tests** in `tests/task_4.rs` (create the file with `mod common;` + helpers; the exact expected numbers below were computed from the committed redacted fixtures [E30–E33, E66] — they are this task's acceptance numbers):

```rust
//! Task 4 acceptance: the pi session source parses the committed fixtures exactly,
//! records parse errors honestly, maps sessions to worktrees only when the cwd really
//! falls inside one, and serves `ra sessions [--json]` end to end over IPC.

mod common;

use std::path::{Path, PathBuf};

/// Committed redacted fixtures only [E29, E66].
fn redacted_fixtures() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().ends_with(".redacted.jsonl"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

/// The three fixture session ids, written once and reused by every test below (so a
/// transcribed id can never drift between tests) [cold-review F1].
const F1_ID: &str = "01a1249c-7c2c-75b4-9cf9-0a646cce0ffa";
const F2_ID: &str = "01a1249c-ec21-7703-8773-813f926a6357";
const F3_ID: &str = "01a1249d-14fb-744b-9318-378a5372ccfd";

/// Deferred to the 4.4 suite (defined there verbatim) [cold-review F3]: the helpers
/// `sessions_root_with_fixtures()` (a private root mirroring pi's real layout —
/// `<root>/--Users-jesse--/<file>.jsonl` plus a `.meta.json` twin and a `subagents/`
/// dir to prove they are skipped [E38]) and `open_store(&common::GitRepo)` (opens
/// `<repo>/state/db.sqlite`, migrated). Defining them earlier would be dead code and
/// break the `-D warnings` gates of the 4.1–4.3 sub-steps; the consts above and
/// `redacted_fixtures` stay because 4.1 uses them immediately. (`mod common;` is safe to
/// declare now: its file carries `#![allow(dead_code)]` internally [read tests/common/mod.rs:8].)

#[test]
fn parser_reads_the_redacted_fixtures_exactly() {
    // (id, tokens_in, tokens_out, tokens_cache, tool_calls, started_ms, last_event_ms) per fixture, in sorted filename order [E30, E31, E32, E33, E71].
    let expected = [
        (F1_ID, 11436, 2253, 32640, 4, 1791615597613_i64, 1791615612968_i64),
        (F2_ID, 4999, 886, 9344, 2, 1791615626273, 1791615634451),
        (F3_ID, 8609, 2286, 17536, 3, 1791615636731, 1791615651076),
    ];
    for (i, f) in redacted_fixtures().iter().enumerate() {
        let ing = repo_assist::sources::sessions::ingest_file(f)
            .unwrap_or_else(|| panic!("fixture {:?} should parse", f));
        let (id, tin, tout, tcache, calls, started, last) = expected[i];
        assert_eq!(ing.id, id, "session id in {f:?}");
        assert_eq!(ing.tokens_in, tin, "tokens_in in {f:?}");
        assert_eq!(ing.tokens_out, tout, "tokens_out in {f:?}");
        assert_eq!(ing.tokens_cache, tcache, "tokens_cache in {f:?}");
        assert_eq!(ing.calls.len(), calls, "toolCall block count in {f:?}");
        assert_eq!(ing.started_ms, Some(started), "started_ms in {f:?}");
        assert_eq!(ing.last_event_ms, Some(last), "last_event_ms in {f:?}");
        assert_eq!(ing.parse_errors, 0, "fixtures parse cleanly [E36]");
        assert_eq!(ing.cost_usd, Some(0.0), "ollama fixtures carry cost total 0 [E30]");
        assert_eq!(ing.cwd.as_deref(), Some("<home>"), "redacted cwd [E35]");
        // every call found its result in the same pass, none errored [E30–E32: 4/4, 2/2, 3/3]
        assert!(ing.calls.iter().all(|c| c.is_error == Some(false) && c.result_hash.is_some()),
            "every call must carry its result linkage: {:?}", ing.calls);
    }
}
```

- [ ] **Step 2: Run to verify it fails** — Run: `cargo test --test task_4 2>&1 | tail -5`
Expected: compile error (module `sources::sessions` does not exist) — that is the red state.

- [ ] **Step 3: Implement the parser** — `src/sources/sessions.rs` (module doc comment cites Task 4 / Section 8.2 and states the read-only guarantee [E51, E48]):

```rust
//! Agent session source (Task 4 / Section 8.2): parse pi session JSONL files into
//! `sessions` + `tool_calls` aggregates. Files are only ever READ — the daemon never
//! writes inside any work tree, and that includes session files [E51].

use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
// (Sub-task 4.4 adds `use std::path::PathBuf; use crate::paths::{self, Env};
//  use crate::store::Store;` — import each name when first used so the `-D warnings`
//  gate stays green at every sub-task.)

/// The agent this source ingests (`sessions.agent`).
pub const AGENT: &str = "pi";

/// The session header — the first line of a session file (strict struct).
#[derive(Debug, Deserialize)]
struct SessionHeader {
    #[serde(rename = "type")]
    kind: String,
    /// pi session-log version; 3 is the only shape this parser claims to know [E36].
    version: i64,
    id: String,
    timestamp: String,
    cwd: Option<String>,
}

/// One assistant `toolCall` block, flattened to a `tool_calls` row.
#[derive(Debug)]
pub struct CallInsert {
    pub ts_ms: i64,
    pub tool: String,
    pub args_hash: String,
    pub is_error: Option<bool>,
    pub result_hash: Option<String>,
}

/// Aggregates extracted from one fully-parsed session file.
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

/// RFC 3339 -> epoch ms ("2026-10-10T06:59:57.613Z" -> 1791615597613 [E33]).
fn epoch_ms(ts: &str) -> Option<i64> {
    let dt = time::OffsetDateTime::parse(
        ts,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()?;
    Some(dt.unix_timestamp() * 1000 + i64::from(dt.millisecond()))
}

/// First 16 hex chars of sha256 over canonical JSON (serde_json orders object keys sorted).
fn hash16(v: serde_json::Value) -> String {
    let mut h = hex::encode(Sha256::digest(
        serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()),
    ));
    h.truncate(16);
    h
}

/// Ingest one session file. Only `\n`-terminated lines are parsed: a torn tail (a
/// writer mid-append) is ignored and is NOT a parse error. Returns `None` when the file
/// has no parseable v3 session header (nothing to key a row on — callers leave no offset).
pub fn ingest_file(path: &Path) -> Option<Ingested> {
    let raw = std::fs::read_to_string(path).ok()?;
    let complete_end = raw.rfind('\n').map(|i| i + 1).unwrap_or(0); // torn-tail rule
    parse_file_lines(&raw[..complete_end])
}

/// Parse the complete lines of one session file. `None` = no v3 header found/valid.
fn parse_file_lines(complete: &str) -> Option<Ingested> {
    let mut id: Option<String> = None;
    let mut have_header = false; // a v3 header is accepted exactly once (first session line)
    let mut cwd: Option<String> = None;
    let mut started_ms: Option<i64> = None;
    let mut last_event_ms: Option<i64> = None;
    let mut tokens_in = 0;
    let mut tokens_out = 0;
    let mut tokens_cache = 0;
    let mut cost_usd: Option<f64> = None;
    let mut parse_errors = 0;
    let mut calls: Vec<CallInsert> = Vec::new();
    // toolCallId -> index into `calls`, so a later toolResult can link back in this pass.
    let mut pending: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for line in complete.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                parse_errors += 1; // rule (a): undecodable JSON
                continue;
            }
        };
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()).and_then(epoch_ms) {
            last_event_ms = Some(last_event_ms.map_or(ts, |m: i64| m.max(ts)));
        }
        match kind {
            "session" => match serde_json::from_value::<SessionHeader>(v.clone()) {
                // Header on the FIRST session line only [E36 + review cold-F2]: later
                // `session` lines are ignored, so a file's row key can never flap — this
                // also keeps the skip cursor (`read_session_id`, line 1) consistent with
                // the ingested id.
                Ok(h) if !have_header && h.kind == "session" && h.version == 3 => {
                    have_header = true;
                    id = Some(h.id);
                    cwd = h.cwd;
                    started_ms = epoch_ms(&h.timestamp);
                }
                _ => {}
            },
            "message" => {
                let msg = &v["message"];
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let ts = v
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(epoch_ms)
                    .or_else(|| msg.get("timestamp").and_then(|t| t.as_str()).and_then(epoch_ms))
                    .unwrap_or(0);
                if role == "assistant" {
                    if let Some(u) = msg.get("usage").and_then(|u| u.as_object()) {
                        tokens_in += u.get("input").and_then(|x| x.as_i64()).unwrap_or(0);
                        tokens_out += u.get("output").and_then(|x| x.as_i64()).unwrap_or(0);
                        tokens_cache += u.get("cacheRead").and_then(|x| x.as_i64()).unwrap_or(0);
                        if let Some(total) = u.get("cost").and_then(|c| c.get("total")).and_then(|x| x.as_f64()) {
                            cost_usd = Some(cost_usd.unwrap_or(0.0) + total);
                        }
                    }
                    if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                        for b in blocks {
                            if b.get("type").and_then(|t| t.as_str()) != Some("toolCall") {
                                continue;
                            }
                            let call_id = b.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                            pending.insert(call_id, calls.len());
                            calls.push(CallInsert {
                                ts_ms: ts,
                                tool: b.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string(),
                                args_hash: hash16(b.get("arguments").cloned().unwrap_or(serde_json::Value::Null)),
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
                                msg.get("content").cloned().unwrap_or(serde_json::Value::Null),
                            ));
                        } // orphan results (no call in this pass) are dropped
                    }
                } // other roles contribute nothing
            }
            "compaction" | "branch_summary" => {} // known-but-unsupported: skip, not an error [E36]
            "model_change" | "thinking_level_change" => {}
            "" => parse_errors += 1, // rule (b): no `type` at all
            _ => parse_errors += 1,   // rule (c): unknown kind — honest count
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
```

Also do in this step: `pub mod sessions;` in `src/sources/mod.rs` [E48], and the `time` features line in `Cargo.toml` → `time = { version = "0.3", features = ["formatting", "macros", "parsing"] }`.

- [ ] **Step 4: Run to verify it passes** — Run: `cargo test --test task_4 2>&1 | tail -3`
Expected: the fixture test passes; expected failures only in the not-yet-written tests (none yet — just this one test). Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && echo GATES-OK` → clean.

#### Sub-task 4.2 — Edge tests for the parser (error accounting, torn tail, orphans, dupes)

- [ ] **Step 1: Add these tests** to `tests/task_4.rs` (synthetic minimal session lines):()
```rust
fn hdr(id: &str, cwd: &str) -> String {
    serde_json::json!({
        "type": "session", "version": 3, "id": id,
        "timestamp": "2026-10-19T00:00:00.000Z", "cwd": cwd
    })
    .to_string()
}
fn assistant_ok(ts: &str) -> String {
    serde_json::json!({
        "type": "message", "id": "m1", "parentId": null, "timestamp": ts,
        "message": {
            "role": "assistant",
            "content": [{ "type": "text", "text": "hi" }],
            "usage": { "input": 10, "output": 5, "cacheRead": 0, "cacheWrite": 0,
                        "cost": { "total": 0.02 } }
        }
    })
    .to_string()
}
fn call(ts: &str, cid: &str) -> String {
    serde_json::json!({
        "type": "message", "id": "m2", "parentId": null, "timestamp": ts,
        "message": {
            "role": "assistant",
            "content": [{ "type": "toolCall", "id": cid, "name": "read",
                          "arguments": { "path": "/x" } }],
            "usage": { "input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0,
                        "cost": { "total": 0 } }
        }
    })
    .to_string()
}
fn result(ts: &str, cid: &str, is_error: bool) -> String {
    serde_json::json!({
        "type": "message", "id": "m3", "parentId": null, "timestamp": ts,
        "message": {
            "role": "toolResult", "toolCallId": cid, "toolName": "read",
            "isError": is_error,
            "content": [{ "type": "text", "text": "done" }]
        }
    })
    .to_string()
}

#[test]
fn parse_errors_are_counted_and_torn_tails_are_not() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("edge.jsonl");
    let body = [
        hdr("s-edge", "/x"),
        assistant_ok("2026-10-19T00:00:01.000Z"),
        "NOT JSON AT ALL".to_string(),               // rule (a): +1
        serde_json::json!({"type":"compaction","id":"c"}).to_string(), // skipped, not an error
        serde_json::json!({"type":"totally_new_kind"}).to_string(),     // rule (c): +1
        serde_json::json!({"id":"m4"}).to_string(),                    // rule (b): no `type`: +1
        result("2026-10-19T00:00:02.000Z", "call_ghost", false), // orphan: dropped, no error
        call("2026-10-19T00:00:03.000Z", "call_a"),
        result("2026-10-19T00:00:04.000Z", "call_a", true),
    ].join("\n");
    // No trailing newline on the LAST line: a real assistant message (with usage input=10)
    // is the torn tail — if it were wrongly parsed, tokens_in below would be 21, not 11,
    // and parse_errors would grow by however it failed to decode. It must be ignored.
    // (The newline AFTER the body matters [review F9]: without it, `body`'s last complete
    // line — the call_a result — is welded onto the tail and lost.)
    std::fs::write(&f, format!("{body}\n{}", assistant_ok("2026-10-19T00:00:05.000Z"))).unwrap();
    let ing = repo_assist::sources::sessions::ingest_file(&f).expect("edge file parses");
    assert_eq!(ing.id, "s-edge");
    assert_eq!(ing.parse_errors, 3, "exactly rules (a),(b),(c) fire — the torn tail is NOT an error");
    assert_eq!(ing.tokens_in, 11, "assistant_ok (10) + the call's usage (1); the torn tail contributes nothing");
    assert_eq!(ing.tokens_out, 6);
    assert_eq!(ing.calls.len(), 1, "orphan result is dropped, one call parsed");
    assert_eq!(ing.calls[0].is_error, Some(true), "result linked to its call in-pass");
    assert!(ing.calls[0].result_hash.is_some());
    assert_eq!(cost(ing.cost_usd), 0.02);
}
fn cost(c: Option<f64>) -> f64 { c.unwrap_or(-1.0) }

#[test]
fn files_without_a_v3_header_are_skipped_whole() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("no_hdr.jsonl");
    std::fs::write(&f, format!("{}\n{}\n", assistant_ok("2026-10-19T00:00:00.000Z"), assistant_ok("2026-10-19T00:00:01.000Z"))).unwrap();
    assert!(repo_assist::sources::sessions::ingest_file(&f).is_none(), "no header -> no Ingested");
    let f2 = tmp.path().join("bad_ver.jsonl");
    std::fs::write(&f2, hdr("s9", "/x").replace("\"version\":3", "\"version\":9")).unwrap();
    assert!(repo_assist::sources::sessions::ingest_file(&f2).is_none(), "unsupported header version -> skip [E36]");
    let f3 = tmp.path().join("empty.jsonl");
    std::fs::write(&f3, "").unwrap();
    assert!(repo_assist::sources::sessions::ingest_file(&f3).is_none());
}

#[test]
fn duplicate_call_ids_each_row_and_last_one_gets_the_result() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("dupes.jsonl");
    std::fs::write(&f, format!(
        "{}\n{}\n{}\n{}\n",
        hdr("s-dup", "/x"),
        call("2026-10-19T00:00:01.000Z", "call_x"),
        call("2026-10-19T00:00:02.000Z", "call_x"),
        result("2026-10-19T00:00:03.000Z", "call_x", true),
    )).unwrap();
    let ing = repo_assist::sources::sessions::ingest_file(&f).unwrap();
    assert_eq!(ing.calls.len(), 2, "duplicate toolCallId -> both rows [duplicate rule]");
    assert_eq!(ing.calls[0].is_error, None, "first dup keeps no linkage");
    assert_eq!(ing.calls[1].is_error, Some(true), "last dup receives the result");
}
```

- [ ] **Step 2: Run** — `cargo test --test task_4` → all four tests pass; gates clean.

#### Sub-task 4.3 — Store methods (TDD)

- [ ] **Step 1: Write the failing store test** (mirrors `tests/store.rs` raw-SQL read-back style [E53-adjacent, E27]):()```rust
#[test]
fn upsert_session_with_calls_replaces_rows_and_is_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("db.sqlite");
    let store = repo_assist::store::Store::open(&db).unwrap();
    let row = repo_assist::store::SessionRow {
        id: "s1", agent: "pi", file: "/tmp/s1.jsonl", cwd: Some("/w"),
        worktree_id: None, started_ms: Some(1), last_event_ms: Some(2),
        tokens_in: 1, tokens_out: 2, tokens_cache: 3, cost_usd: Some(0.5),
        tool_calls: 1, byte_offset: 100, parse_errors: 0,
    };
    let calls = [repo_assist::store::ToolCallRow {
        ts_ms: 2, tool: "read", args_hash: "a1b2c3d4e5f60718",
        is_error: Some(false), result_hash: None,
    }];
    store.upsert_session_with_calls(&row, &calls).unwrap();

    // Second ingest of the same file with different totals and no calls: everything is
    // replaced; stale tool_calls rows must not survive (full re-parse semantics).
    let row2 = repo_assist::store::SessionRow {
        id: "s1", agent: "pi", file: "/tmp/s1.jsonl", cwd: Some("/w"),
        worktree_id: None, started_ms: Some(1), last_event_ms: Some(2),
        tokens_in: 9, tokens_out: 2, tokens_cache: 3, cost_usd: None,
        tool_calls: 0, byte_offset: 200, parse_errors: 1,
    };
    store.upsert_session_with_calls(&row2, &[]).unwrap();

    let (tin, off, pe, calls): (i64, i64, i64, i64) = store
        .read(|conn| {
            let (tin, off, pe) = conn.query_row(
                "SELECT tokens_in, byte_offset, parse_errors FROM sessions WHERE id = 's1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let calls = conn.query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE session_id = 's1'",
                [],
                |r| r.get(0),
            )?;
            Ok((tin, off, pe, calls))
        })
        .unwrap();
    assert_eq!((tin, off, pe, calls), (9, 200, 1, 0), "second upsert wins; calls replaced");

    // A rewritten file with a different id must not leave stale rows behind.
    let row3 = repo_assist::store::SessionRow {
        id: "s2", agent: "pi", file: "/tmp/s1.jsonl", cwd: None, worktree_id: None,
        started_ms: None, last_event_ms: None, tokens_in: 0, tokens_out: 0, tokens_cache: 0,
        cost_usd: None, tool_calls: 0, byte_offset: 5, parse_errors: 0,
    };
    store.upsert_session_with_calls(&row3, &[]).unwrap();
    let n: i64 = store
        .read(|conn| conn.query_row("SELECT COUNT(*) FROM sessions WHERE file = '/tmp/s1.jsonl'", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(n, 1, "same file with a new session id replaces the old row");

    let offsets = store.session_file_offsets().unwrap();
    assert_eq!(offsets.get("s1"), None, "s1 was replaced away");
    assert_eq!(offsets.get("s2"), Some(&5));
}
```

- [ ] **Step 2: Run to verify it fails** (compile error: `SessionRow` etc. don't exist).
- [ ] **Step 3: Implement** in `src/store/mod.rs` — types after `EvidenceRow` [E27], methods after `append_evidence` [E26]:()
```rust
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
```

and inside `impl Store` (after `append_evidence`):()```rust
    /// Upsert one `sessions` row and REPLACE its `tool_calls` rows in a single write
    /// transaction (Task 4): per-file atomicity — readers never see half a session. A
    /// file rewritten with a different session id drops the previous row (`file` match),
    /// cleaning that stale row's tool_calls EXPLICITLY first (not relying on ON DELETE
    /// CASCADE — although foreign_keys is ON for the writer [E69]) [cold-review F5].
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
            // A rewritten file may carry a different session id: clean up the stale rows'
            // tool_calls explicitly, then the stale session rows.
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

    /// `sessions.id -> byte_offset` map — the skip cursor the session source consults
    /// before re-parsing a file (Task 4).
    pub fn session_file_offsets(
        &self,
    ) -> Result<std::collections::HashMap<String, i64>, StoreError> {
        self.read(|conn| {
            let mut stmt = conn.prepare("SELECT id, byte_offset FROM sessions")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            Ok(rows.collect::<Result<std::collections::HashMap<_, _>, _>>()?)
        })
    }
```

- [ ] **Step 4: Run** — `cargo test --test task_4` → green including the store test; gates clean.

#### Sub-task 4.4 — Discovery, sync, and worktree mapping (TDD)

- [ ] **Step 1: Write the failing tests.** First add the two deferred helpers verbatim at the top of the file (this is where they first get used [cold-review F3]):

```rust
/// A private sessions root mirroring pi's real layout: `<root>/--Users-jesse--/<file>.jsonl`
/// (plus `.meta.json` twins and a `subagents/` dir to prove they are skipped) [E38].
fn sessions_root_with_fixtures() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("sessions").join("--Users-jesse--");
    std::fs::create_dir_all(&dir).unwrap();
    for f in redacted_fixtures() {
        std::fs::copy(&f, dir.join(f.file_name().unwrap())).unwrap();
    }
    // Layout noise the discovery must skip.
    std::fs::write(dir.join("noise.meta.json"), "{}").unwrap();
    std::fs::create_dir_all(dir.join("subagents")).unwrap();
    std::fs::write(dir.join("subagents/inner.jsonl"), "{\"type\":\"session\"}").unwrap();
    (tmp, tmp.path().join("sessions"))
}

fn open_store(repo: &common::GitRepo) -> repo_assist::store::Store {
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    repo_assist::store::Store::open(&db).unwrap()
}
```

then the tests:

```rust
#[test]
fn discovery_returns_top_level_jsonl_only_and_resolves_the_root() {
    let (_tmp, root) = sessions_root_with_fixtures();
    let files = repo_assist::sources::sessions::discover_session_files(&root);
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), 3, ".meta.json and subagents/ are skipped [E38]: {names:?}");
    assert!(names.iter().all(|n| n.ends_with(".redacted.jsonl")));
    assert!(files.iter().all(|f| f.is_file()));

    // Root resolution follows pi exactly [E63, E64]: env SESSION_DIR > env AGENT_DIR/sessions > HOME default.
    // (set_var is process-global. It is safe here because (a) the only in-process reader of
    // these two variables is this test itself, and (b) every daemon child spawned by the
    // e2e test sets PI_CODING_AGENT_SESSION_DIR explicitly, and SESSION_DIR has top
    // precedence in the child's resolution — the momentary in-process value of
    // PI_CODING_AGENT_DIR cannot change what any spawned daemon ingests.)
    std::env::set_var("PI_CODING_AGENT_SESSION_DIR", "/opt/somewhere");
    let env = repo_assist::paths::Env::with("/home/u", None, None);
    assert_eq!(
        repo_assist::sources::sessions::resolve_sessions_root(&env),
        std::path::PathBuf::from("/opt/somewhere")
    );
    std::env::remove_var("PI_CODING_AGENT_SESSION_DIR");
    std::env::set_var("PI_CODING_AGENT_DIR", "/opt/agent");
    assert_eq!(
        repo_assist::sources::sessions::resolve_sessions_root(&env),
        std::path::PathBuf::from("/opt/agent/sessions")
    );
    std::env::remove_var("PI_CODING_AGENT_DIR");
    assert_eq!(
        repo_assist::sources::sessions::resolve_sessions_root(&env),
        std::path::PathBuf::from("/home/u/.pi/agent/sessions")
    );
}

#[test]
fn sync_ingests_fixtures_with_expected_totals_and_null_worktree() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let store = open_store(&repo);
    let (_tmp, root) = sessions_root_with_fixtures();

    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);

    // All three fixture sessions are ingested with their exact aggregates [E30–E33].
    let rows: Vec<(String, i64, i64, i64, i64, Option<i64>)> = store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT s.id, s.tokens_in, s.tokens_out, s.tokens_cache, s.tool_calls, s.worktree_id FROM sessions s ORDER BY s.id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    assert_eq!(rows.len(), 3, "exactly the three fixture sessions (no .meta/subagents)");
    let by_id = |id: &str| rows.iter().find(|r| r.0 == id).unwrap().clone();
    let f1 = by_id(F1_ID);
    assert_eq!((f1.1, f1.2, f1.3, f1.4), (11436, 2253, 32640, 4));
    // Redacted cwds are `<home>` — they MUST map to no worktree [E35, E37].
    for r in &rows {
        assert_eq!(r.5, None, "fixture sessions map to no worktree (Item 3 posture)");
    }
    // tool_calls rows ride along: 4+2+3 = 9
    let calls: i64 = store
        .read(|conn| conn.query_row("SELECT COUNT(*) FROM tool_calls", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(calls, 9);
}

#[test]
fn sync_maps_sessions_inside_a_worktree_by_component_prefix() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-m", "m.txt");
    // `wt-m2` is a registered worktree whose path STRING-extends `wt-m` — the boundary case
    // a string-prefix comparison would flunk; Path::starts_with is component-wise.
    repo.add_worktree("wt-m2", "m2.txt");
    let store = open_store(&repo);
    let (_tmp, root) = sessions_root_with_fixtures();
    let dir = root.join("--tmp--");
    std::fs::create_dir_all(&dir).unwrap();
    let wt = std::fs::canonicalize(repo.path().join("wt-m")).unwrap();
    let wt2 = std::fs::canonicalize(repo.path().join("wt-m2")).unwrap();

    // sessions run AT the worktree root, DEEPER inside it [A4 rule], inside wt-m2,
    // and at a path that is only a STRING prefix of wt-m (it must NOT match wt-m; it does
    // fall component-wise inside the repo root, so the honest expectation is the MAIN
    // worktree — asserted below, per review F1).
    std::fs::write(
        dir.join("at-root.jsonl"),
        format!("{}\n{}\n", hdr("s-at-root", wt.to_str().unwrap()), assistant_ok("2026-10-19T00:00:01.000Z")),
    ).unwrap();
    std::fs::write(
        dir.join("nested.jsonl"),
        format!("{}\n{}\n", hdr("s-nested", wt.join("sub/dir").to_str().unwrap()), assistant_ok("2026-10-19T00:00:02.000Z")),
    ).unwrap();
    std::fs::write(
        dir.join("in-wt2.jsonl"),
        format!("{}\n{}\n", hdr("s-in-wt2", wt2.join("sub").to_str().unwrap()), assistant_ok("2026-10-19T00:00:03.000Z")),
    ).unwrap();
    let fool_cwd = format!("{}-suffix", wt.to_string_lossy());
    std::fs::write(
        dir.join("fool.jsonl"),
        format!("{}\n{}\n", hdr("s-fool", &fool_cwd), assistant_ok("2026-10-19T00:00:04.000Z")),
    ).unwrap();

    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);

    // worktree id map keyed by the git-reported canonical path (the store's `worktrees.path`)
    let id_of: std::collections::HashMap<String, i64> = store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT id, path FROM worktrees")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            Ok(rows.collect::<Result<std::collections::HashMap<_, _>, _>>()?)
        })
        .unwrap();
    let wt_m_id = id_of.get(wt.to_str().unwrap()).copied().expect("wt-m row exists");
    let wt_m2_id = id_of.get(wt2.to_str().unwrap()).copied().expect("wt-m2 row exists");
    let main_id = id_of
        .get(std::fs::canonicalize(repo.path()).unwrap().to_str().unwrap())
        .copied()
        .expect("the repo root is itself a present worktree (review F1)");
    assert_ne!(wt_m_id, wt_m2_id);
    assert_ne!(main_id, wt_m_id);

    // `store.read` closures must return `Result<T, rusqlite::Error>` [E25-signature]; a tiny
    // helper keeps each lookup in that shape without naming rusqlite types in the test crate.
    let wt_of = |sid: &str| {
        store
            .read(|conn| {
                conn.query_row(
                    "SELECT worktree_id FROM sessions WHERE id = ?1",
                    [sid],
                    |r| r.get(0),
                )
            })
            .unwrap()
    };
    assert_eq!(wt_of("s-at-root"), Some(wt_m_id), "session at the worktree root maps to it [E37 rule]");
    assert_eq!(wt_of("s-nested"), Some(wt_m_id), "nested cwd maps to the same, most-specific worktree");
    assert_eq!(wt_of("s-in-wt2"), Some(wt_m2_id), "cwd inside wt-m2 maps to wt-m2, NOT to wt-m (component boundary)");
    // The fool cwd is component-wise inside the MAIN worktree (the repo root) but only a
    // string-prefix of wt-m: it must map to the main worktree, never to wt-m [review F1].
    assert_eq!(wt_of("s-fool"), Some(main_id), "string-prefix cwd falls to main-worktree containment, not wt-m");
}

#[test]
fn sync_skips_unchanged_files_grows_on_append_and_resets_on_shrink() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-s", "s.txt");
    let store = open_store(&repo);
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("sessions");
    let dir = root.join("--x--");
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = redacted_fixtures()[1].clone(); // 10 lines, tokens_in 4999, calls 2
    let target = dir.join(fixture.file_name().unwrap());
    std::fs::copy(&fixture, &target).unwrap();

    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    let size = std::fs::metadata(&target).unwrap().len() as i64;
    let (tin, calls): (i64, i64) = read_agg(&store, F2_ID, &size);
    assert_eq!((tin, calls), (4999, 2), "first pass ingests the whole fixture [E31]");

    // Unchanged -> skip. Proof the skip cursor fired [review F8 + F10]: a manually inserted
    // marker tool_calls row for this session can ONLY survive if the sync did no store write
    // at all — a re-parse would DELETE+reinsert and wipe it. (`tool_calls.id` has no
    // AUTOINCREMENT [E23], so raw rowid equality cannot prove the skip; the marker can.)
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO tool_calls(session_id, ts_ms, tool, args_hash, is_error, result_hash) VALUES (?1, 0, 'marker', 'marker', NULL, NULL)",
                [F2_ID],
            )
        })
        .unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    assert_eq!(read_agg(&store, F2_ID, &size), (4999, 2));
    let marker: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1 AND tool = 'marker'",
                [F2_ID],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(marker, 1, "the byte-offset skip cursor prevented a re-parse (marker survived)");

    // Append one complete assistant message with usage 10 -> totals grow by that line only.
    let appended = assistant_ok("2026-10-19T00:00:09.000Z");
    let mut body = std::fs::read_to_string(&target).unwrap();
    body.push_str(&appended);
    body.push('\n');
    std::fs::write(&target, &body).unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    let new_size = std::fs::metadata(&target).unwrap().len() as i64;
    assert_eq!(read_agg(&store, F2_ID, &new_size), (4999 + 10, 2));

    // Shrink (truncate by one line) -> full recompute, totals drop, offset follows down.
    let body2 = std::fs::read_to_string(&target).unwrap();
    let kept: String = body2.lines().take(10).map(|l| format!("{l}\n")).collect();
    std::fs::write(&target, kept).unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    let shrunk = std::fs::metadata(&target).unwrap().len() as i64;
    assert_eq!(read_agg(&store, F2_ID, &shrunk), (4999, 2));
}
fn read_agg(store: &repo_assist::store::Store, id: &str, expect_off: &i64) -> (i64, i64) {
    let (tin, calls, off): (i64, i64, i64) = store
        .read(|conn| {
            conn.query_row(
                "SELECT s.tokens_in, s.tool_calls, s.byte_offset FROM sessions s WHERE s.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .unwrap();
    assert_eq!(off, *expect_off, "byte_offset tracks the file size (skip cursor)");
    (tin, calls)
}
```

- [ ] **Step 2: Run** — expect compile errors (`discover_session_files`, `resolve_sessions_root`, `sync_sessions` missing).
- [ ] **Step 3: Implement** the rest of `src/sources/sessions.rs`:()```rust
/// Resolve the pi sessions root the way pi itself does [E63, E64]:
/// `PI_CODING_AGENT_SESSION_DIR`, else `$PI_CODING_AGENT_DIR/sessions`, else
/// `$HOME/.pi/agent/sessions`. `~` values expand against `env.home` (pi's expandTildePath).
pub fn resolve_sessions_root(env: &Env) -> PathBuf {
    let expand = |dir: String| {
        PathBuf::from(if let Some(rest) = dir.strip_prefix("~/") {
            format!("{}/{}", env.home.trim_end_matches('/'), rest)
        } else {
            dir
        })
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

/// Every session file under `root`: top-level `*.jsonl` of `root` and of each immediate
/// subdirectory (the real layout is `<root>/<encoded-cwd>/<ts>_<uuid>.jsonl` [E38, E65]).
/// `*.meta.json` twins do not match the extension; `subagents/` is a directory one level
/// deeper than we scan. Sorted for deterministic behavior.
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
        let Ok(sub) = std::fs::read_dir(&d) else { continue };
        for f in sub.flatten() {
            let jsonl = f
                .path()
                .extension()
                .map(|e| e == "jsonl")
                .unwrap_or(false);
            if jsonl && f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                files.push(f.path());
            }
        }
    }
    files.sort();
    files
}

/// Component-wise worktree match: canonicalized worktree path equal to or a prefix of
/// the canonicalized session cwd; longest wins; no match -> None (never force-join) [E37].
/// `paths::canonicalize(&Path) -> PathBuf` falls back to the original for non-existent
/// paths [E72].
fn match_worktree(cwd: Option<&str>, worktrees: &[(i64, String)]) -> Option<i64> {
    let cwd = cwd?;
    let cwd = paths::canonicalize(Path::new(cwd));
    worktrees
        .iter()
        .filter(|(_, path)| {
            let wt = paths::canonicalize(Path::new(path));
            cwd.starts_with(&wt) // Path::starts_with is component-wise: /a/b never matches /a/bc
        })
        .max_by_key(|(_, path)| paths::canonicalize(Path::new(path)).as_os_str().len())
        .map(|(id, _)| *id)
}

/// One ingest pass: refresh worktree rows [E59], discover session files, skip unchanged
/// ones via the byte-offset cursor, and fully re-parse the rest. One write transaction per
/// file (advisor: no cross-file lock holds); per-file error isolation — a stat/read/store
/// failure logs and continues; this never panics and never aborts the pass.
pub fn sync_sessions(store: &Store, repo_root: &Path, sessions_root: &Path) {
    if let Err(e) = crate::sources::git::sync_worktrees(store, repo_root) {
        eprintln!("sessions sync: git source failed: {e}"); // mapping sees stale rows, ingest continues
    }
    let worktrees: Vec<(i64, String)> = store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, path FROM worktrees WHERE present = 1")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap_or_default();
    let offsets = store.session_file_offsets().unwrap_or_default();

    for file in discover_session_files(sessions_root) {
        let Ok(meta) = std::fs::metadata(&file) else {
            eprintln!("sessions sync: cannot stat {}: skipped", file.display());
            continue;
        };
        let size = meta.len() as i64;
        let Some(id) = read_session_id(&file) else {
            continue; // no parseable v3 header (or unreadable): skip WITHOUT caching an offset
        };
        if offsets.get(&id) == Some(&(size)) {
            continue; // unchanged since last pass: the byte_offset skip cursor [E22]
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
        let calls: Vec<crate::store::ToolCallRow<'_>> = ing
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
        if let Err(e) = store.upsert_session_with_calls(&row, &calls) {
            eprintln!("sessions sync: store write failed for {file_s}: {e}");
        }
    }
}

/// Read only the header line of a session file: its id, or None when absent/unparseable.
/// (The header is line 1, so this is O(1) regardless of file size — pi appends only.)
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
```

- [ ] **Step 4: Run** — `cargo test --test task_4` → green; run each new test once more with `-- --nocapture` to see the sync logs; gates clean.

#### Sub-task 4.5 — RPC `Sessions` handler (pure store read)

- [ ] **Step 1: Implement** in `src/ipc/server.rs` — dispatch arm after `Method::Drift` (mirroring that arm's `block_in_place` pattern [E43]), and `sessions_json` after `plans_json`:()```rust
        Method::Sessions => {
            let result = tokio::task::block_in_place(|| sessions_json(store))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            write_reply(stream, &json!({ "id": req.id, "result": result })).await?;
        }
```

```rust
/// Build the `Method::Sessions` result (Task 4): every session with its worktree path (when
/// mapped), most recently active first. Pure store read — the daemon's sessions ticker
/// keeps the rows fresh, mirroring how `plans_json` reads what the daemon ingested.
fn sessions_json(store: &Store) -> Result<Value, crate::store::StoreError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.agent, s.file, s.cwd, s.worktree_id, wt.path, \
             s.started_ms, s.last_event_ms, s.tokens_in, s.tokens_out, s.tokens_cache, \
             s.cost_usd, s.tool_calls, s.byte_offset, s.parse_errors \
             FROM sessions s LEFT JOIN worktrees wt ON wt.id = s.worktree_id \
             ORDER BY COALESCE(s.last_event_ms, s.started_ms, 0) DESC, s.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(json!({
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
        Ok(json!({ "sessions": sessions }))
    })
}
```

- [ ] **Step 2: Verify in isolation** (no test yet): replace this with the e2e in 4.7. Meanwhile: `cargo build && echo BUILD-OK`.

#### Sub-task 4.6 — Daemon ticker wiring (lifecycle: drop-guard)

- [ ] **Step 1: Implement** `src/daemon/sessions.rs` (mirrors the bus thread pattern [E46], with the advisor's Drop guard):()```rust
//! Sessions ingest ticker (Task 4): a daemon-owned thread that re-ingests pi session
//! files into the store. Mirrors the worktree event bus: sources feed the store in the
//! background; RPC handlers stay pure store reads. Freshness: first pass immediately at
//! daemon start, then every [`TICK_INTERVAL`] — `ra sessions` output may trail reality by
//! up to ~60 seconds, which is the documented behavior.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::store::Store;

/// Re-scan cadence for pi session files.
pub const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// Handle for the running ticker. Drop stops it: `run_foreground` simply holds this value,
/// and any return path stops the thread — no per-arm stop calls to maintain.
pub struct SessionsTicker {
    stop: Arc<AtomicBool>,
}

impl SessionsTicker {
    /// Start the ticker for one repo: one immediate `sources::sessions::sync_sessions`
    /// pass, then a pass every [`TICK_INTERVAL`]. `sessions_root` is resolved by the caller
    /// (daemon start) from the same env rules pi uses [E63].
    pub fn start(store: Arc<Store>, repo_root: PathBuf, sessions_root: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            loop {
                crate::sources::sessions::sync_sessions(&store, &repo_root, &sessions_root);
                let deadline = std::time::Instant::now() + TICK_INTERVAL;
                while std::time::Instant::now() < deadline {
                    if flag.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                if flag.load(Ordering::SeqCst) {
                    return;
                }
            }
        });
        SessionsTicker { stop }
    }
}

impl Drop for SessionsTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
```

Wire it in `src/daemon/mod.rs`: add `pub mod sessions;` beside `pub mod bus;` [E46 region, mod.rs:25-26], and in `run_foreground` immediately after the `start_bus` call [E46, mod.rs:86-88]. `store` there is `Arc<Store>` (bound as `let store = Arc::new(Store::open(&db)……)` before the bus starts [E72]), so `store.clone()` hands the ticker its own `Arc`:()```rust
    // Task 4: the sessions ingest ticker (pi files -> sessions/tool_calls rows). First
    // pass runs immediately; then every 60s. Drop-guarded: any return path below drops the
    // handle, which stops the thread.
    let sessions_root = crate::sources::sessions::resolve_sessions_root(&state.env);
    let _sessions_ticker = crate::daemon::sessions::SessionsTicker::start(
        store.clone(),
        repo_root.to_path_buf(),
        sessions_root,
    );
```

#### Sub-task 4.7 — CLI `run_sessions`, spec flip, and the end-to-end test

- [ ] **Step 1: Write the failing e2e test** (mirrors `tests/task_2_4.rs::ra_plans_and_plan_over_ipc` verbatim in structure: env-isolate, spawn `ra daemon --foreground`, wait for socket, drive `ra` via assert_cmd [E53, E54]; NO DB seeding — the daemon must ingest everything itself):()```rust
#[test]
fn ra_sessions_end_to_end_over_ipc() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-e", "e.txt");
    let (state, runtime) = (repo.path().join("state"), repo.path().join("run"));
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let (_fix_tmp, pi_root) = sessions_root_with_fixtures();
    let envs: Vec<(String, String)> = vec![
        ("HOME".into(), state.display().to_string()),
        ("XDG_STATE_HOME".into(), state.display().to_string()),
        ("XDG_RUNTIME_DIR".into(), runtime.display().to_string()),
        // the daemon ingests ONLY these fixtures — proving isolation end to end [E45, E63]
        ("PI_CODING_AGENT_SESSION_DIR".into(), pi_root.display().to_string()),
    ];
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();

    let mut daemon = std::process::Command::new(&ra_bin)
        .arg("daemon")
        .arg("--foreground")
        .envs(envs.iter().cloned())
        .current_dir(repo.path())
        .spawn()
        .expect("spawn ra daemon --foreground");

    // repo key exactly as the daemon computes it [E53's fallback pattern, task_2_4.rs:213-219]
    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_assist::paths::repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sock = runtime.join("repo-assist").join(format!("{key}.sock"));
    for _ in 0..50 {
        if sock.exists() { break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(sock.exists(), "daemon did not bind its socket at {sock:?}");

    // First tick runs at daemon start; poll until all three sessions are served.
    let mut served = Vec::<serde_json::Value>::new();
    for _ in 0..30 {
        let out = assert_cmd::Command::cargo_bin("ra").unwrap()
            .envs(envs.iter().cloned())
            .current_dir(repo.path())
            .arg("sessions")
            .arg("--json")
            .timeout(std::time::Duration::from_secs(20))
            .output()
            .expect("run ra sessions --json");
        if out.status.success() {
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            if v["sessions"].as_array().map(|a| a.len()) == Some(3) {
                served = v["sessions"].as_array().unwrap().clone();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert_eq!(served.len(), 3, "the daemon must ingest and serve the three fixture sessions");

    // Most recently active first: fixture 3 (07:00:51.076Z), then 2, then 1 [E32, E30].
    assert_eq!(served[0]["id"].as_str(), Some(F3_ID));
    assert_eq!(served[0]["tokens_in"].as_i64(), Some(8609));
    assert_eq!(served[0]["tool_calls"].as_i64(), Some(3));
    assert_eq!(served[1]["id"].as_str(), Some(F2_ID));
    assert_eq!(served[2]["agent"].as_str(), Some("pi"));
    assert_eq!(served[2]["worktree"].as_str(), None, "unmapped session shows null worktree [E37]");
    assert_eq!(served[2]["parse_errors"].as_i64(), Some(0));
    assert!(served[2]["byte_offset"].as_i64().unwrap_or(0) > 0);

    // `ra status --json` surfaces the session count without further work [E28].
    let out = assert_cmd::Command::cargo_bin("ra").unwrap()
        .envs(envs.iter().cloned())
        .current_dir(repo.path())
        .arg("status")
        .arg("--no-spawn")
        .arg("--json")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(out.status.success(), "status --no-spawn: {}", String::from_utf8_lossy(&out.stderr));
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["sessions"].as_i64(), Some(3));

    // Human output: one line per session, visible id/agent/tokens [E44 style].
    let out = assert_cmd::Command::cargo_bin("ra").unwrap()
        .envs(envs.iter().cloned())
        .current_dir(repo.path())
        .arg("sessions")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(F3_ID), "full ids are shown: {text}");
    assert!(text.contains("pi"));
    assert!(text.contains("8609"), "token counts are shown: {text}");

    let _ = daemon.kill();
    let _ = daemon.wait();
}
```

Note: the e2e holds `_fix_tmp` (the TempDir) alive as long as the fixture root is needed — assert the binding order in the test (declare `_fix_tmp` before `envs`, as above).

- [ ] **Step 2: Run to verify it fails** — `cargo test --test task_4 2>&1 | tail -6`. The e2e fails at "the daemon must ingest and serve..." (RPC Sessions still `not_implemented`, and `ra sessions` exits non-zero).

- [ ] **Step 3: Implement the CLI** in `src/cli.rs` — (a) in `fn spec` [E41]:()```rust
        Sessions(_) => Spec {
            implemented: true,
            task: 4,
        },
```
(b) in `fn run`'s match [E44]:()```rust
        Command::Sessions(args) => run_sessions(&args),
```
(c) `run_sessions` (placed after `run_drift`, mirroring `run_plans` [E44]):()```rust
/// `ra sessions`: known agent sessions (Task 4) — auto-spawns the daemon, then prints a
/// table or JSON. Rows are refreshed by the daemon's sessions ticker, so output may trail
/// the newest session file by up to ~60s (documented cadence, `daemon/sessions.rs`).
fn run_sessions(args: &SessionsArgs) -> Result<i32, anyhow::Error> {
    let mut client = resolve_and_connect(true)?;
    let reply = client.call(crate::ipc::Method::Sessions, serde_json::Value::Null)?;
    match reply {
        crate::ipc::Reply::Result { result, .. } => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_sessions_human(&result);
            }
        }
        crate::ipc::Reply::Error { error, .. } => {
            anyhow::bail!("daemon error [{}]: {}", error.code, error.message);
        }
    }
    Ok(0)
}
```
(d) `print_sessions_human` (placed after `print_plan_human`, terse one-line-per-session style like `run_drift`'s human branch [E44]):()```rust
/// Human-readable `ra sessions` output: one line per session, most recently active first.
fn print_sessions_human(s: &serde_json::Value) {
    let rows: &[_] = s
        .get("sessions")
        .and_then(|v| v.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    if rows.is_empty() {
        eprintln!("no sessions");
    }
    for row in rows {
        let id = row.get("id").and_then(serde_json::Value::as_str).unwrap_or("?");
        let agent = row.get("agent").and_then(serde_json::Value::as_str).unwrap_or("?");
        let cwd = row.get("cwd").and_then(serde_json::Value::as_str).unwrap_or("-");
        let worktree = row.get("worktree").and_then(serde_json::Value::as_str).unwrap_or("-");
        let cost = row
            .get("cost_usd")
            .and_then(serde_json::Value::as_f64)
            .map(|c| format!("{c:.4}"))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{id}  {agent}  in={} out={} cache={}  calls={}  cost={cost}  err={}  cwd={cwd}  worktree={worktree}",
            row.get("tokens_in").and_then(serde_json::Value::as_i64).unwrap_or(0),
            row.get("tokens_out").and_then(serde_json::Value::as_i64).unwrap_or(0),
            row.get("tokens_cache").and_then(serde_json::Value::as_i64).unwrap_or(0),
            row.get("tool_calls").and_then(serde_json::Value::as_i64).unwrap_or(0),
            row.get("parse_errors").and_then(serde_json::Value::as_i64).unwrap_or(0),
        );
    }
}
```

- [ ] **Step 4: Run the full task verification** — Run: `cargo test`
Expected: all suites green, now including `task_4.rs` (10 tests: parser×1, edge×3, store×1, discovery×1, sync×3, e2e×1 — ALL pass) + all previous suites.
Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && echo GATES-OK`
Run: `git status --porcelain | grep -v '^??' && echo DIRTY-UNEXPECTED || echo CLEAN` (all changes intentional).
Expected: gates clean; only expected files modified.

- [ ] **Step 5: Commit (single Task-4 commit, matching the repo's one-commit-per-task convention [E60])**()```bash
git add src/sources/sessions.rs src/sources/mod.rs src/daemon/sessions.rs src/daemon/mod.rs \
  src/store/mod.rs src/ipc/server.rs src/cli.rs Cargo.toml Cargo.lock tests/task_4.rs
git commit -m "Task 4: pi session source -> sessions store -> RPC Sessions -> ra sessions [--json]

- sources/sessions.rs: pi-root resolution (PI_CODING_AGENT_SESSION_DIR / PI_CODING_AGENT_DIR,
  verified against pi's shipped config.js), discovery (top-level *.jsonl only), parser
  (v3 headers; torn-tail rule; honest parse_errors; toolCall/toolResult linkage in-pass),
  sync with byte-offset skip cursor + full re-parse of changed files + component-wise
  cwd->worktree mapping (no match => NULL, per verified-facts Item 3).
- store: upsert_session_with_calls (one txn per file), session_file_offsets.
- daemon: SessionsTicker (immediate first pass + 60s cadence, drop-guarded).
- ipc: Method::Sessions -> sessions_json (pure store read, freshest-first).
- cli: run_sessions + print_sessions_human; spec table flips to implemented.
- Staleness note: output trails the newest file by up to ~60s. Session files are only read.
Verified: cargo test all green incl. task_4 (10 tests); fmt/clippy gates clean."
```

---

### Task 5 — Docs: correct the stale status.md claims

**Owner:** @worker · **Depends on:** Task 4 (documents what it built; also the promised corrections to stale claims); run last, after the Task-4 commit lands.

**Files:**
- Modify: `status.md` [E58]. No changes to `docs/ra.1.md` [E52 — no flag drifted].

- [ ] **Step 1: Correct status.md** — every edit is enumerated:()
  1. CLI table: `| ra sessions | no | 4 |` → `| ra sessions | yes | 4 |` [the spec table in src/cli.rs flips in 4.7 too — they must agree].
  2. Baseline honesty: replace the test-suite line "`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` — all clean." with two sentences: (a) the crate's three baseline defects (clippy `nonminimal_bool` in `tests/store.rs`; a Linux-only `/proc` liveness probe that made the second-daemon check fail off-Linux; a macOS tmpdir path-form assumption in `tests/task_2_3.rs`) were repaired in the three `fix(...)` commits listed below, each verified by its own gate; (b) the three gates are re-verified on this machine per task and the latest full-run summary is quoted verbatim (see Task 4, Step 4).
  3. Test-table: add a row for `task_4.rs` ("Task 4 acceptance: pi session source, ingest, worktree mapping, `ra sessions` end-to-end") and correct the counts line if the test totals moved.
  4. "Not yet" RPC list: add `Sessions` to the implemented list ("Implemented: `Hello`, `Ping`, `Status`, `Shutdown`, `Plans`, `Plan`, `Drift`, `Sessions`") — matching what `dispatch_request` actually arms after 4.5 [E42, E43].
  5. Data-model notes: the `sessions` / `tool_calls` line already exists [E22]; append one line describing the ingest model (root/60s cadence/full-re-parse-on-change/skip-cursor; measured-cwd stance: a session whose cwd falls inside no known worktree maps to no worktree).
  6. Status summary: add one "**Repaired before Task 4:** the three baseline fixes (clippy guards; lock-first pidfile ordering + portable test wait; canonicalized tmpdir key)" sentence with the three `fix(...)` commit SHAs (from `git log --oneline -4` at the time), and update the "Backlog" line (Task 4 leaves only Task 5 items) and "Latest commit" to the Task-4 SHA, plus a pointer to this plan file.
  7. Commits table: append rows for the fix commits and the Task-4 commit (leave the "Latest commit" line equal to the last commit at write time).

(Note: do not claim "all green" without a fresh run — paste the actual `cargo test` summary line from Task 4 Step 4's run.)

- [ ] **Step 2: Verify** — `grep -n sessions status.md` shows the flipped row and the new RPC/notes paragraphs render as intended (eyeball test); `cargo test` still green (docs only).

- [ ] **Step 3: Commit**()```bash
git add status.md
git commit -m "docs: status.md — Task 4 recorded, baseline-repair commits recorded, stale claims corrected"
```

---

## Out of scope (do not build these)

- Everything tagged task 5 in the spec table [status.md CLI table]: `ra worktrees`, `ra conflicts`, `ra who-touches`, `ra findings`, `ra ack`, `ra tui`, `ra doctor`, `ra render` — mock `Subscribe`/`RenderViews` RPC paths [E42 fallthrough] and any Subscribe/render wiring.
- **Retention / garbage collection of sessions rows** — explicitly out of Task 4 (a separate policy exists in the other repo's Step 1 plan; not imported here). A deleted session file merely leaves its row behind; a future retention task decides how to age rows out.
- Reactive fs-watching of `~/.pi` (the polled ticker is enough); an interval knob; `sources.pi.sessions_dir` config wiring (PiSource exists [E49] but Config loading is not wired into the DaemonState today — wiring it is out of scope; the env vars cover everything).
- True incremental byte-tailing (tail-cursor parsing) — deliberately rejected for this task (no call_id in `tool_calls` [E23]); revisit only if per-tick parse cost becomes observable (A7).
- Session-budget findings, "agent stuck" findings, or any **finding** emission at all — sources never write findings [E48]; loop detection and burned-token alerts belong to Task 5.
- Tools having `sessions` subcommands other than `ra sessions` (e.g. `ra session <id>` detail, `--agent` flag): not in the man page, not built [E52].
- Adding a `call_id` column / schema `user_version = 3` migration — no new column, no migration (schema stays at 2 [E24]).
- Wiring `Config::load` into the daemon [E49]; changing any RPC protocols; touching the drift/plans paths [E43].

## Open questions / assumptions carried

- **A2 (CI red is inferred):** the plan asserts CI-main red solely from local reproduction of CI's exact clippy command [E2, E5, E21]. If, at implementation time, GitHub Actions shows a green run on `main` (some caching artifact or a stable-clamp wave), the symptom disappears once Task 1 lands; no action either way. Uncertainty is harmless to the plan's structure.
- **A4 (mapping rule):** "worktree path is a canonical component-prefix of the session cwd" is the rule quoted from verified-facts [E37]; no fixture contains a real match case, so the matched path is exercised only by synthetic tests. If the incoming data ever nests deep (session in `sub/dir`), the longest-worktree-wins rule governs (tested).
- **A5 (dup ids):** duplicate `toolCallId`s in one pass produce duplicate rows, the last getting the result linkage (tested in 4.2); if real data shows ambiguity harder than that, it is the call_id-less consequence of E23 and lands in Task 5's loop detection anyway.
- **A6 (cost shape):** fixture cost totals are all 0.0 (ollama); `cost_usd` sums `usage.cost.total` and stores NULL when absent — real paid-session variants are unobserved beyond verified-facts' description [E36].
- **A7 (corpus scale):** first tick parses every file pi ever wrote (363M here) but once only; afterwards re-parse happens only for grown files (skip cursor) — if a session file ever grows into hundreds of MB and local testing shows a future observable cost, revisit the tail-cursor (rejected today for E23's reason).
- **Old-pi-version files:** other header `version`s are skipped whole (no row, no offset) [E36]; if pi ships version 4, parsing it is new work with new evidence, not a Task-4 defect.

## Self-review record (planner)

- **Spec coverage:** every pinned requirement from the caller's brief maps to a numbered step: ordering of fixes (Position §), per-flaw root cause + exact fix (Tasks 1–3), CLI Spec-table/RPC/CLI mirroring (Task 4 sub-tasks 4.1–4.7), TDD + test file conventions (Task 4 "Tests" in headers, `tests/task_4.rs`), worktree/retention scoping decisions (mapping § + Out of scope), docs/status.md corrections (Task 5). deprecated-docs status.md overclaims are called out explicitly [E58].
- **Type consistency:** `Ingested`/`CallInsert` (owned `String`s) → `SessionRow<'_>`/`ToolCallRow<'_>` (borrowed `&str`) conversion is defined once, in `sync_sessions`, and used by both store callsites; `session_byte_offsets` name never appears outside 4.3 after renaming (single canonical name: `session_file_offsets` — this renamed form is what the test at 4.3 Step 1 asserts; note the interfaces block at the top of Task 4 uses `session_file_offsets` too).
- **Placeholder scan:** none. Every expected number and session id comes from the ledger (E30–E33, E71).
- **Risk section:** the Position § (fix-first), the advisor-dispositions block, and the Out-of-scope § cover scope creep, environment drift (macOS vs CI Linux), corpus scale, and post-check behavior (staleness, no-push, no finding-mission creep).
