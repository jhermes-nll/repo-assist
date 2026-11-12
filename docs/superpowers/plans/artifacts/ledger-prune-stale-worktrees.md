# Ledger — prune stale worktrees (repo-assist)

**Date:** 2026-11-12
**Plan:** `docs/superpowers/plans/2026-11-12-herdr-ra-pane.md` (rev 2, Task 1)
**Repo:** `/Users/jesse/Developer/repos/repo-assist` · branch `main` · tree clean at start
**Mode:** current checkout, no worktree isolation

## In flight

| Child name | Role | Task | State |
|---|---|---|---|
| `worker-prune-stale-worktrees` | Worker | Task 1 — prove the 4 suspect worktrees are dead, then prune/mark-gone + exclude from stalled/abandoned/status/plans aggregates, TDD | complete — verified by orchestrator |
| `reviewer-prune-stale-worktrees` | Reviewer | Round 1 review of the uncommitted diff | complete — **CHANGES-REQUIRED**, 1 must-fix |
| `worker-prune-stale-worktrees-r2` | Worker (resumed) | Fix must-fix + same-class read paths | complete — verified by orchestrator |
| `reviewer-prune-stale-worktrees-r2` | Reviewer (resumed) | Round 2 — verify closure + hunt regressions | dispatched |

## Verification round 2 (run by orchestrator)

| Command | Exit | Result |
|---|---|---|
| `cargo test` | **0** | passed=201 failed=0 (was 196) |
| `cargo fmt --check` | **0** | clean |
| `cargo clippy --all-targets -- -D warnings` | **0** | clean |
| `ra plans --json` | **0** | 17 rows |
| `ra conflicts --json` | **0** | **0 rows** (was 5 phantom `plan.overlap`) |
| `ra status --json` | **0** | `open_findings` **22 → 17**, `worktree_count` 1, `active_plans` 9 |

### Guards added in round 2
- `check_plan_overlap` — `src/checks/plan_overlap.rs:21-25` (the must-fix)
- `who_touches_json` — worktrees/plans/sessions guards
- `sessions_json` — `WHERE wt.id IS NULL OR wt.present = 1`, matched to the `status` predicate
- TUI worktree rows skip `present: false`, agreeing with `ra status` as reference
- Cross-surface test `no_read_surface_reports_a_gone_worktree` as a guard for future read paths

### Still to confirm at review round 2
- merge-conflict edge case via stale stored heads (round-1 NIT) — no fix was claimed, may be dropped
- whether the new guards can wrongly hide a **live** worktree
- whether `wt.id IS NULL` is reachable and leaks


## Review round 1 — must-fix

**`check_plan_overlap` lacks the `present` guard** — `src/checks/plan_overlap.rs:15-32`.
Verified against the live daemon DB: all **5 open `plan.overlap` findings** are active plans on
`present=0` worktrees (`3bca848bb798142b`, `f6cf6b07713fcbd4`, on deleted
`/private/tmp/cezar-integrate/*`). `src/daemon/findings.rs:56` → `sync_findings` **re-emits them
on every tick**, so `ra conflicts` / FINDINGS.md keep surfacing plan ids that `ra plans` now hides.
Same defect as the stalled path, one guard missed.

Also routed in the same pass: `who-touches` + `sessions` missing the filter, and the TUI worktree
count disagreeing with `ra status`.

### Cleared by the reviewer
- `present` semantics correct (retain-before-upsert ordering sound; 4 rows `present=0` in live DB)
- mark-and-exclude only — no row deletions, no disk deletions anywhere
- idempotency holds (code + test)
- new tests are non-trivial: they fail without the filters
- `git()` safe on missing dirs
- **`cargo clean` footnote is genuine**, not a masked bug: `env!("CARGO_MANIFEST_DIR")` was baked
  into a binary from a deleted checkout — a loud failure. Accepted.


## Verification (run by orchestrator, not taken from the worker's word)

| Command | Exit | Result |
|---|---|---|
| `cargo test` | **0** | passed=196 failed=0 |
| `cargo fmt --check` | **0** | clean |
| `cargo clippy --all-targets -- -D warnings` | **0** | clean |
| `ra plans --json` | **0** | **17 rows, 0 stalled** (baseline 77 rows / 6 stalled) |

Rows by source after fix: `superpowers 10, spec 7`. All 4 suspect worktrees were confirmed absent from
disk *and* from git admin entries.

### Root cause (worker's finding, believed)

The worktree mark-gone convention already existed (`present = 0`, set by `sync_worktrees` Phase C,
`src/sources/git.rs:279-300`) and worked — but the read paths never filtered on it. Additionally,
discovery via `git worktree list` misses the dir-deleted-but-unpruned case. So the bug was
write-correct / read-wrong.

### Open question raised by the fix

`status` after the fix reports `open_findings: 22`. If any of those findings belong to gone
worktrees, the phantom-data problem is only half fixed — a finding is what a user acts on.
Handed to the reviewer as a must-check.

## Gate state

- Plan gate: **satisfied** (plan rev 2 on disk)
- Approval gate: **satisfied** — user chose "prune any dead stuff in repo-assist if its actually dead"
- Deadness gate: **delegated to the worker as STEP 0** — it must prove each path ABSENT and paste `git worktree list` before writing code. If any path EXISTS it must stop and report.

## User's condition

> prune any dead stuff in repo-assist **if its actually dead**

So: no pruning without per-path proof. A git-registered-but-absent worktree and a
removed worktree are different cases and the worker must say which each is.

## Baseline to compare against

- `ra plans --json`: 77 rows, 17 distinct plan files × 5 worktrees
- `stalled`: 6 (all on non-existent worktrees)
- `state` distribution: active 42, unknown 30, done 5
- `observed_done > 0`: 0 (suspected separate defect D3, NOT in scope here)
- `cargo test` baseline: ~149 tests
- Expected after: rows toward ~17, stalled toward 0

## Explicitly out of scope for this child

- herdr-app mod (Task 3)
- spec glob configurability / narrowing (Task 2)
- `observed_done = 0` (D3 — separate investigation)
- any config change, any disk deletion, any commit

## Known risks carried

1. Shipping a UI over phantom data — why this task runs first.
2. `observed_done = 0` across all rows may be a larger bug; not addressed here.
3. `ra` CLI is daemon-dependent; `--no-spawn` opts out; daemon-down gives exit 2.

## CLOSE — review round 2: APPROVE

| Finding | Status | Evidence |
|---|---|---|
| `check_plan_overlap` missing `present` guard (MUST-FIX) | **CLOSED** | `src/checks/plan_overlap.rs:21-25`, mirrors `plan_stalled.rs:26`. Test `plan_overlap_skips_gone_worktree_but_fires_on_live` (`tests/stale_worktree_prune.rs:394-424`) fails pre-fix. Live DB: 5 stored `plan.overlap` now `resolved`. |
| `who-touches` read path | **CLOSED** | `src/ipc/server.rs:859`, `:875-880`, `:917` + `who_touches_excludes_gone_worktree` (`:427-447`) covering all three entity kinds. |
| `sessions` read path | **CLOSED** | Filter `src/ipc/server.rs:583`; status count aligned `:460-464` + `sessions_json_excludes_gone_worktree_sessions` (`:450-481`). |
| TUI count vs `ra status` | **CLOSED** | `src/tui/client.rs:163-167` + genuine e2e `tui_worktree_rows_match_status_excluding_gone` (`tests/tui.rs:494-561`, real daemon over real socket). |
| merge-conflict via stale stored heads | **STILL-OPEN (dropped by worker)** | No change to `src/checks/mod.rs:247`. Not currently firing (0 conflict findings) but a branch change on a gone head would emit `worktree.merge_conflict` naming a deleted worktree. |

No regressions. Guards key only on `present == false`; live rows get `present=1` every sync
(`upsert_worktree`). `wt.id IS NULL` in `plans_json` (`src/ipc/server.rs:507`) is reachable and
intentional — worktree-less spec rows, leaks no path.

Cross-surface test `no_read_surface_reports_a_gone_worktree` (`tests/stale_worktree_prune.rs:485-556`)
scans **six** surfaces (`plans_json`, `status_json`, `sessions_json`, `who_touches_json`,
`conflicts_json`, `plan_drift_value`) plus `run_all_checks`. Declared exemptions: `worktrees_json`
(the inventory endpoint that carries `present`), `Method::Plan` (numeric id only), `Method::PlansTick`.

## Follow-ups (accepted NITs, not shipped)

1. **Merge-conflict stale-heads guard** — `src/checks/mod.rs:247`; dropped, still live as an edge case.
2. **`Method::PlansTick` skip records name gone paths** — `src/ipc/server.rs:1073`. Informational; `plans_tick` also *heals* (`src/plans/discover.rs:281-290` marks plans `state='gone'`).
3. **Two equivalent SQL formulations for "session not on a gone worktree"** — `src/ipc/server.rs:460-464` vs `:583`. Provably equivalent today; drift risk.
4. Any **new read path** must be hand-added to the cross-surface sweep.
5. **D3 still unexamined** — `observed_done = 0` across all rows.

## Status

Task 1 **done and reviewed**. Not committed. Nothing deleted from disk; mark-and-exclude only.


- Task 2 — add `spec_globs` to `PlansSource` and narrow the default (exclude
  `docs/superpowers/plans/artifacts/**` and the 6 non-plan docs).
- Task 3 — herdr-app mod: clone the `tabs` header mod, add a `panelButton`, `exec` `ra status --json`.
- D3 — find why `tasks.observed` is never `done`.
