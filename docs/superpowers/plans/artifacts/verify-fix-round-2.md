# Verification — review-findings fix round 2 (F6, F7, nits)

**Commit:** `fbc823e` — "fix(F6,F7): resolve swallowed checkpoint; nits (lock loop/wording/Solaris; store doc)"
**Base:** `8c9d941` · **Trigger:** deep-review SHOULD-FIX findings + 4 nits

## Orchestrator verification on `fbc823e` (run after the edits, not cited from an earlier run)

| Gate | Result |
|---|---|
| `cargo fmt --check` | exit 0, clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | exit 0 |
| `cargo test` | **44 passed / 0 failed** |
| `cargo test --test task_3` | 3/3 |
| `cargo test --test task_2_4` | 3/3 |
| Stray daemons | none |
| Working tree | clean (only pre-existing untracked `docs/`) |

## F6 — swallowed checkpoint resolved

**Diagnosis confirmed by the worker's own probe:** an in-transaction `PRAGMA wal_checkpoint(TRUNCATE)` issued *after* an `UPDATE` returns `"database table is locked"`, and the `.ok()` was swallowing it — so the checkpoint silently stopped happening when `5750247` made `Store::write` a real transaction.

**Fix chosen: delete the checkpoint**, after proving it is not load-bearing — the worker removed it entirely and `cargo test --test task_3` still passed 3/3, showing the follow-up read sees committed WAL rows without a truncate. The stale "cross-process WAL visibility … checkpoint explicitly" comment was replaced with one that **explains the trap** instead of asserting a false premise:

> Committed WAL rows are visible to a fresh read connection, so no checkpoint is needed here; an in-transaction checkpoint after the UPDATE would fail with "database table is locked" — that is what hid the earlier swallowed error.

**No error is swallowed anywhere in `tests/` now** — verified: `grep "wal_checkpoint"` returns only the `task_2_4` site (which propagates with `.expect`), and a grep for `execute_batch(...).ok()` / `.ok();` across `tests/*.rs` returns nothing.

## F7 — benign site annotated

`tests/task_2_4.rs:259` retains its checkpoint (proven still working: busy=0, WAL truncated to 0 bytes, because that closure has no writes). Added an explicit write-free constraint comment naming the exact failure mode if a write is added. Verified in place.

## Nits — all four applied

| Nit | Result |
|---|---|
| `upsert_session_with_calls` doc attribution | done — now reads "connection (supplied by `Store::write`)" (`src/store/mod.rs:352`) |
| Lock loop tail sleep | done — `if i < 4 { sleep(2ms) }`, no sleep after the final read |
| Lock `Flock` wording for the self case | done — distinguishes `"recovered holder pid equals this process (self)"` (`:91`) from `"no holder pid was recoverable"` (`:93`) |
| Solaris qualifier | done — the module doc now states the contract holds on Linux/macOS/BSD, that fs2 0.4.3 emulates `flock` via `fcntl` on Solaris (`fs2/src/unix.rs:56`) which lacks the same open-file-description scoping, and that the path is Unix-only (`LockFileEx` on Windows) |
| `WriteTxn` | deliberately **left alone** (dead but documents intent; `pub` defeats the lint) |

## Process correction (mine, not the worker's)

The **prior** verification pass (`verify-fix-round-f1-f4.md`) reported "no `PRAGMA`/`VACUUM` inside a closure" — but it grepped **`src/` only** and therefore missed these two **test** call sites. The deep reviewer caught the gap and flagged that the audit scope was wrong. That is an orchestrator framing error: the claim was stated as global when it was source-only. Recorded here so the record is honest.
