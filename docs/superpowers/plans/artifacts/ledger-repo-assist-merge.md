# Ledger — land overnight branches on main (repo-assist)

**Plan:** `docs/superpowers/plans/2026-10-28-land-overnight-branches.md`
**Started:** 2026-10-28
**Status:** in flight

## User decisions (confirmed in chat, this session)

1. Merge set = the 7 real branches, in order. Exclude `-21`, `-16`, both `fix/auto-scan-*`,
   `smoke/full2`, `cez/16eaca5c`.
2. Push to `origin/main` once tests are green.
3. Board: close each row with an evidence comment naming the merge commit; any row still
   unbacked gets reset to `Backlog` with a comment saying what is left.
4. Cezar: Path A live `create_task` with `workflow: superpowers_full`, scope = repo-assist
   follow-ups only (not ai-dashboard / theFall).

## Agents

| Role | Task | Agent ID | Outcome |
|---|---|---|---|
| @worker | execute the 7 merges + push | _pending_ | _pending_ |
| @reviewer | review integrated result | _pending_ | _pending_ |

## Preflight facts (verified, not assumed)

- `cargo test` on `smoke/full2` (== `main` + 1 docs commit): **101 passed, 0 failed**, 18 bins.
- `origin/main` = `eb3379e`; local `main` = `faf2d6a` (one behind, fast-forwardable).
- `upsert_plan` (`src/store/mod.rs:289`): zero production call sites (tests only).
- `Method::Subscribe` (`src/ipc/mod.rs:84`): no server handler; hits `not_implemented` at
  `src/ipc/server.rs:287`.
- Bare `ra findings`: no default state filter (`src/store/mod.rs:694`, `src/cli.rs:104`).
- `README.md` and `docs/agents-snippet.md`: absent on `main`, present on `-22-23`.
- Branches overlap on `src/store/mod.rs`, `src/ipc/server.rs`, `src/checks/mod.rs`,
  `Cargo.lock`, `status.md` → real conflict resolution expected, not fast-forwards.

## Board snapshot before (Notion `🤖 Agent Work`)

`collection://0a767995-7a41-4ec0-9a3f-e53f655bb1f6` — 70 rows total. `repo-assist`: 8 rows,
all `Done`, 0 open. Last edited 2026-10-26 and 2026-10-28 in two bulk flips, no comments.

## Log

- _pending_
