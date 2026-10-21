# Task board payload — repo-assist remaining work

Written 2026-10-21. **Supersedes nothing**; there is no earlier repo-assist board entry.

Notion MCP was unavailable when this was written, so these rows are in board shape to be created verbatim.
Board defaults: data source `collection://0a767995-7a41-4ec0-9a3f-e53f655bb1f6`, `Agent` = `pi`,
`Repository` = `repo-assist`, `Status` = `Backlog`, `Claimed` = `__NO__`.

**Basis:** `docs/2026-10-19-repo-assist-implementation-plan.md` (spec, phases 0-5, tasks 0.1-5.1).
Verified state: `cargo fmt --check` clean, `cargo clippy --all-targets --all-features -- -D warnings` exit 0,
`cargo test` = **85 passed / 0 failed**. So the gap is spec coverage, not code quality.

**The headline:** Phase 3 (Plans) is essentially absent. There is no `src/plans/` module, no plan parser, and
`Store::upsert_plan` (`src/store/mod.rs:289`) has **zero callers** — no code path creates a plan row. Consequently
`ra plans`, `ra plan` and `ra drift` are implemented and unit-tested but operate only on rows that *tests* insert,
so in real use the daemon cannot detect plan drift, overlap, stall or checkbox mismatch. R1-R3 below fix that.

---

## R1 — Plan parsers: superpowers + checklist adapters  · P1

- **Task**: Implement plan file parsers (`src/plans/`)
- **Status**: Backlog · **Priority**: P1 · **Repository**: repo-assist · **Agent**: pi
- **Plan Path**: `docs/2026-10-19-repo-assist-implementation-plan.md` (Task 3.1, §8.1)

**Body.** Create `src/plans/{mod.rs,superpowers.rs,checklist.rs}`. Parse a plan document into tasks: title,
`Create:`/`Modify:`/`Test:` path declarations, and `- [ ]`/`- [x]` checkboxes, producing `TaskRow` values with
`expected_paths` and a declared state (`done` only when every checkbox is `[x]` and at least one exists, else `open`).

- Tests: `tests/plans_parse.rs` plus fixtures under `tests/fixtures/plans/`.
- Must not panic on malformed input; unknown formats yield a plan with **zero tasks and a logged warning**.
- Done when the spec's representative sample (Task 1: Add limiter / Task 2: Wire into router) parses to 2 tasks with
  paths `src/limiter.rs`, `tests/limiter.rs`, `src/router.rs`, and a fuzz-lite test proves no panic on garbage.

## R2 — Plan discovery, linking, `observed` derivation  · P1

- **Task**: Populate plans from files (discovery + `observed` derivation)
- **Status**: Backlog · **Priority**: P1 · **Repository**: repo-assist · **Agent**: pi
- **Blocked By**: R1
- **Plan Path**: `docs/2026-10-19-repo-assist-implementation-plan.md` (Task 3.2)

**Body.** This is the row that makes `ra plans` and `ra drift` real: today nothing ever calls `Store::upsert_plan`.

- Scan plan globs per worktree on `worktree_dirty` events that touch them, plus a slow timer; upsert plans and tasks;
  mark vanished plans `gone`.
- Add `src/plans/derive.rs`: pure `observed` derivation per spec §9, with a truth-table test.
- Extend `src/sources/git.rs` with per-path commit lookup to supply commit evidence.
- Done when a temp-repo scenario shows: committing a task's files flips `observed` to `committed` and records commit
  evidence; a plan file deleted on disk becomes `gone`.

## R3 — Four plan checks + Plans tab  · P1

- **Task**: Implement `plan.overlap` / `plan.drift` / `plan.stalled` / `plan.checkbox_mismatch` + Plans tab
- **Status**: Backlog · **Priority**: P1 · **Repository**: repo-assist · **Agent**: pi
- **Blocked By**: R2
- **Plan Path**: `docs/2026-10-19-repo-assist-implementation-plan.md` (Task 3.3)

**Body.** Create `src/checks/{plan_overlap,plan_drift,plan_stalled,plan_checkbox}.rs` and `src/tui/tabs/plans.rs`.

- Plans without `expected_paths` skip the path-based checks, and `ra plan` must say "no path data" rather than render
  a misleading 0%.
- Plans tab: title, worktree, declared-vs-observed progress, stalled flag; detail pane lists tasks and evidence.
- Done when two worktrees holding plans that list the same file produce `plan.overlap` **before any edit exists**, and
  a plan whose task checkboxes disagree with its `expected_paths` produces `plan.checkbox_mismatch`.

## R4 — `Subscribe` RPC and TUI subscribe/reconnect  · P3

- **Task**: Add the `Subscribe` RPC and make the TUI use it
- **Status**: Backlog · **Priority**: P3 · **Repository**: repo-assist · **Agent**: pi
- **Plan Path**: `docs/2026-10-19-repo-assist-implementation-plan.md` (Task 2.5)

**Body.** `src/ipc/server.rs` has **no** `Subscribe` handler (0 occurrences); the TUI polls on the ~1 s refresh tick
and reconnects by retry. Spec Task 2.5 asks for subscription with delta events and explicit reconnect.

- Add `Subscribe` to the RPC surface, emitting finding/status deltas.
- Switch the TUI to subscribe, keeping the polling path as a fallback when subscription is unavailable.
- Done when killing and restarting the daemon mid-session makes the TUI reconnect and repopulate without crashing,
  covered by a test.

## R5 — Phase 5: README, agents snippet, performance numbers  · P2

- **Task**: Write README + `docs/agents-snippet.md` and record performance numbers
- **Status**: Backlog · **Priority**: P2 · **Repository**: repo-assist · **Agent**: pi
- **Plan Path**: `docs/2026-10-19-repo-assist-implementation-plan.md` (Task 5.1)

**Body.** Both files are **absent** — there is no `README.md` and no `docs/agents-snippet.md`, even though `ra doctor`
already exists.

- README: install, quick start, full config reference, and how to add a plan source or an agent adapter.
- `docs/agents-snippet.md`: the copy-paste agent integration snippet.
- Measure and record: idle daemon CPU < 1 % and RSS < 50 MB with 5 worktrees; `ra status` < 100 ms warm. Cap the
  `events` table and the tool-call ring sizes while doing so.
- Done when the numbers are written in the README and `cargo fmt`/`clippy`/`test` are green.

## R6 — Definition-of-done cleanup  · P2

- **Task**: Satisfy spec §16 — no unwrap in src, zero-writes test, verified-facts resolution
- **Status**: Backlog · **Priority**: P2 · **Repository**: repo-assist · **Agent**: pi
- **Plan Path**: `docs/2026-10-19-repo-assist-implementation-plan.md` (§16 Definition of done)

**Body.** Four DoD/spec-fidelity items are unmet:

1. **2 `unwrap()` calls remain in non-test code** (`grep -rn '\.unwrap()' src`), against a DoD of zero.
2. The DoD requires proof the daemon makes **zero writes inside any work tree** — a test that snapshots each
   worktree's file tree and `git status` before and after a daemon run. Not present.
3. §13 must be fully resolved in `docs/verified-facts.md`; it reads as largely resolved — do a pass and record any
   remaining item as verified or explicitly not-verified with evidence.
4. Spec Task 2.5 asks for the markdown views to be **snapshot-tested with `insta`** — there is no `insta` dependency
   (0 hits in `Cargo.toml`); the 9 `views_render` tests are hand-rolled comparisons. Either add `insta` snapshots or
   record the deviation explicitly and say why. (Related, smaller: the spec names `src/checks/{worktree_overlap,
   merge_conflict,abandoned}.rs` as separate files; the code keeps them in `src/checks/mod.rs`. Functionally
   equivalent — decide whether to split or note the deviation, and don't let it block.)

Done when all four hold and the gates stay green.

## R7 — Fix bare `ra findings` state filter  · P2

- **Task**: Bare `ra findings` must not show resolved rows alongside open ones
- **Status**: Backlog · **Priority**: P2 · **Repository**: repo-assist · **Agent**: pi

**Body.** Acknowledged in `status.md` as the one open known limitation: `ra findings` with neither `--open` nor `--all`
applies **no** state filter, so resolved findings appear mixed in with open ones. Either default to open-only or
require an explicit flag; `--all` must still show everything.

Done when a test proves a resolved finding is hidden by default and visible with `--all`.

## R8 — Repo hygiene: move stray root files under `docs/`  · P3

- **Task**: Move `plan-tasks-2-4.md` and `.sonnet-plan-prompt.md` under `docs/`
- **Status**: Backlog · **Priority**: P3 · **Repository**: repo-assist · **Agent**: pi

**Body.** Both sit at the repo root (`plan-tasks-2-4.md` is 323 lines of planning for the plans data model;
`.sonnet-plan-prompt.md` is a 19-line prompt). Everything else planning-related lives under `docs/superpowers/plans/`,
so these break the convention and clutter the root. Move them, fix any references, no behaviour change.

Done when the root contains only `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `status.md`, and the source dirs.
