# Fix round — outstanding reviewer findings (repo-assist)

**Date:** 2026-10-19 · **Base:** `5ee66cc` · **Trigger:** user instruction "fix what reviewer found"
**Class:** bounded — enumerated findings with exact `file:line`, no new design.

## Source of truth

The findings come from two on-disk review artifacts, both already adversarial-reviewed:

- `docs/superpowers/plans/artifacts/review-baseline-tasks-1-3.md` — findings against `002b59c`/`950d996`/`292c026`
- `docs/superpowers/plans/artifacts/review-task-4-sessions.md` — findings against `75f7434`
- `docs/superpowers/plans/artifacts/verify-red-green-task-4.md` — independently confirmed the `git.rs` follow-up

## Findings to fix

### F1 — `replace_files` autocommit bug (the serious one)

**File:** `src/sources/git.rs:161-177` (+ call site `:222-234`)

`replace_files` does `DELETE FROM worktree_files WHERE worktree_id = ?1` then loops `INSERT OR IGNORE` on the same raw autocommit connection. Called from the Phase B `store.write` closure, which first runs `UPDATE worktrees SET head = ?2, merge_base = ?3, updated_ms = ?4`. **No transaction anywhere.** A failure mid-loop leaves the worktree's file set partially deleted / partially inserted, while the `UPDATE worktrees` in the same closure has already committed.

Confirmed independently **twice** (deep reviewer + the red→green verifier).

The comment at `git.rs:185` — `// Phase A (single write txn): upsert rows...` — **is false today**. Phase A's closure also has multiple statements (`upsert_worktree` in a loop).

### F2 — Root-cause fix: make `Store::write` actually transactional

**File:** `src/store/mod.rs:220-227` (`Store::write`)

Rather than patch each call site, make the closure atomic **by construction**. Rationale:

- The reviewer found this same bug in **two** independent places (`upsert_session_with_calls`, `replace_files`). A third will appear. Centralizing eliminates the class.
- `WriteTxn<'a>` at `store/mod.rs:61` is **dead code** — `pub type WriteTxn<'a> = rusqlite::Transaction<'a>;` has zero uses. The original design clearly intended transactions.
- The 8 existing call sites were audited: **no nesting** (no closure calls `store.write`/`self.write`), and **no closure runs `VACUUM` or `PRAGMA journal_mode`**, so wrapping them in `BEGIN … COMMIT` is safe.

**CRITICAL CONSTRAINT (verified from rusqlite's docs, not memory):** `unchecked_transaction` **cannot nest** — it fails at runtime with `"cannot start a transaction within a transaction"`. Therefore centralizing in `Store::write` **requires removing** the inner `unchecked_transaction` from `upsert_session_with_calls` (`store/mod.rs:351`/`:377`). Leaving both in place turns a silent-corruption bug into a hard runtime failure — the tests will catch it, but the change must be done as one unit.

`unchecked_transaction()` takes `&self`, so it fits `Store::write`'s existing `FnOnce(&rusqlite::Connection)` signature via deref coercion (`Transaction: Deref<Target = Connection>`) — **no signature change needed**.

Leave `BEGIN DEFERRED` (the `unchecked_transaction` default). One writer behind a mutex in WAL mode; readers use separate handles. `IMMEDIATE` would add nothing.

### F3 — `lock.rs` messaging race

**File:** `src/daemon/lock.rs:51-58`

Between the winner's `try_lock_exclusive()` success (`:51`) and its `fs::write(pid_path, …)` completing (`:53`), a contender whose flock attempt fails reads either an **empty** pidfile (first-ever run → `held` parses to 0) or the **previous holder's dead pid**. Both fall through to the raw `LockError::Flock("Resource temporarily unavailable (os error 35)")` instead of the friendly `Busy` message.

Message-quality only — every path still refuses to start and the one-daemon invariant holds — but the message is misleading exactly when it matters.

**Fix:** on the failure path, do a short bounded re-read of the pidfile (the winner's write completes in microseconds), then fall back. Also reword `LockError::Flock`'s message so the fallback is not misleading.

Note: writing the pid via the already-open locked fd does **not** remove the window — the gap between acquiring the lock and writing content is inherent. Only a bounded re-read (or accepting the wording) closes it.

### F4 — `lock.rs` module doc nit

**File:** `src/daemon/lock.rs:3-5`

Pin the `flock(2)` dependency: correctness of writing the pid via `fs::write` on a **second, short-lived fd** depends on fs2 using `flock(2)` (open-file-description-scoped — unaffected by truncation or another fd's close). A future migration to an `fcntl`/`F_SETLK` locker would **silently drop the lock** when that fd closes. One sentence. Also mark the lock path Unix-only (fs2's Windows path is `LockFileEx` range locking, where the contender's read of the locked region can fail outright).

### F5 — amend evidence ledger E62 (orchestrator-owned)

**File:** `docs/superpowers/plans/artifacts/evidence-repo-assist-task-4-sessions.md:85`

E62 asserts a failed lock attempt "returns `LockError::Busy` … on ALL platforms". Not literally true during the write window described in F3. Amend to record the exception. **I will do this one myself** — it is inside my write allow-path (`docs/superpowers/**`).

## Task table

| # | Task | Files | Verification |
|---|---|---|---|
| 1 | TDD: prove `replace_files`/Phase-B non-atomicity (red) | `tests/task_2_1_git.rs` or existing git test file | new test FAILS against base |
| 2 | Centralize transaction in `Store::write`; remove inner tx from `upsert_session_with_calls` | `src/store/mod.rs` (`:220-227`, `:340-378`) | new test green; **existing `upsert_session_with_calls_is_atomic_on_midstream_failure` still green** |
| 3 | Bounded re-read + reworded fallback in `acquire_lock` failure path | `src/daemon/lock.rs:51-58` (+ `:22-25` message) | new/extended test; `cargo test --test cli` still 1/1 |
| 4 | Module doc: pin `flock(2)` dependency + Unix-only | `src/daemon/lock.rs:3-5` | `cargo doc` builds; gates clean |
| 5 | Amend E62 | `artifacts/evidence-…md:85` | eyeball (orchestrator) |

**Order:** 1 → 2 (2 depends on 1's red test), then 3 → 4. Task 5 independent, done by the orchestrator.

## Constraints

- **Local commits only.** No push, no branches, no worktrees.
- Do **not** touch `status.md` (Task 5 of the prior plan already corrected it; this round is code).
- **No new dependency.**
- Gates: `cargo fmt --check` clean, `cargo clippy --all-targets --all-features -- -D warnings` exit 0, `cargo test` fully green.
- **Preserve the existing atomicity test** `upsert_session_with_calls_is_atomic_on_midstream_failure` — it must still pass after centralization. If it does not, the centralization is wrong.
- Baseline to preserve: **43 passed / 0 failed** (cli 1, config 7, fswatch 2, git_repo 3, git_source 7, store 3, task_2_3 2, task_2_4 3, task_3 3, task_4 12).

## Out of scope

- The Windows `LockFileEx` behaviour itself (doc note only).
- Any Task 5 CLI commands (`ra worktrees`, `ra conflicts`, etc.).
- `ra.1.md` — no flag drift.
