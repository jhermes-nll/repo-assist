# Fix round 2 — findings from the fix-round review

**Date:** 2026-10-19 · **Base:** `8c9d941` · **Trigger:** @reviewer-deep verdict APPROVED (0 must-fix) with 2 SHOULD-FIX + 4 nits, under the user's standing instruction "fix what reviewer found".

**Review record:** the verdict and evidence are in the deep reviewer's report for `5750247`/`8c9d941`. Sonnet sign-off: `SIGNOFF: APPROVE`.

## Context

Commits `5750247` (F1/F2) + `8c9d941` (F3/F4) centralized write atomicity in `Store::write`. That change is sound (the review confirmed 8 call sites audited, no nesting, WAL on readers, no starvation path, panic/poison semantics unchanged-or-better). But making `Store::write` a **real transaction** changed the behaviour of anything that ran inside a write closure assuming autocommit — and two *test* call sites do.

## F6 — SHOULD-FIX: the silently-failing checkpoint (`tests/task_3.rs:126-137`)

```rust
store.write(|conn| {
    conn.execute("UPDATE tasks SET expected_paths = ?1 WHERE id = 't-extra'", ["[\"extra.txt\"]"])?;
    // Cross-process WAL visibility ... checkpoint explicitly before the follow-up read.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").ok();   // <-- now silently fails
    Result::<(), _>::Ok(())
})
```

Pre-`5750247` this closure ran in autocommit, so the checkpoint actually truncated the WAL. Now the closure is a `BEGIN DEFERRED` transaction that has already written, and an in-transaction `wal_checkpoint` returns `"database table is locked"`. The `.ok()` **swallows it** — so the checkpoint is gone and nothing says so.

The test still passes, which means the checkpoint was **not** load-bearing for the assertion (readers do see committed WAL rows — reviewed probe: read-only handle reports `journal_mode="wal"` and sees committed rows with a non-empty 4152-byte WAL). So the comment's premise is now false.

**Required:** determine empirically whether the checkpoint is needed here at all, then either
 - **move it after the `.write(...)` call** (so it runs outside the transaction and genuinely truncates — preserves the original intent, minimal change), or
 - **delete it and correct the comment** (if the read genuinely doesn't need it).

Prefer whichever is *proven* by running the test; state which you chose and why. In **either** case **stop swallowing the error** — that `.ok()` is what hid this.

## F7 — SHOULD-FIX: same pattern, currently benign (`tests/task_2_4.rs:259`)

```rust
store.write(|conn| conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)"))
    .expect("checkpoint the seeded DB's WAL");
```

Reviewed probe: this one **still works** (busy=0, WAL truncated to 0 bytes) because the closure has **no writes of its own**, so the deferred transaction holds no write lock. It is one stray edit away from the `task_3` failure mode.

**Required:** add a comment stating the closure must stay **write-free**, and why (a write inside it would make the checkpoint fail). Do not otherwise restructure it.

## NITS (apply if cheap; say if skipped)

- `src/store/mod.rs:359-362` — `upsert_session_with_calls`'s doc still says "All statements run inside a single `unchecked_transaction`". Literally true, but no longer **self-attributing** since the body doesn't create it. Add "(supplied by `Store::write`)".
- `src/daemon/lock.rs:66-73` — two things: (a) the loop sleeps 2ms **after** the 5th failed read, a wasted tail sleep; (b) when a recovered pid equals our own, `LockError::Flock`'s wording "no holder pid was recoverable" is inaccurate — a pid *was* recovered and rejected as self. Fix the sleep placement and the wording.
- `src/daemon/lock.rs:7-9` — the `flock(2)` pin is accurate for Linux/macOS/BSD, but **fs2 0.4.3 itself emulates flock via `fcntl` on Solaris** (`fs2/src/unix.rs:56`) — precisely the documented hazard. Add a platform qualifier. Moot for a UDS daemon, but the doc should not overclaim.
- `src/store/mod.rs:61` — `WriteTxn<'a>` remains dead (definition only; `pub` defeats the `dead_code` lint). Pre-existing, out of scope unless trivially removable. **Do not remove it** — it documents intent; just leave it.

## Task table

| # | Task | Files | Verification |
|---|---|---|---|
| 1 | F6 — resolve the checkpoint; stop swallowing | `tests/task_3.rs` | `cargo test --test task_3` green; report which option + why |
| 2 | F7 — write-free comment | `tests/task_2_4.rs` | `cargo test --test task_2_4` green |
| 3 | Nits a/b — doc attribution + lock loop/wording | `src/store/mod.rs`, `src/daemon/lock.rs` | gates clean |
| 4 | Nit c — Solaris qualifier | `src/daemon/lock.rs` | `cargo doc` builds |

## Constraints

- **Local commits only.** No push, branches, or worktrees.
- **No new dependency.**
- **Do not modify anything under `docs/`** — that is the orchestrator's allow-path. (Especially not `docs/superpowers/plans/artifacts/verify-fix-round-f1-f4.md`, whose "no PRAGMA inside a closure" line is about to be superseded by this round.)
- Do **not** touch `status.md`, `docs/ra.1.md`, or any Task 5 CLI command.
- **Do not weaken or delete assertions** to make anything pass. If a test only passes because of the swallowed `.ok()`, surface that rather than hiding it.
- Gates: `cargo fmt --check` clean; `cargo clippy --all-targets --all-features -- -D warnings` exit 0; `cargo test` fully green.
- **Baseline to preserve: 44 passed / 0 failed** (cli 1, config 7, fswatch 2, git_repo 3, git_source 8, store 3, task_2_3 2, task_2_4 3, task_3 3, task_4 12).
- If you find the reviewer's F6 diagnosis does **not** hold when you probe it, say so with evidence rather than making a change you believe is unnecessary.

## Out of scope

- `WriteTxn` removal; Windows `LockFileEx`; any Task 5 CLI work.
