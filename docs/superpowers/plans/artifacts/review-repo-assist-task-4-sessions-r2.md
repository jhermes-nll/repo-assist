# Adversarial plan review — round 2 (delta only)

Same warm session; the plan and ledger are unchanged except the deltas below. Rules for this round:

- Below, every finding F1–F8 from round 1 is marked CLOSED (with disposition) or OPEN. In your reply, do the same for any new findings.
- Raise a new MUST ONLY against text changed in this delta or new evidence shown here. Do not reopen a closed finding.
- Reply with the same format: first line `SIGNOFF: APPROVE` (zero open MUST) or `SIGNOFF: CHANGES`, then at most 8 findings `F<n> [MUST|NICE] <Task/Step> | <claim attacked> | <why it breaks> | <fix>`.

## Dispositions of round-1 findings

**F1 [MUST] — CLOSED: FIXED.** You are right: the main repo root is itself a `present = 1` worktree [E59 git.rs:239-305 upserts every discovered worktree incl. root; NEED-range below], so the fool cwd (`<repo>/wt-m-suffix`) is component-wise inside the MAIN worktree and must map to it, not to NULL. The test now asserts exactly that, and it is a stronger boundary proof (main ≠ wt-m):

```rust
    let main_id = id_of
        .get(std::fs::canonicalize(repo.path()).unwrap().to_str().unwrap())
        .copied()
        .expect("the repo root is itself a present worktree (review F1)");
    assert_ne!(main_id, wt_m_id);
    ...
    // The fool cwd is component-wise inside the MAIN worktree (the repo root) but only a
    // string-prefix of wt-m: it must map to the main worktree, never to wt-m [review F1].
    assert_eq!(wt_of("s-fool"), Some(main_id), "string-prefix cwd falls to main-worktree containment, not wt-m");
```
(lookup helper rewritten flat to avoid naming rusqlite types in the test crate — see F6.)

**F2 [MUST] — CLOSED: FIXED.** The `call()` helper carries `usage.input = 1`, so totals are 11/6, not 10/5. Also the torn tail is now a REAL usage-bearing assistant line so the torn-tail check actually bites (wrongly parsing it would yield 21, not 11):

```rust
    // No trailing newline on the LAST line: a real assistant message (with usage input=10)
    // is the torn tail — if it were wrongly parsed, tokens_in below would be 21, not 11,
    // and parse_errors would grow by however it failed to decode. It must be ignored.
    std::fs::write(&f, format!("{body}{}", assistant_ok("2026-10-19T00:00:05.000Z"))).unwrap();
    let ing = repo_assist::sources::sessions::ingest_file(&f).expect("edge file parses");
    assert_eq!(ing.id, "s-edge");
    assert_eq!(ing.parse_errors, 3, "exactly rules (a),(b),(c) fire — the torn tail is NOT an error");
    assert_eq!(ing.tokens_in, 11, "assistant_ok (10) + the call's usage (1); the torn tail contributes nothing");
    assert_eq!(ing.tokens_out, 6);
```

**F3 [MUST] — CLOSED: FIXED.** All four sub-defects:
- `read_session_id` now opens with `use std::io::{BufRead, BufReader};`.
- `match_worktree` tail is now `.map(|(id, _)| *id)` (no map_flatten).
- the `expand` closure returns the `PathBuf::from(...)` expression directly (no let_and_return).
- 4.1's module header imports only what 4.1 uses:

```rust
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
// (Sub-task 4.4 adds `use std::path::PathBuf; use crate::paths::{self, Env};
//  use crate::store::Store;` — import each name when first used so the `-D warnings`
//  gate stays green at every sub-task.)
```
and the Task 4 preamble now says: "Run `cargo fmt` after every code step below — the gates check it."

**F4 [MUST] — CLOSED: FIXED.** Correct: `-D warnings` compiles all targets, so the gates sequence the tasks even though the files are disjoint. Changed:
- Global-constraints bullet renamed "**Blast radius / ordering:**" and now reads: "They are file-parallel in principle, but the `-D warnings` gate compiles ALL targets — including `tests/store.rs` — so **execute strictly in order 1 → 2 → 3** (single worker); Task 3's full-suite gate is the checkpoint proving the baseline green."
- Task 2 header: "· **Depends on:** Task 1 (gates)".
- Task 3 header: "· **Depends on:** Tasks 1–2".
- Task 1–3 are committed in order by the single @worker.

**F5 [MUST] — CLOSED: REBUTTED (E50, strengthened).** The constructor exists — it was in evidence all along, but my E50 excerpt was too thin, which is fair to flag. Strengthened entry, now with the verbatim signature (this is also your NEED `src/paths.rs:54-131`, answered in full below):

```rust
/// Construct directly (for tests).
pub fn with(
    home: impl Into<String>,
    state_home: Option<String>,
    runtime_dir: Option<String>,
) -> Self {
    Env {
        home: home.into(),
        state_home: state_home.unwrap_or_default(),
        runtime_dir: runtime_dir.unwrap_or_default(),
    }
}
```
with field `pub home: String`. So `Env::with("/home/u", None, None)` and `env.home.clone()` / `trim_end_matches('/')` on it are all well-typed.

**F6 [MUST] — CLOSED: REBUTTED (E25 strengthened + new E67).** `Store::read`'s bound is `F: FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error>` (verbatim in strengthened E25 and in the NEED excerpt below). A closure whose body is exactly `conn.query_row(...)` returns `Result<T, rusqlite::Error>` — the bound is satisfied directly; the `Ok(rows.collect::<...>()?)` wrapper is only needed for multi-statement bodies, which is why both shapes appear in the plan. New evidence E67: the COMMITTED Task 2.4 test already uses this exact "bare query_row as read body" shape and compiles green in the current suite:

```rust
    let first_seen: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT first_seen_ms FROM plans WHERE id = 'plan-a'",
                [],
                |r| r.get(0),
            )
        })
        .expect("read first_seen_ms");
```
Additionally, the mapping test no longer needs inner closure annotations at all: the per-session lookup is now a flat helper whose closure returns `conn.query_row(...)` directly, avoiding any rusqlite naming in the test crate (rusqlite is not a dev-dependency [read Cargo.toml]).

**F7 [NICE] — CLOSED: REBUTTED (E28/E41 strengthened).** Both facts were already gathered; excerpts now in the ledger: `StatusArgs` declares `#[arg(long)] pub json: bool,` and `#[arg(long)] pub no_spawn: bool,` (src/cli.rs:37-49), and `status_json` finishes with `Ok(json!({ ... "sessions": sessions, }))` (src/ipc/server.rs:296-305) — the exact key the e2e asserts. The `--no-spawn` path is also already exercised by the committed `tests/cli.rs` (step 4 of the round-trip test).

**F8 [NICE] — CLOSED (two parts) FIXED, (one part) REBUTTED.**
- (b) Task 5 garbled text FIXED: items 2 and 6 rewritten into plain, complete sentences (now pasting the verbatim gate summary; enumerating the three fixes with one-line root causes; no stray tokens).
- (c) skip-proof FIXED: the unchanged-file re-sync now must leave the materialized `tool_calls` row ids untouched — a re-parse re-mints rowids, so stability proves the skip:

```rust
    // Unchanged -> skip. Proof the skip cursor fired: the materialized tool_calls row ids
    // are stable — a re-parse would DELETE+reinsert and mint NEW rowids [review F8].
    let ids_after_first = call_ids(&store, "01a1249c-ec21-7703-8773-813f926a6357");
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    assert_eq!(read_agg(&store, "01a1249c-ec21-7703-8773-813f926a6357", &size), (4999, 2));
    assert_eq!(
        call_ids(&store, "01a1249c-ec21-7703-8773-813f926a6357"),
        ids_after_first,
        "the byte-offset skip cursor prevented a re-parse"
    );
```
with helper:
```rust
/// The materialized `tool_calls` row ids for one session — a re-parse re-mints them.
fn call_ids(store: &repo_assist::store::Store, id: &str) -> Vec<i64> {
    store
        .read(|conn| {
            let mut stmt = conn
                .prepare("SELECT id FROM tool_calls WHERE session_id = ?1 ORDER BY id")?;
            let ids = stmt
                .query_map([id.to_string()], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
        .unwrap()
}
```
- (a) REBUTTED with reworded comment (kept design): `resolve_sessions_root` must read the same environment variables pi does, because the daemon is a separate process that must find the same sessions pi writes [E63, E64]; parameterizing the lookups would diverge from that contract. The race is safe because (1) the only in-process reader of these two variables is that one test, and (2) every daemon child spawned by the e2e sets `PI_CODING_AGENT_SESSION_DIR` explicitly, which has TOP precedence in the child's resolution — the momentary in-process value of `PI_CODING_AGENT_DIR` cannot change what any spawned daemon ingests. Comment in the plan now says exactly this.

## NEED responses (the five ranges requested)

```text
NEED: src/paths.rs:54-131  (as read)
    /// Whether `path` is git metadata we skip — NO, lines 54-131 are:
    54  /// Environment inputs used to resolve paths (overridable for tests).
    55  #[derive(Debug, Clone)]
    56  pub struct Env {
    57      /// `$HOME` (or `~`).
    58      pub home: String,
    59      /// `$XDG_STATE_HOME`; empty falls back to `$HOME/.local/state`.
    60      pub state_home: String,
    61      /// `$XDG_RUNTIME_DIR`; empty falls back to the state directory for the socket.
    62      pub runtime_dir: String,
    63  }
    64
    65  impl Env {
    66      /// Read from `HOME` + `XDG_STATE_HOME` + `XDG_RUNTIME_DIR` (empty where unset).
    67      pub fn from_real() -> Self {
    68          Env {
    69              home: std::env::var("HOME").unwrap_or_default(),
    70              state_home: std::env::var("XDG_STATE_HOME").unwrap_or_default(),
    71              runtime_dir: std::env::var("XDG_RUNTIME_DIR").unwrap_or_default(),
    72          }
    73      }
    74
    75      /// Construct directly (for tests).
    76      pub fn with(
    77          home: impl Into<String>,
    78          state_home: Option<String>,
    79          runtime_dir: Option<String>,
    80      ) -> Self {
    81          Env {
    82              home: home.into(),
    83              state_home: state_home.unwrap_or_default(),
    84              runtime_dir: runtime_dir.unwrap_or_default(),
    85          }
    86      }
    87
    88      /// Base state directory: `$XDG_STATE_HOME/repo-assist/<repo-key>`, or
    89      /// `~/.local/state/repo-assist/<repo-key>` when `XDG_STATE_HOME` is unset.
    90      pub fn state_dir(&self, repo_key: &str) -> PathBuf {
    91          let base = if self.state_home.is_empty() {
    92              Path::new(&self.home).join(".local/state")
    93          } else {
    94              Path::new(&self.state_home).to_path_buf()
    95          };
    96          base.join("repo-assist").join(repo_key)
    97      }
    ... (state_files(), socket_path() follow; no other constructors)
```

```text
NEED: src/store/mod.rs:146-170  (as read)
    146     /// Run a read query callback using a separate read-only connection handle.
    147     pub fn read<F, T>(&self, f: F) -> Result<T, StoreError>
    148     where
    149         F: FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error>,
    150     {
    151         let conn = rusqlite::Connection::open_with_flags(
    152             &self.path,
    153             rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    154         )
    155         .map_err(StoreError::Database)?;
    156         f(&conn).map_err(StoreError::Database)
    157     }
    158
    159     /// Run a write callback against the guarded writer connection.
    160     pub fn write<F, T>(&self, f: F) -> Result<T, StoreError>
    161     where
    162         F: FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error>,
    163     {
    164         let lock = self.writer.lock().map_err(|_| StoreError::Poisoned)?;
    165         f(&lock).map_err(StoreError::Database)
    166     }
```

```text
NEED: src/daemon/mod.rs:66-95  (as read; run_foreground interior)
     66   pub async fn run_foreground(repo_root: &Path, state: DaemonState) -> Result<(), std::io::Error> {
     67       let repo_key = state.repo_key.clone();
     68       let StateFiles { db, pid, .. } = state.state;
     69       let common_dir = git_common_dir(repo_root)
     70           .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not a git repository"))?;
     71       ... (empty-key check)
     ... // Acquire the pidfile lock (Section 6): returns early on Busy.
     ...     let _pid_lock = lock::acquire_lock(&pid)
     ...         .map_err(|e| std::io::Error::new(std::io::ErrorKind::AlreadyExists, e.to_string()))?;
     ... // (Re)create the socket dir, clear any stale socket, open the store.
     ...     let store = Arc::new(Store::open(&db).map_err(...)?);
     86       // Task 2.3: start the worktree event bus (watcher + drain thread).
     87       let bus = crate::daemon::bus::start_bus(store.clone(), repo_root);
     ... // (signal setup; run_accept_loop takes `store` by value at the select below)
```
(state.env remains live in scope after the destructure — `state.state` is moved, `state.env` is not.)

```text
NEED: src/sources/git.rs:239-250  (as read)
    239 /// Sync worktrees + changed-file sets into the store (Task 2.1 entry point).
    240 pub fn sync_worktrees(store: &Store, repo_root: &Path) -> Result<(), crate::store::StoreError> {
    241     let wts = discover_worktrees(repo_root);
    242     let base_ref = detect_base_ref(repo_root);
    243     let ms = now_ms();
    244     // Phase A: upsert one worktrees row per DISCOVERED worktree — including the main
    245     //     repo root itself (`git worktree list` reports it), with present = 1.
```
(Phases A–C upsert every discovered worktree, replacing changed-file sets and marking vanished ones not-present — nothing filters the main root out.)

```text
NEED: src/cli.rs:37-49 (StatusArgs region — the range your F7 actually depends on)
    37  /// One-screen summary: worktrees, active plans, open findings, sessions.
    38  #[derive(Args, Debug, Default)]
    39  pub struct StatusArgs {
    40      /// Emit JSON instead of human-readable text.
    41      #[arg(long)]
    42      pub json: bool,
    43
    44      /// Do not auto-start the daemon if none is running (Section 7).
    45      #[arg(long)]
    46      pub no_spawn: bool,
    47  }
```
plus, at src/ipc/server.rs:296-305, the status result object ends: `"worktree_count": worktrees.len(), "active_plans": active_plans, "open_findings": open_findings, "sessions": sessions`.

## Round-1 findings against evidence (explicit close-out)

F1 CLOSED (FIXED) · F2 CLOSED (FIXED) · F3 CLOSED (FIXED) · F4 CLOSED (FIXED) · F5 CLOSED (REBUTTED: E50) · F6 CLOSED (REBUTTED: E25+E67) · F7 CLOSED (REBUTTED: E28, E41) · F8 CLOSED (FIXED b,c; REBUTTED a). No finding remains open.
