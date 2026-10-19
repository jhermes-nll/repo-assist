# Review — Task 4 `ra sessions` (commit 75f7434)

**Reviewer:** @reviewer-deep (GLM-5.3) + Sonnet headless sign-off · **Date:** 2026-10-19
**Scope:** `git diff 292c026..75f7434` (9 files, +1299/−2)
**Verdict: CHANGES** — one MUST-FIX. Sonnet round 1 `SIGNOFF: CHANGES` (agreed), round 2 `SIGNOFF: APPROVE` after one elevation.

## Orchestrator independently confirmed the MUST-FIX is real

Read the code myself:
- `src/store/mod.rs:220-227` — `Store::write` takes a **mutex** (`self.writer.lock()`), then calls `f(&lock)`. There is **no `BEGIN`**.
- `src/store/mod.rs:341-376` — `upsert_session_with_calls` issues **6 sequential `conn.execute` calls** with no transaction. Under rusqlite's default autocommit, each commits on its own.
- `src/store/mod.rs:339` — the doc comment claims *"per-file atomicity (Task 4): readers never see half a session."* **The code does not deliver this.**
- `grep -n "BEGIN\|transaction\|commit()" src/store/mod.rs` → only two hits, both in comments/doc (`:60`, `:339`). No real transaction anywhere.

## MUST-FIX

**`src/store/mod.rs:341-376` (`upsert_session_with_calls`) is not atomic.**

Statement 1 is the sessions-row upsert, which **advances `byte_offset`”. Statements 2..N (stale-row deletes, own `tool_calls` delete, per-call inserts) can fail after it — via `SQLITE_FULL`, an IO error, or the daemon being killed mid-sync. Result: the session row is marked "up to date" while its `tool_calls` are stale or missing. `session_file_offsets` then returns the advanced offset, so the **skip cursor suppresses the re-parse that would heal it** — the torn state persists permanently for every file that never grows again. Concurrently `sessions_json` can read `tool_calls=N` while the table is mid-replacement: exactly the half-session state the doc comment promises readers never see.

Reviewer proved it with a probe against rusqlite 0.32 bundled:
`"closure returned: Err(... no such table ...) … sessions => 1 … => PARTIAL WRITE PERSISTED … s3 rows persisted: 0 (tx rolled back correctly)"`
— i.e. plain `execute` leaves partial writes; `unchecked_transaction` rolls back correctly.

**Fix:** inside the write closure use `conn.unchecked_transaction()?` … `tx.commit()?` (takes `&Connection` — matches what the closure provides), or teach `Store::write` to open `BEGIN IMMEDIATE … COMMIT`. Small change.
**Same latent flaw in pre-existing `src/sources/git.rs:161-177` (`replace_files`)** — outside this diff, follow-up.

## SHOULD-FIX

1. **`tests/task_4.rs:150-194`** — the edge test dropped the plan's cost-accumulation assertion (plan 4.2 Step 1 specifies asserting `Some(0.02)` via a `cost()` helper). All fixtures carry cost total 0.0, so a `0.0` sum is indistinguishable from a `0.0` replacement: **a regression storing `Some(total)` instead of adding would keep the whole suite green.** The edge input was built to distinguish (0.02 then 0.00). Restore the one-line assert.
2. **NULL-cost path is never asserted anywhere.** No test ingests a file lacking `usage.cost` and checks stored NULL → `"cost_usd": null` in JSON → `-` in human output. Code is correct (`src/cli.rs:561`), but a regression would be silent. Needs one synthetic case (fixtures are all-0.0).
3. **`src/sources/sessions.rs:274`** — warns on unstat-able files, but a file that stats and then fails to **READ** (permissions; invalid UTF-8 via `read_to_string` at `:60`; unparseable line 1 in `read_session_id` at `:321`) is skipped **silently**. The plan explicitly requires `eprintln!` warn + continue. Such a file would silently never appear in `ra sessions`.
4. **`tests/task_4.rs:614`** — the e2e test leaks a live daemon whenever any assertion after spawn fails; `daemon.kill()` runs only on the happy path. **Not hypothetical:** the reviewer found PID 7098, an orphaned `ra daemon --foreground` started 12:45:48, alive 33 minutes against a **deleted** temp repo, re-running its sync pass every 60s (killed it). Same shape at `tests/task_2_4.rs:379`. Fix = a `ChildGuard` struct with a `Drop` that kills.
5. **Mapped-worktree rendering over IPC is untested** — every e2e-visible session has cwd `<home>`, so `"worktree": null` is asserted but `"worktree": "<path>"` never is. The `sessions_json` LEFT JOIN (`src/ipc/server.rs:341-373`) could regress unseen. One synthetic mapped session served over IPC closes it.

## NITS

- `src/cli.rs:512` — leftover doc line "Human-readable one-screen status output." stacked above `run_sessions`' own doc comment. Delete the first.
- `tests/task_4.rs:367` — `assert!(files.iter().all(is_file))` is tautological (`discover_session_files` only collects `is_file` paths).
- Header-placement drift: plan prose says the session header must be line 1; `ingest_file` accepts the first header line anywhere. Matches the plan's own pseudocode, and sync separately enforces line 1 via `read_session_id` — but the module doc should state the actual rule.
- Commit body thinner than the plan's Step 5 draft; title verbatim and verify line present. Cosmetic.

## Section verdicts

A ingest model **concern** (must-fix above; cursor/torn-tail/no-offset-skip/isolation all correct) · B parse errors **ok** · C token/cost **ok** (code; gap is coverage) · D tool_call pairing/hash **ok** · E worktree mapping **ok** (component-wise `starts_with`, longest wins, no force-join; `wt-m` vs `wt-m2` and the string-prefix fool both proven) · F root resolution **ok** · G discovery **ok** · H ticker **ok** · I ipc/cli **ok** · J test quality **concern**

## Independent fixture verification (the key trust check)

The reviewer recomputed from the committed redacted bytes **itself, with Python**, independent of the green run:

| | id | in | out | cache | calls | started | last |
|---|---|---|---|---|---|---|---|
| F1 | `01a1249c-…0ffa` | 11436 | 2253 | 32640 | 4 (4/4 paired) | 1791615597613 | 1791615612968 |
| F2 | `01a1249c-…6357` | 4999 | 886 | 9344 | 2 (2/2) | 1791615626273 | 1791615634451 |
| F3 | `01a1249d-…cffd` | 8609 | 2286 | 17536 | 3 (3/3) | 1791615636731 | 1791615651076 |

**All match the asserts.** Library probe cross-check: F2 → `cost Some(0.0)`, call 0 `args_hash 8678e56658a28f39` / `result_hash 1b6852a38f9f5998` — both match its independent sha256-of-canonical-JSON recomputation. **Reordered nested keys → identical hash**; missing `arguments` → hash of JSON `null`. `Cargo.lock` has no `preserve_order` (BTreeMap ordering holds).

**Test-vacuity check: none of the ten tests is vacuous.** Each was individually judged; the diagnostics report per-test reasoning.

## Scope check — and a correction to the orchestrator

- Diff = 9 files, all inside the plan's declared set (3 new, 6 modified as planned). Only dependency change: `time` features → `["formatting","macros","parsing"]` (also deduping the old duplicated `"macros"`); no new crates; `Cargo.lock` untouched (correct for a feature flip). `status.md` untouched (correct — Task 5).
- **The orchestrator's "one commit for seven sub-tasks is a deviation" was WRONG.** Plan Task 4 Step 5 itself says *"Commit (single Task-4 commit, matching the repo's one-commit-per-task convention [E60])"*. 4.1–4.7 are sub-tasks *within* Task 4; the convention refers to Tasks 1–5. **The worker followed the written plan.** Corrected here.
- The orphaned daemon at 12:45 is consistent with a mid-TDD-red e2e run, not the final code.

## Disposition

- MUST-FIX is unresolved → **gate not passed**. Fix round required.
- Fix round scope: the MUST-FIX (atomicity) + SHOULD-FIX 1–5. Nits folded in if cheap.
