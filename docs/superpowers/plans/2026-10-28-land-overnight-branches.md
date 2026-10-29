# Land the overnight branches on main

**Date:** 2026-10-28
**Repo:** `repo-assist` (`ra`)
**Confirmed by user:** merge set, push to `origin/main`, close board rows with evidence, then Path A cezar handoff.

## Why

The spec (`docs/2026-10-19-repo-assist-implementation-plan.md`, 16 tasks over Phase 0–5) is
implemented, but roughly half of it sits on unmerged `overnight-*` branches. `origin/main` is
stuck at `eb3379e` (PR #1). The Notion board shows all 8 repo-assist rows as `Done`, which is
true of the *work* and false of `main`.

Confirmed state at plan time:

- `cargo test` on `main`+`smoke/full2`: **101 passed, 0 failed**, 18 test binaries.
- `upsert_plan` (`src/store/mod.rs:289`) has no call sites outside tests; nothing outside
  `src/plans/` references `plans::`. Plan subsystem is library-only on `main`.
- `Method::Subscribe` exists at `src/ipc/mod.rs:84` with no server handler; falls through to
  `not_implemented` (`src/ipc/server.rs:287`).
- Bare `ra findings` still has no default state filter (`src/store/mod.rs:694`,
  `src/cli.rs:104`), so resolved rows appear — the bug `status.md:146` lists as open.

## Branches to land, in order

| # | Branch | Ahead | Contains |
|---|---|---|---|
| 1 | `overnight-20261022-17` | 1 | `ra findings` default-filter fix (`src/store/mod.rs`, `tests/findings_engine.rs`) |
| 2 | `overnight-20261022-22` | 5 | Parser refinements (plan-scoped ids, tolerant paths, checkbox precision) + plan checks split into `src/checks/plan_*.rs` |
| 3 | `overnight-20261023-1` | 15 | Plan discovery (`src/plans/discover.rs`), 60s ticker, observed derivation, `tests/daemon_zero_writes.rs`, insta snapshots |
| 4 | `overnight-20261022-20` | 6 | `deny(unwrap/expect)` in source, `docs/verified-facts.md` fix (4 commits already in #3) |
| 5 | `overnight-20261022-18` | 7 | `Method::Subscribe` handler, client long-poll, TUI reconnect, `tests/subscribe_rpc.rs` |
| 6 | `overnight-20261022-23` | 3 | Task 5.1: `README.md`, `docs/agents-snippet.md`, `scripts/perf.sh`, `tests/performance.rs`, event/tool_call retention |
| 7 | `overnight-beam-hygiene` | 1 | Moves stray planning files (`status.md`, `plan-tasks-2-4.md`, …) into `docs/` |

`beam-hygiene` is last on purpose: it moves `status.md`, which branches 5 and 6 also edit.

## Explicitly excluded

| Branch | Reason |
|---|---|
| `overnight-20261022-21` | Subset of `-22` (already contained) |
| `overnight-20261022-16` | Nothing ahead of `main` |
| `fix/auto-scan-20261023-050932-_wd-20261022-17` | Already merged |
| `fix/auto-scan-20261023-221030-_wd-20261022-17` | Identical to `main` |
| `smoke/full2` | Cezar smoke-test artifact, docs only |
| `cez/16eaca5c` | Cezar run branch; appends a junk line to `status.md` |

**No branch is deleted.** **No force-push.**

## Procedure

1. Confirm the tree is clean. Tag the pre-merge state: `git tag pre-merge-20261028 eb3379e`.
2. `git checkout main && git merge --ff-only origin/main` (local `main` is `faf2d6a`, one behind).
3. For each branch in order: `git merge --no-ff origin/<branch>`, resolve conflicts, then
   **`cargo build` + `cargo test` must be green before the next merge starts.**
4. A branch that cannot be made green in two attempts is aborted back to the previous merge
   commit and recorded as SKIPPED with the failing test names. Continue with the next branch.
   A skipped merge is reported, never silently dropped.
5. Final `cargo test` green → `git push origin main`. Otherwise stop and report.

## Conflict policy

- `Cargo.lock` / `Cargo.toml` — regenerate (`cargo build`), never hand-merge the lock.
- `src/checks/mod.rs` — take `-22`'s module split, then re-apply `-23-1`'s additions to the
  dispatcher (`run_all_checks`).
- `src/store/mod.rs` — keep every distinct function from both sides; these are additive.
- `src/ipc/server.rs` — keep both handlers (`Subscribe` from `-18`, discovery hooks from `-23-1`).
- `status.md` — union of documented facts. Never delete a limitation that is still real.
- `tests/` — keep every test from both sides. A merge that lowers the test count is wrong.

## Done when

- `origin/main` contains all 7 branches, `cargo test` green, count **> 101**.
- `upsert_plan` has a production call site; `Method::Subscribe` has a handler; bare
  `ra findings` filters resolved out; `README.md` + `docs/agents-snippet.md` exist.
- Notion rows closed with a comment naming the merge commit; any row still unbacked reset to
  `Backlog` with a comment saying what is left.

## Out of scope

Cezar delegation (separate step), branch deletion, the removed `Claimed`/`Agent` board columns,
and any new feature work beyond what the branches already contain.
