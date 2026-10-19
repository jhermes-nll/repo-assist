# Verification — review-findings fix round (F1–F4)

**Verifier:** dedicated @worker dispatch (independent of the fix author) · **Date:** 2026-10-19
**Commits under test:** `5750247` (F1/F2 store atomicity), `8c9d941` (F3/F4 lock) · base `5ee66cc`

## Part 1 — red→green re-derived (not taken on trust)

Method: `git checkout 5ee66cc -- src/store/mod.rs src/sources/git.rs` (revert both source files, keep the new test at HEAD), run, restore.

**RED (fix reverted, test present):** `FAIL`
```
test sync_worktrees_replace_files_is_atomic ... FAILED
assertion `left == right` failed: file set must not be deleted on failure
  left: 0
 right: 4
```
Matches the developer's claimed `left: 0, right: 4` exactly. Against unfixed code the worktree file set **was** deleted and not restored — the bug is real and the test detects it.

**GREEN (fix restored):** `PASS` — `1 passed; 0 failed`.

**Restoration confirmed:** `git status --short` identical to before (only the two pre-existing untracked `docs/` entries); HEAD unchanged at `8c9d941`.

## Part 2 — centralization soundness (the risky change)

Centralizing a transaction into a globally-used `write()` helper could break things subtly. All checks came back clean:

| Check | Finding |
|---|---|
| Nesting removal complete? | **yes** — `upsert_session_with_calls` (`src/store/mod.rs:356`) body has no inner `unchecked_transaction`/`begin`/`commit`. Only executable `unchecked_transaction` is the centralized one at `:228`. |
| Recursion at other call sites? | **none** — 3 sequential `store.write` calls in `sync_worktrees` (`git.rs:186, 222, 239`), none recursive; store methods at `mod.rs:250, 288, 315, 335, 361` each wrap a top-level `self.write` with no re-entrance. |
| `PRAGMA`/`VACUUM` inside a txn? | **none** — open/migrate run `PRAGMA journal_mode = WAL` and `PRAGMA user_version` on a **raw `Connection` outside** `Store::write` (`mod.rs:154, 168`; `migrations.rs:64`). No `VACUUM` anywhere in `src/`. |
| Slow work inside a txn? | **correct** — `discover_worktrees`, `detect_base_ref`, and per-worktree `git rev-parse`/`merge-base`/`changed_file_set` all run **before** each `store.write`. No subprocess call inside a closure (so no writer mutex held across a subprocess). |
| Reader/writer lock hazard? | **none** — `Store::read` opens a fresh read-only `Connection` per call (`mod.rs:206`); the writer is a separate `Arc<Mutex<…>>`. Readers never touch the writer mutex, so an open write transaction can't block reads (WAL). |
| Existing regression guard? | **passes** — `upsert_session_with_calls_is_atomic_on_midstream_failure` (pre-existing, from `a28b553`) still green, proving the centralization did not weaken the earlier fix. |

**VERDICT: centralization sound.**

## Part 3 — lock change

| Check | Finding |
|---|---|
| Retry bounded? | **yes** — `for _ in 0..5` (`src/daemon/lock.rs:73`), fixed 5 attempts. Not a `loop`/`while true`. |
| Sleep on happy path? | **none** — `try_lock_exclusive()` success returns immediately; sleep only in the failure branch while `held == 0`. |
| Worst-case delay | **10 ms** (5 × 2 ms). |
| Pidfile ordering | **correct** — lock acquired first, `fs::write` of the pid only after. |
| `cargo test --test cli` | pass (1/1). |

## Part 4 — cleanup

- Full suite: **44 passed / 0 failed**.
- Stray daemons: **none**.
- Repo left exactly as found: **yes**.

## Orchestrator-side verification (independent of the above)

- `cargo fmt --check` clean; `cargo clippy --all-targets --all-features -- -D warnings` exit 0; `cargo test` **44 passed / 0 failed**; no stray daemons.
- Read `Store::write` myself: `unchecked_transaction` at `:228`, commit on `Ok`, drop-rollback on `Err`.
- Read `upsert_session_with_calls`: plain `conn.execute` throughout, no `tx.` prefix — inner transaction genuinely removed.
- Read the `acquire_lock` failure branch: bounded retry, no happy-path sleep.

## Honest gap (stated by the developer, confirmed by me)

The **F3 lock-race test is not deterministic** — the window is microseconds, so no test proves the race is closed. The developer said so rather than faking a test. Mitigation: the retry logic is 10 lines and was read directly; the bound and the no-happy-path-sleep property are confirmed by inspection. Residual risk is message-quality only (the one-daemon invariant was never at stake).
