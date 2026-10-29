# Ledger — land overnight branches on main (repo-assist)

**Plan:** `docs/superpowers/plans/2026-10-28-land-overnight-branches.md`
**Started:** 2026-10-28
**Closed:** 2026-10-29
**Status:** COMPLETE

## User decisions (confirmed in chat)

1. Merge set = the 7 real branches, in order. Exclude `-21`, `-16`, both `fix/auto-scan-*`,
   `smoke/full2`, `cez/16eaca5c`.
2. Push to `origin/main` once tests are green.
3. Board: close each row with an evidence comment naming the merge commit; reset anything
   still unbacked to `Backlog`.
4. Cezar: Path A live `create_task` with `workflow: superpowers_full`, repo-assist scope only.

## Agents

| Role | Task | Agent ID | Outcome |
|---|---|---|---|
| @worker | 7 merges + push | `02eb3820-d46d-47c` (gc'd) | d8e3acb, 123 passed |
| @reviewer | round 1 | `1ec6d1ce-a20e-46f` (gc'd) | 2 MUST-FIX, 5 SHOULD-FIX |
| @worker | MUST-FIX + SHOULD-FIX | `6bd2c110-2d58-494` | 080d0e3, 124 passed ×5 |
| @reviewer | round 2 (cold, round-1 agent gc'd) | `b4d82c9d-76d9-46f` | **VERDICT: PASS** |
| @worker | commit cleanup plan | (inline, 17.9s) | 67082a7 |

## Result

`main` == `origin/main` == `67082a7`. Suite **124 passed, 0 failed**, verified over 8
consecutive runs (5 by the worker, 3 by the orchestrator). No hangs.

Merges, in order: `fd09782` (-22-17 findings filter) · `1d9782e` (-22-22 parsers + checks
split) · `80e6989` (-23-1 plan discovery + insta + zero-writes) · `83859fd` (-22-20 deny
unwrap/expect) · `5ce0fdc` (-22-18 Subscribe + reconnect) · `635c613` (-22-23 Task 5.1 docs +
retention) · `d8e3acb` (beam-hygiene docs move). Fix commit `080d0e3`.

Three conflicts, all resolved and reviewed clean: `src/checks/mod.rs` (kept `-22`'s module
split, re-applied `-23-1`'s 9 checks — none lost), `src/ipc/server.rs` (kept `-22-20`'s
match rewrite — no handler arm lost), `src/store/mod.rs` (retention pruning + `notify_waiters`
+ `list_events_since` coexist, prune→notify ordering correct).

## Board (`🤖 Agent Work`, `collection://0a767995-7a41-4ec0-9a3f-e53f655bb1f6`)

8 repo-assist rows, all `Done`, each now carries one evidence comment naming its merge
commit plus the proving `path:line` or test. Phase 5 row flagged in its comment that
`README.md:98` named a non-existent `src/agents/` module — corrected in `080d0e3`.

## Cezar

`repo-assist/368df232-5381-4f64-9d34-517124f6bb36`, `superpowers_full`, status **running**,
branch `cez/368df232`, cockpit `http://127.0.0.1:4321/p/repo-assist/tasks/368df232-5381-4f64-9d34-517124f6bb36`.
Scope: post-merge debt cleanup per `docs/superpowers/plans/2026-10-28-post-merge-debt-cleanup.md`.
`done` is a claim — review `get_changes` before merging.

## Gotchas recorded

- **The merge source was the real story, not `main`.** My first status report said 6 board
  rows were false. They were implemented-but-unmerged. Always check remote branches before
  declaring spec items unbuilt: `git rev-list --left-right --count origin/main...origin/<b>`
  for every branch, not just the checked-out one.
- **Local `main` was stale** (`faf2d6a` vs `origin/main` `eb3379e`), which corrupted the
  ahead/behind counts until `git fetch` + comparing against `origin/main`.
- **`cargo test` was intermittently hanging** — `drop(child)` on a spawned daemon without
  `kill()`/`wait()` orphaned processes that spun on deleted temp dirs. A green suite was not
  reliable evidence until the guards landed. Now: kill-on-drop + `Stdio::null()` in all seven
  daemon-spawning test files.
- Nothing left open: no skipped branches, no lowered test counts, no unresolved MUST-FIX.
