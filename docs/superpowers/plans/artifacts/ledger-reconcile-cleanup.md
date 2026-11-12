# Ledger — repo-assist cleanup + board reconcile

**Date:** 2026-11-12
**Repo:** `/Users/jesse/Developer/repos/repo-assist` · branch `main`
**Skill:** `cezar-reconcile` (read-only → one diff table → apply on yes → never merge/land from reconcile itself)
**Board:** Notion data source `0a767995-7a41-4ec0-9a3f-e53f655bb1f6`, `Repository = 'repo-assist'`

## User instruction

> clean up repo assist. compare against spec and board
> once you plan and land this, update teh board

Sequence: **plan → land → update board.**

## In flight

| Child name | Role | Task | State |
|---|---|---|---|
| `worker-reconcile-evidence` | Worker | Read-only: `git fetch origin` preflight, plan-checkbox units, per-unit merged evidence from `origin/main` only, integration/cez branch contents, spec-docs-vs-reality, stale artifact list | dispatched |

## Board state (read, not modified)

**15 cards, ALL `Status = Done`.** No non-Done cards → nothing to close, no in-flight cezar work.

| Task ID | Task | Plan Path | Notes |
|---|---|---|---|
| AW-5 | Repo hygiene: move stray planning files from root into docs/ | — | |
| AW-6 | Fix bare ra findings showing resolved rows alongside open ones | — | |
| AW-7 | Add Subscribe RPC and make the TUI subscribe + reconnect | impl plan | |
| AW-8 | Plan discovery, linking, observed derivation (upsert_plan never called) | impl plan | P1 |
| AW-9 | Definition-of-done cleanup: no unwrap, zero-writes test, insta snapshots | impl plan | |
| AW-10 | Plan file parsers: superpowers + checklist adapters (src/plans/) | impl plan | P1 |
| AW-11 | Four plan checks (overlap/drift/stalled/checkbox) + Plans tab | impl plan | P1 |
| AW-12 | Phase 5 hardening: README, agents snippet, performance numbers | impl plan | |
| AW-90 | Docs truth pass: retire stale plan checkboxes and verified-facts premises | `docs/status.md` | Plan Path is a SPEC |
| AW-91 | plan.checkbox_mismatch confidence: explicit → inferred | `docs/verified-facts.md` | Plan Path is a SPEC |
| AW-149 | Complete open plan items: write failing test + add middleware | impl plan | run `e7f2a33b`, **"middleware" smells mis-scoped** |
| AW-154 | Fix review findings F1-F5 (rounds 1 and 2) | impl plan | run `c8db9dd1` |
| AW-176 | LANDING — repo-assist — 1 branch(es) → main | — | `cez/c8db9dd1`, Integration Branch `integration/overnight-2026-11-09 @ 198b99d` |
| AW-181 | LANDING — repo-assist — 1 branch(es) → main | — | `cez/c8db9dd1`, **duplicate of AW-176** |

## Board anomalies (to surface, not silently fix)

1. **Duplicate landing cards** — AW-176 and AW-181 are the same landing (`cez/c8db9dd1`). One landing, two cards.
2. **AW-149 mis-scoped text** — "add middleware"; repo-assist is a Rust CLI with no middleware. Text likely borrowed from another repo.
3. **Plan Path points at specs** — AW-90 → `docs/status.md`, AW-91 → `docs/verified-facts.md`. Per task-board, Plan Path should be a plan. These are exactly the non-plan docs swept into `ra`'s `spec` source — same category confusion in two places.
4. **Integration branch ≠ main** — AW-176's `Integration Branch` is `integration/overnight-2026-11-09 @ 198b99d`. Landing credit requires reachability from `origin/main`, not from an integration branch.

## Gates

- `git fetch origin` must succeed or reconcile STOPS (stale base ⇒ false "unshipped").
- **A card saying `Done` is a claim, never evidence.** Credit requires merged evidence on `origin/main`.
- SHIPPED only from `origin/main`; integration/`cez/*`/`recover/*` reach is IN-FLIGHT, never SHIPPED.
- Nothing applies to the board before the diff table is shown.
- Reconcile never merges or lands. Landing is a separate step, after the plan.

## Uncommitted work carried into this task

The prune-stale-worktrees fix (verified: `cargo test` 0 / 201 passed, `fmt` 0, `clippy` 0, `ra plans` 17 rows / 0 stalled, `ra conflicts` 0, `open_findings` 22→17). Two review rounds, APPROVED. Not committed. It is part of what must be **landed** before the board is updated.

## Landing preflight — BLOCKER FOUND

`git merge-base --is-ancestor origin/main HEAD` → **1**. Local `main` `6ecbb2c` is **0 ahead / 8 behind**
`origin/main` `c6b7aca`. The prune fix was built on a stale base. Landing must rebase first and re-run
the full gate on the new base, or it is a false land.

`c6b7aca` = `merge: land AW-154 (cez/c8db9dd1) onto main` — so **AW-154 genuinely landed**, and the
AW-176/AW-181 landing claim is **true**, not CLAIMED.

## Reconcile diff table

| Category | Result |
|---|---|
| Plan checkboxes → flip | **NONE.** 61 units across 9 tracked plans; every one seen is `[x]` **and** SHIPPED with a sha reachable from `origin/main`. |
| Cards → close | **NONE.** All 15 repo-assist cards are already `Done`. |
| Cards → append comment | AW-176 + AW-181 (landing sha), AW-90 (outcome drifted) |
| Cards → create | 3 follow-ups queued as Backlog (D2, D3, merge-conflict guard) |
| Cards → fold | AW-181 → AW-176 (duplicate landing card) |
| Claimed — not credited | `origin/cez/e7f2a33b` (+4), `origin/integration/overnight-2026-11-09` (+5), `origin/cez/148278d7` (+1), `origin/cez/16eaca5c` (+2). Nothing credited from any of these. |
| Cardless run / work | **This session's prune fix had no card.** Surfaced as a finding. |
| FALSIFIED | **NONE.** No card claims a merge that did not happen. |
| UNKNOWN | Informational headings only (title, constraints, out-of-scope sections) — no discrete claims. |

**Headline: the board and plans do not lie about SHIPPED work. What they lie about is CURRENT STATE.**

## Current-state lies (spec docs)

| File | Lie | Truth |
|---|---|---|
| `docs/status.md` | "Latest main commit `c2aa80f`", "177 passed", "101 → 149 tests" | actual tip `c6b7aca`, 201 tests |
| `docs/ra.1.md` | manpage omits real `ra plans tick [--plan <id>] [--apply]` and `--version` | user-visible drift |
| `docs/plan-tasks-2-4.md` | "verified current state" false on every count | history only |
| `docs/OVERNIGHT-PROMPT.md` | frozen at HEAD `2d678e0` / 44 tests / "findings engine does not exist"; links `docs/overnight-notes.md` which never existed | dead |
| impl plan §14/§15 | names `src/model.rs`, `src/tui/tabs/sessions.rs`, `tests/pi_adapter.rs` | never created |
| 2 artifact ledgers | reference plan paths that are gone | dead links |

## Cleanup candidates — NOT actioned, needs an explicit decision

8 fully-shipped plans are archive candidates (`2026-10-19-repo-assist-task-4-sessions`, the two
`review-findings-fix-round*`, `2026-10-19-status-refresh-and-smoke-run`, `2026-10-20-render-doctor-tui`,
`2026-10-28-land-overnight-branches`, `2026-10-28-post-merge-debt-cleanup`). Also: dead `.worktrees/`
containing only `.DS_Store`, `.sonnet-plan-prompt.md` (tracked, orphaned), broken symlink
`.ai/cezar/workflows/maintenance.yaml` → `/cezar/workflows/shared/maintenance.yaml` (no `/cezar` here),
root + `.ai/` + `.worktrees/` `.DS_Store`.
Kept as-is deliberately: no `.bak`/`.orig`/`*.rej` exist; no stashes.

## Board writes planned (after landing)

1. **Fold** AW-181 → AW-176 (`Merged Into`), removing the duplicate landing card.
2. **Comment** AW-176 with landing evidence `c6b7aca`, verified reachable from `origin/main`.
3. **Comment** AW-90 — card is `Done` but `docs/status.md` still carries stale facts. Honest read: true
   when done, drifted since. Not reopening.
4. **Create Backlog** cards for the 3 real follow-ups: D3 (`observed_done = 0`), D2 (spec globs
   hardcoded/over-inclusive), merge-conflict stale-heads guard (`src/checks/mod.rs:247`).
5. **Report without writing**: AW-149's "add middleware" text is mis-scoped for a Rust CLI;
   AW-90/AW-91 `Plan Path` point at specs not plans.

2. `Method::PlansTick` skip records name gone paths — `src/ipc/server.rs:1073`.
3. Duplicate equivalent SQL for "session not on a gone worktree".
4. **D3 — `observed_done = 0` across all rows** vs 80 declared done. Unexamined.
5. **D2 — spec globs hardcoded and over-inclusive** (`docs/**/*.md` picks up status.md, ra.1.md, verified-facts.md, agents-snippet.md, OVERNIGHT-PROMPT.md, plan-tasks-2-4.md).
