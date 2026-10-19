# Overnight build task — repo-assist Task 5 (remaining CLI commands + findings engine)

You are an autonomous coding agent working **overnight** without a human in the loop. This prompt is your full brief. Read it completely before doing anything.

---

## Your task in one sentence

Finish `repo-assist` — a Rust CLI/daemon (`ra`) that watches a git repo across all worktrees and surfaces agent/plan conflicts, drift, and stalls — by implementing the remaining unimplemented commands and the findings engine.

---

## Where the repo stands

`/Users/jesse/Developer/repos/repo-assist`, branch `main`, HEAD `2d678e0`. **Tree is clean. All tests pass.**

**Current state: `cargo fmt --check` clean · `cargo clippy --all-targets --all-features -- -D warnings` exit 0 · `cargo test` = 44 passed / 0 failed.**

Per-suite counts: cli 1, config 7, fswatch 2, git_repo 3, git_source 8, store 3, task_2_3 2, task_2_4 3, task_3 3, task_4 12.

**Already implemented** (do not rebuild): `ra daemon`, `ra status`, `ra plans`, `ra plan <id>`, `ra drift`, `ra sessions`, plus the store (SQLite + WAL), IPC server/client, fs watcher, git source, worktree event bus, plans data model, and the pi session adapter.

---

## What's left — the exact remaining scope

Verify this yourself with `grep -A3 -E "^\s+\w+\(_\) => Spec" src/cli.rs`, but as of HEAD these are `implemented: false` in the CLI spec table:

| Command | Task | Purpose (from the plan's §7) |
|---|---|---|
| `ra worktrees` | 5 | Worktrees with branch, ahead/behind base, changed-file counts, last activity |
| `ra conflicts` | 5 | Overlaps and merge conflicts between worktrees and between in-flight plans |
| `ra who-touches <path>` | 5 | Worktrees, plans, sessions currently touching a path |
| `ra findings [--open\|--all] [--kind K]` | 5 | List findings |
| `ra ack <id>` | 5 | Acknowledge a finding |
| `ra tui` | 5 | Open the TUI |
| `ra doctor` | 5 | Environment checks (git version, state dir writable, socket, adapters' roots) |
| `ra render` | 2 | Force-render markdown views now |

Plus **RPC methods** that currently fall through to `not_implemented` in `src/ipc/server.rs` (see the `other =>` arm around line 236): `Worktrees`, `Conflicts`, `WhoTouches`, `Findings`, `Ack`, `Subscribe`, `RenderViews`.

**The findings engine does not exist at all.** That is the largest piece and the reason the commands above are stubs — `ra findings`, `ra ack`, `ra conflicts`, and much of `ra conflicts`/`ra status` depend on it.

---

## Read these first, in this order

1. **`status.md`** (repo root) — **THE AUTHORITY on what actually exists.** Note: the upstream plan's task *numbering* does NOT match what was built (its `Task 2.3` is "Findings engine"; this repo's `Task 2.3` is "worktree event bus"). Trust `status.md` and the code, not the plan's numbers.
2. **`docs/2026-10-19-repo-assist-implementation-plan.md`** — the upstream plan. **§7 (CLI), §9 (Findings catalog), §5 (data model), §6 (IPC), §10 (config)** are your spec for the remaining work. Read §9 carefully — it is a precise, table-driven spec with rules, severities, and dedupe keys.
3. **`docs/ra.1.md`** — the man page. It already documents all these commands, so it tells you the intended surface.
4. **`docs/verified-facts.md`** — verified vs unverified assumptions. Note it was written on a *different machine* (Linux/aarch64, cargo 1.97.1); its environment claims do not apply here.
5. **`docs/superpowers/plans/`** — the plan, evidence ledger, and **six review records** from the session that built Task 4. These are gold: they show exactly what kinds of bugs the reviewers catch in this codebase.

---

## The established architecture — follow the existing patterns

Task 4 (`ra sessions`) was built as a full vertical slice and is your template. Mirror it:

- **Source module** `src/sources/sessions.rs` (pure: no daemon types, no tokio) → **store methods** in `src/store/mod.rs` → **RPC handler + `*_json` builder** in `src/ipc/server.rs` (e.g. `sessions_json` at `:336`) → **CLI runner + human printer** in `src/cli.rs` (e.g. `run_sessions`, `print_sessions_human`) → **integration tests** in `tests/task_N.rs`.
- **Test convention:** all tests in `tests/`, **zero inline `#[test]` in `src/`**, one test file per task, `mod common` (see `tests/common/mod.rs`).
- **Commit convention:** one commit per task, message body carrying the verify command and its result.

### Findings engine design (the core new work)

Per the plan's §9, findings are **deduped, lifecycle-managed rows**. The `findings` table already exists in `src/store/schema.sql` (columns include `dedupe_key`, `kind`, `severity`, `subject`, `summary`, `state`, `first_seen_ms`, `last_seen_ms`, `resolved_ms`). Your job is the *engine* that populates it.

Key requirements from §9:
- Each check has a `kind`, trigger, rule, severity, and a **`dedupe_key`** (ids sorted where the key names two entities).
- `plan.drift` is **info** severity normally, **warn** if >10 paths; one finding per plan listing up to 20 paths.
- `worktree.merge_conflict` needs `git merge-tree --write-tree` (git ≥ 2.38). **Cache by `(headA, headB)`.** If git is too old, **disable the check and have `ra doctor` say why** — do not silently skip.
- Task `observed` derivation (`committed` / `in_progress` / `not_started` / `unknown`) is a **pure function**, table-tested, defined in §9. Evidence from inference must carry `confidence=inferred`, and **only `explicit` evidence may justify `error` severity**.
- Findings have a **state lifecycle** (`open` → acknowledged/resolved) — that's what `ra ack` manipulates and what `--open`/`--all` filter on.

**Hard rule from §2 of the plan:** the daemon is **advisory and read-only with respect to work trees** — it never edits, commits, kills, or resolves anything, and never writes inside a work tree. All output goes to its own state directory. **Sources never write findings** — checks read from the store and write findings.

---

## Traps that already bit this codebase — do not rediscover these

These are all real bugs found by adversarial review in this repo. Each cost a review cycle.

1. **Multi-statement writes must be transactional.** This bug appeared **twice** (`upsert_session_with_calls`, then `replace_files` in `src/sources/git.rs`). It is now fixed at the root: **`Store::write` wraps every closure in a transaction** (`src/store/mod.rs:220-238`). So: just use `store.write(|conn| {...})` and you get atomicity for free. **Do NOT add your own transaction inside a `store.write` closure** — `unchecked_transaction` **cannot nest** and fails at runtime with `"cannot start a transaction within a transaction"`.
2. **`PRAGMA wal_checkpoint` inside an open write transaction fails** with `"database table is locked"`. And never swallow its error with `.ok()` — a swallowed checkpoint silently stopped working for exactly this reason. If you checkpoint, do it **outside** any transaction.
3. **macOS `$TMPDIR` is a symlink.** `/var/folders/...` vs the resolved `/private/var/...`. `git worktree list` reports the resolved form while `tempfile::TempDir` hands out the unresolved form — this broke a test. **Canonicalize both sides before comparing paths.**
4. **`ps` does not report cwd.** For process work use `lsof -p <pid> -a -d cwd`. Identify processes via `ps -o comm`, not lsof's command column.
5. **Spawning a child process in a test needs an RAII guard.** A bare `child.kill()` on the last line is skipped on panic — a leaked `ra daemon` survived 33 minutes this way. See `DaemonGuard` in `tests/task_4.rs` / `tests/task_2_4.rs`.
6. **The slug transform for pi session dirs is NOT reversible** — compute forward from a canonicalized cwd, never decode backwards.
7. **Real pi transcripts contain entry types a naive parser doesn't know** (`custom`, `session_info`, `custom_message`, `context_edit` — thousands of occurrences). The existing `src/sources/sessions.rs` **counts unknown types as parse errors**, which is honest for its purpose but means it is *not* a verbatim template for a tolerant consumer.
8. **Never weaken an assertion or delete a test to make something pass.** If a test only passes because an error is swallowed, surface it.

---

## Working discipline — required

- **TDD.** Write the failing test, confirm it is red, implement, confirm green. Report the **red output verbatim** — a test that passes both before and after proves nothing.
- **Verify by running, not reasoning.** Run the stated command and paste real output.
- **One commit per task**, body naming the verify command and its result.
- **Commit locally only: NEVER `git push`.** The user has not authorized a push and explicitly wants local-only commits on this repo.
- **Keep the tree clean.** No stray `ra daemon` processes. `ps ax | grep "[r]a daemon"` should be empty when you're done for the night.
- **Check the premise.** If the plan's diagnosis turns out wrong when you read the code, say so with evidence rather than implementing something you believe is incorrect.

---

## Gates — must hold at every commit

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Baseline to preserve and grow: **44 passing**. Add tests; never remove or weaken them.

---

## Autonomy policy — what to do when stuck

- **Blocked on something the plan decides → follow the plan.**
- **Blocked on something genuinely undecided → make the smallest reasonable choice, log it in a `## Overnight decisions` section of `docs/overnight-notes.md`, and keep moving.**
- **Blocked on something a human must decide → skip it, log it clearly, proceed with what's unblocked.**
- **A task fails twice → stop attempting it, log the exact error, move to the next task.** Do not grind.
- **Never leave the tree broken.** If a task ends half-done, either finish it or revert that task's changes so the tree stays buildable.
- **Never push, never force-push, never rewrite history.**

**Suggested order** (dependency-aware — the findings engine unblocks several commands):
1. Findings engine + its checks (§9), with the `findings` table lifecycle. Biggest piece; unblocks the rest.
2. `ra findings` + `ra ack` (CLI + RPC) — they exercise the engine.
3. `ra worktrees`, `ra who-touches` (mostly queries over existing store data).
4. `ra conflicts` (needs `worktree.overlap` + `merge-tree`, both from step 1).
5. `ra render` (markdown views per §11).
6. `ra doctor` (environment checks).
7. `ra tui` (largest UX piece; `ratatui` + `crossterm` are already dependencies — check `Cargo.toml`).

If you get through 1–5, that is a strong night's work.

---

## Success criteria — how you'll be judged

```
## TASKS COMPLETE
<each>: <done | partial | skipped> — <commit sha> — <verify command + ACTUAL result>

## FINDINGS ENGINE
<the kinds you implemented; which §9 rules are live; anything not implemented and why>

## TEST RESULTS
cargo fmt --check / clippy / cargo test: <real output, real counts>

## NOT DONE
<everything skipped or partial, with the reason>

## OVERNIGHT DECISIONS
<every judgment call, so it can be reviewed>

## BLOCKERS FOR THE USER
<anything needing a human>
```

**Honesty is the top priority.** A truthful "tasks 1–3 done, 4 blocked on X" is far more valuable than a vague success claim. If you did not run a check, say so. If something is unverified, label it unverified.

---

## Boundaries

- **Repo:** `/Users/jesse/Developer/repos/repo-assist` only. You may *read* `/Users/jesse/Developer/repos/ai-dashboard` for reference but must not modify it.
- **Do NOT read or dump real pi transcript contents** into logs, commits, or reports — they contain the user's private data. The redacted fixtures at `tests/fixtures/pi/*.redacted.jsonl` are the sanctioned test inputs; **never use the raw `.jsonl` ones**.
- **Do not `git push`.** Do not install global tooling.
- **Do not add a dependency** without a justification in the commit message. The plan's §3 lists the intended set.
- Do not commit `target/`.
- `docs/ra.1.md` needs updating only if a command's flags actually change — say so if you do.

---

## Start here

1. `cd /Users/jesse/Developer/repos/repo-assist && git log --oneline -5 && git status`
2. Confirm the baseline: `cargo test` (expect 44 passed).
3. Read `status.md`, then the plan's §7 and §9.
4. Read `src/sources/sessions.rs` + `src/ipc/server.rs` (`sessions_json`) + `src/cli.rs` (`run_sessions`) — that vertical slice is your template.
5. Read `docs/superpowers/plans/artifacts/review-task-4-sessions.md` — it shows what reviewers catch here.
6. Begin the findings engine.

Work through the night. Report when done or genuinely blocked.
