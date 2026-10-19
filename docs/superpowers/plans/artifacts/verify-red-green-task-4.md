# Red→green verification — Task 4 atomicity fix (`a28b553`)

**Verifier:** dedicated @worker dispatch (independent of the fix author) · **Date:** 2026-10-19
**Purpose:** confirm the new atomicity test genuinely detects the bug. A test that passes both before and after a fix proves nothing, so the developer's red→green claim was re-derived from scratch.

## Method

Reverted **only** `src/store/mod.rs` to `75f7434` (the pre-fix version), keeping the new test at HEAD, then ran the single test. Restored afterwards.

## Result — claim CONFIRMED

**RED (fix reverted, test present):** `FAIL`
```
test upsert_session_with_calls_is_atomic_on_midstream_failure ... FAILED
assertion `left == right` failed: byte_offset must not advance when the write fails mid-way
  left: 99
 right: 7
test result: FAILED. 0 passed; 1 failed; 0 ignored; 11 filtered out
```
Matches the developer's claimed `left 99 / right 7` exactly.

**GREEN (fix restored):** `PASS` — `1 passed; 0 failed`.

## Test meaningfulness checks

| Question | Finding |
|---|---|
| Proves row rollback, or merely that `Err` returned? | **Both.** Asserts `res.is_err()` **and** that persisted `byte_offset` stayed `7` vs `99` — so it detects the row actually rolling back. |
| Does the trigger fail *inside* the transaction? | **Yes.** Trigger is `BEFORE INSERT ON tool_calls`; the failing statement runs inside `unchecked_transaction()` before `tx.commit()`. The earlier sessions-upsert/deletes are in the same tx, so rollback is genuinely exercised. |
| Would it pass if the fix were a no-op? | **No.** Without the transaction the sessions upsert autocommits, `byte_offset` becomes 99, and the `off == 7` assertion fails. |

**VERDICT: the test genuinely detects the bug.**

## Restoration

- `git status --short` after: identical to before (only the two untracked `docs/` entries).
- `git rev-parse HEAD`: unchanged, `a28b553`.
- `unchecked_transaction` present again (2 occurrences).
- No `ra daemon` processes at any point; nothing left running.

## Other fixes verified as real (by reading code, not trusting the commit message)

| Fix | Finding |
|---|---|
| `Some(0.02)` cost assertion | confirmed, `tests/task_4.rs:181` ("cost accumulates, not replaces") |
| NULL-cost test | confirmed, `tests/task_4.rs:768` — checks ingest `None` **and** DB `cost_usd IS NULL` |
| read-failure warn | confirmed, `src/sources/sessions.rs:284-294` ("cannot read … skipped") |
| stale cli.rs doc line | gone (grep returns none) |
| `DaemonGuard` RAII | confirmed in **both** `tests/task_4.rs:759` and `tests/task_2_4.rs:455` |
| mapped-worktree over IPC | confirmed — asserts `mapped["worktree"] == Some(wt_map …)` |

## Follow-up confirmed (NOT fixed, out of scope)

`src/sources/git.rs:161-177` `replace_files` — **same class of bug, independently confirmed.** It performs `DELETE FROM worktree_files` then a loop of `INSERT OR IGNORE` on the same raw autocommit connection with no transaction, *and* the preceding `UPDATE worktrees` in the same `store.write` closure has already committed. A failure mid-loop leaves the file set partially deleted / partially inserted. (`upsert_worktree` itself is single-statement, so it is fine.)

This is a pre-existing defect, not introduced by Task 4. It should be fixed in a future task — the same `unchecked_transaction` pattern applies.
