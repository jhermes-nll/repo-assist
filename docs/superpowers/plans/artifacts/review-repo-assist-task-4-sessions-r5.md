# Adversarial plan review — round 5 (delta only; resume of the round-4 cold pass)

Rules: below, every finding from round 4 is marked CLOSED (with disposition) or OPEN; in your reply do the same for new findings. Raise a new MUST ONLY against text changed in this delta or the new evidence shown here; do not reopen a closed finding. First line: `SIGNOFF: APPROVE` (zero open MUST) or `SIGNOFF: CHANGES`. Same finding format. Reply with text only.

## Dispositions of round-4 (cold pass) findings

**C-F1 [MUST] — CLOSED: FIXED (evidence corrected; id verified; constants adopted).** Real discrepancy found and resolved against ground truth: fixture 2's FILENAME and its line-1 header BOTH carry id `01a1249c-ec21-7703-8773-813f926a6357` (new evidence E71, extracted by glob so no human-typed digits were involved). The PLAN's 8 uses were already this exact id; the LEDGER's E31 was the mis-transcription (last segment written `613f`). Fixes applied:

- E31 corrected to the verified id; E71 added recording the extraction verbatim.
- The plan now defines the ids ONCE in `tests/task_4.rs` and reuses them everywhere:

```rust
/// The three fixture session ids, written once and reused by every test below (so a
/// transcribed id can never drift between tests) [cold-review F1].
const F1_ID: &str = "01a1249c-7c2c-75b4-9cf9-0a646cce0ffa";
const F2_ID: &str = "01a1249c-ec21-7703-8773-813f926a6357";
const F3_ID: &str = "01a1249d-14fb-744b-9318-378a5372ccfd";
```

- All later usages converted: `by_id(F1_ID)`, every `read_agg(&store, F2_ID, &…)` in the append/skip/shrink test, the marker INSERT and COUNT (now parameterized as `[F2_ID]`), and the e2e's `served[0] == F3_ID` / `served[1] == F2_ID` / `text.contains(F3_ID)` — AND the parser acceptance table itself uses `(F1_ID, 11436, …) / (F2_ID, 4999, …) / (F3_ID, 8609, …)`, so the consts are referenced from the moment they are defined (4.1) and never trip dead_code. (Correction to an earlier draft sentence of this delta which wrongly said the table kept literals — the table's actual text is:) `let expected = [ (F1_ID, 11436, 2253, 32640, 4, 1791615597613_i64, 1791615612968_i64), (F2_ID, 4999, 886, 9344, 2, 1791615626273, 1791615634451), (F3_ID, 8609, 2286, 17536, 3, 1791615636731, 1791615651076), ];`

**C-F2 [NICE] — CLOSED: FIXED.** The parser now accepts a v3 header exactly once — `let mut have_header = false;` is declared with the other accumulators, and the arm changed to:

```rust
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
```

This matches the documented rule ("a file may contain a `session` header only on line 1") and removes the re-parse-every-tick hazard.

**C-F3 [NICE] — CLOSED: FIXED.** The 4.1 block now contains only what 4.1 uses (`mod common;` — its file carries `#![allow(dead_code)]` (read tests/common/mod.rs:8), `redacted_fixtures`, the ID consts, and the parser test). The `sessions_root_with_fixtures` and `open_store` definitions were REMOVED from 4.1 and re-inserted verbatim at the top of the 4.4 Step 1 test block ("First add the two deferred helpers verbatim at the top of the file … then the tests:"), with a note in 4.1 explaining why an earlier definition would break the 4.1–4.3 gates.

**C-F4 [NICE] — CLOSED: FIXED (greps run; evidence recorded).** New evidence E68: (a) NO test asserts the `"not yet implemented"` stub (grep over `tests/`: zero hits) nor any session-count value — the only `sessions` mention in pre-existing tests is `tests/config.rs:82` asserting the CONFIG DEFAULT `c.sources.pi.sessions_dir == ""`, which Task 4 does not touch; (b) both daemon-spawning tests isolate HOME/XDG env (`tests/cli.rs::isolate`, `tests/task_2_4.rs::repo_isolate`), so under the plan's design their tickers resolve the sessions root into their own nonexistent temp state dir and never scan the real corpus [E39]. The plan's Task-4 Files paragraph now carries this blast-radius sentence with the [E68] tag.

**C-F5 [NICE] — CLOSED: FIXED.** Both halves: (a) the pragma is in fact ON for the writer connection in both `Store::open` branches — new evidence E69 (`src/store/mod.rs:123` and `:137`, verbatim `conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")`); (b) the upsert no longer relies on it anyway: the code now deletes the stale rows' `tool_calls` EXPLICITLY before deleting the stale `sessions` rows:

```rust
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
```

with the doc comment noting CASCADE would also cover it (E69) but is not relied upon.

**C-F6 [NICE] — CLOSED: FIXED.** Citations corrected: Task 2 Step 3 now cites [E61] for socket removal at `src/ipc/server.rs:80` (E61 already contained exactly that grep), and the `sock` binding is now separately evidenced by new E70 (bound at tests/cli.rs:65-71, verbatim `let sock = repo.path().join("run").join("repo-assist").join(format!("{key}.sock"));`). The mis-citation of E66 is gone.

**Trailing signature concerns (`run_foreground` vars + `store` type, `sync_worktrees`, `paths::canonicalize`) — CLOSED: evidenced + tagged.** New evidence E72 records all three verbatim: `paths::canonicalize(path: &Path) -> PathBuf` (falls back to the original when `fs::canonicalize` fails, src/paths.rs:21-24); `git::sync_worktrees(store: &Store, repo_root: &Path) -> Result<(), crate::store::StoreError>` (src/sources/git.rs:239-240); `run_foreground` holds `let store = Arc::new(Store::open(&db)…)` before starting the bus (src/daemon/mod.rs:79-86). The plan's 4.4 mapping helper doc and the 4.6 wiring instruction now carry [E72] tags (the wiring note spells out that `store` is `Arc<Store>` so `store.clone()` hands the ticker its own `Arc`).

## NEED responses (the three ranges requested)

```text
NEED: src/paths.rs:25-53 (as read; 21-53 shown for context)
21  /// Canonicalize a path for keying: make it absolute and resolve symlinks. Falls back to the
22  /// original path if it does not (yet) exist on disk.
23  pub fn canonicalize(path: &Path) -> PathBuf {
24      std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
25  }
26
27  /// The 12-char repo key: first 12 hex chars of `sha256(canonical common dir)`.
28  pub fn repo_key_from_git_common_dir(common_dir: &Path) -> String { ... }
...
37  /// Resolve the git common dir of `repo_root` ... shells out to
38  /// `git rev-parse --absolute-git-common-dir` ... Returns `None` when not inside a git repository.
39  pub fn git_common_dir(repo_root: &Path) -> Option<PathBuf> { ... }
...
53  (fn continues with the empty-output check and PathBuf::from)
```

```text
NEED: src/sources/git.rs:239-250 (as read)
239  /// Sync worktrees + changed-file sets into the store (Task 2.1 entry point).
240  pub fn sync_worktrees(store: &Store, repo_root: &Path) -> Result<(), crate::store::StoreError> {
241      let wts = discover_worktrees(repo_root);
242      let base_ref = detect_base_ref(repo_root);
243      let ms = now_ms();
244
245      // Phase A (single write txn): upsert rows ... for every DISCOVERED worktree —
246      //      `git worktree list --porcelain` reports the main repo root as a worktree too,
247      //      so it is upserted with present = 1 like any other.
...
250  (Phase A continues: per-worktree `upsert_worktree(...)` incl. `present = 1`)
```

```text
NEED: src/daemon/mod.rs:60-95 (as read; interior of run_foreground)
60   ...
62       let StateFiles { db, pid, .. } = state.state;   // state.env remains live/usable
66       let common_dir = git_common_dir(repo_root).ok_or_else(...)?;
70       let repo_key = state.repo_key.clone();   (empty-key refusal omitted)
74       let _pid_lock = lock::acquire_lock(&pid).map_err(|e| std::io::Error::new(
            std::io::ErrorKind::AlreadyExists, e.to_string()))?;
...
80  // (Re)create socket dir, clear any stale socket, then open the store:
84       let store = Arc::new(Store::open(&db).map_err(...)?);
86   // Task 2.3: start the worktree event bus (watcher + drain thread).
87       let bus = crate::daemon::bus::start_bus(store.clone(), repo_root);
...
95   (watch channel setup; run_accept_loop then takes `store` by value in the select)
```

## State

C-F1 CLOSED (FIXED) · C-F2 CLOSED (FIXED) · C-F3 CLOSED (FIXED) · C-F4 CLOSED (FIXED) · C-F5 CLOSED (FIXED) · C-F6 CLOSED (FIXED) · trailing signature concerns CLOSED (E72). No open findings. New/changed ledger entries this round: E31 (corrected), E68, E69, E70, E71, E72.
