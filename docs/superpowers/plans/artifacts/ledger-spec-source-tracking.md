# Ledger — spec source tracking

Repo: `/Users/jesse/Developer/repos/repo-assist`
Branch: `main` (HEAD `d345e47` at start; work is committed directly, no worktree — the plan
artifacts already live in this checkout and a worktree would separate them from the code they describe)
Plan: `docs/superpowers/plans/2026-11-14-spec-source-tracking.md` (see its **AMENDMENT** section — it
supersedes the original task table)
Evidence: `docs/superpowers/plans/artifacts/evidence-spec-source-tracking.md`
Plan review: `docs/superpowers/plans/artifacts/review-spec-source-tracking.md` (self-review only —
`PI_MODEL=claude-sonnet-5-5` skipped the external `claude -p` loop)

## Confirmed decisions (user, in chat)

| # | Decision |
|---|---|
| D1 | **No config key.** Spec scanning is unconditional via `default_adapters()`; `src/config.rs` untouched. The pre-existing `superpowers_globs`/`checklist_globs` keys are inert at runtime and stay inert — out of scope. |
| D2 | **Specs excluded from the findings engine** (`active_plans`, `plan.stalled`, `plan.overlap`, `check/mod.rs:308`). Visible in `ra plans`/`PLANS.md`/JSON, silent in alerts. |
| D3 | **Skip files a plan adapter already claimed** in the same scan (generic seen-path rule, no plans/ path constant for that purpose) **plus** a `/artifacts/` skip, because plan globs are non-recursive and artifacts would otherwise land as `unknown` specs. |
| D4 | Specs parse superpowers-first, then checklist fallback. |

Also confirmed: zero-task spec → `state = "unknown"` (stored, not skipped); machine-readable spec uses
the existing all-checkboxes-checked → `done` rule; **zero-task plans must keep `active`** (scope boundary).

## Daemon test flake — investigation record (not attributed)

HEAD `06ac712`/`d257beb` was seen failing 3 full `cargo test` runs, each time a **different**
daemon-spawning test: `subscribe_rpc_reconnect_kill_restart`, `daemon_writes_nothing_inside_any_worktree`
(`assert!(sock.exists())`, a 5s startup bound), `no_duplicate_rows_after_reconnect`. Each passes in isolation.

Measurements: parent `d345e47` 3/3 clean full suites; HEAD clean in 6 full + 20 targeted runs by an
independent agent, and in 2 simultaneous full suites run by the orchestrator. Two agents (A/B stress
test) returned malformed output and contributed nothing.

**Read-and-verified exoneration of the bind path** (`src/daemon/mod.rs`, orchestrator's own read):
startup order is repo key → pidfile flock → socket dir + stale-socket removal → `Store::open` →
`start_bus` (thread spawns only) → findings/sessions/plans/render thread spawns → `run_accept_loop`
(which binds). `sync_plans` runs in a detached thread with a **3s initial delay**; `plan_globs()` is
consumed only at `src/daemon/bus.rs:111`, inside the drain thread on a `Dirty` event. The commit adds
**no** work to the socket-bind path, so it cannot explain a 5s socket timeout.

**Status: unexplained, intermittent, unattributed.** Ambient machine load was NOT controlled during the
original failing runs (a local llama-server at ~350% CPU, OrbStack, and an unrelated CPU-bound grep were
observed concurrently), so the "parent clean vs HEAD flaky" contrast is not a controlled comparison.
Candidate hypothesis, **unverified**: two daemon instances for the same repo key collide on the pidfile
flock (`lock::acquire_lock` returns `AlreadyExists`, daemon exits before binding). Worth its own task;
out of scope for spec tracking. No test was weakened, skipped, or serialized at any point.

## Decisions D5–D6 (user, after close review)

| # | Decision |
|---|---|
| D5 | **Broaden `unknown`**: a spec is `unknown` when it is not machine-readable, i.e. zero tasks **or** no checkbox anywhere — not only zero tasks. A `### Task N` doc with no checkboxes read as "active, 0/3 done", which falsely implies progress. |
| D6 | **Add a `SOURCE` column to `ra plans`**, so specs and plans are distinguishable in human output. `docs/**/*.md` would otherwise flood the list looking identical to plans. |

## Review round 1 — outcome

`@reviewer` (per-task, T2/T3/T4): APPROVE, no must-fix; flagged `plan.drift` as should-fix.
`@reviewer-deep` (whole branch): **CHANGES**, goal only _partial_.

**Must-fix accepted — specs were NOT silent in findings.** `667a06b` guarded only the 4 enumerated
sites; three consumers were missed:
- `src/checks/plan_checkbox.rs:11` — no plan/source filter → `plan.checkbox_mismatch` fires on spec tasks
- `src/checks/plan_drift.rs:13` — no source guard → `plan.drift` fires on specs
- `src/ipc/server.rs:499` — `stalled` in `plans_json` unguarded → an open spec reports `stalled: true`
  while `plan.stalled` is suppressed, so JSON and findings disagree

Root cause of the miss: the orchestrator's brief enumerated four sites, and the enumeration was
incomplete. Tests covered only `plan.stalled` and `active_plans` — the two checks that were fixed.

The README claim "`unknown` specs … never fire findings" was therefore an overclaim.

**Confirmed good by review:** `unknown` is visible in `ra plans`, `PLANS.md` and JSON (so "is this spec
complete?" is answerable); the `gone` sweep cannot wrongly mark an `unknown` spec gone; zero-task plans
stay `active`; no fixed-set parser of `state` exists; the socket-bind exoneration was independently
confirmed correct.

**Open, not fixed:** commit authorship is `jesse@takethefort.com`, not `jesse@nightlight.io` as AGENTS.md
requires and as the orchestrator instructed. Fixing it means rewriting three unpushed commits — **not done
without explicit user approval**.

## Task status

| id | scope | status | worker | review |
|---|---|---|---|---|
| T1 | `discover.rs` SpecAdapter + seen-path + artifacts skip + `unknown` state; `tests/plans_spec.rs` | **done** `06ac712` | executed | APPROVE |
| T1b | glob matcher tests (`**/` zero-segment) | **done** `d257beb` | executed | covered by T1 review |
| T2 | findings-engine exclusion — sites 1-4 (`active_plans`, `plan_stalled`, `plan_overlap`, `worktree_abandoned`) | **done** `667a06b` | executed | APPROVE per-task, **CHANGES on deep review** |
| T3 | `cli.rs` STATE column width; `PLANS.md` render guard | **done** `ba05a41` | executed | APPROVE |
| T4 | README | **done** `934bcf6` | executed | corrected later |
| T5 | findings leak at `plan_checkbox`, `plan_drift`, `plans_json.stalled` | **done** `6d5c95b` | executed | **must-fix from review** |
| T6 | broadened `unknown` rule (D5) + `parse_checkbox` reuse | **done** `ef06aec` | executed | APPROVE |
| T7 | `SOURCE` column in `ra plans` (D6) | **done** `56ce230` | executed | APPROVE |
| T8 | README accuracy | **done** `6eba329` | executed | corrected again in T10 |
| T9 | `ra drift` spec leak — `Store::plan_worktrees` filter | **done** `148e912` | executed | **must-fix from review** |
| T10 | cleanups: test slack removed, `PLAN` column width 16, dead code | **done** `5d3a905` | executed | APPROVE |

## Close — final state

Nine commits on `main`, `06ac712..5d3a905`, nothing pushed.

**Gate (orchestrator-run, alone):** `cargo fmt --check` exit 0 · `cargo clippy --all-targets -- -D warnings` exit 0 ·
`cargo test` exit 0, **149 passed, 0 failed**. Baseline at `d345e47` was 126 tests.

**Final review:** `@reviewer-deep` round 3 — **APPROVE**, no must-fix, no should-fix. Bottom-up leak sweep
confirmed every `FROM plans` / `workspace.plans` / `workspace.tasks` / `plan_worktrees` site is either guarded
or intentionally unguarded and documented.

**Verified live, not just by test:** `ra plans` lists 7 `source=spec` rows — 6 `unknown`, 1 `active`
(`c687a1a690a0d398`, "repo-assist Implementation Plan", 18 tasks, 0 done). `ra drift` emits no spec ids, so the
command and the `plan.drift` finding now agree.

**Known limitation, accepted:** `ra who-touches` lists specs as `kind: plan` (a spec that references a path
does touch it). Documented in the README rather than "fixed".

**Known limitation, open:** on any edit that touches a `docs/**/*.md` file, the bus drain thread runs a full
`sync_plans`, and `discover_files` does not skip `target/` or `node_modules/`, so each such edit costs a full
worktree walk. Pre-existing shape, now triggered more often because the watch set is wider.

**Authorship:** all nine commits are authored `jesse@takethefort.com`, not `jesse@nightlight.io` per AGENTS.md.
User explicitly chose **not** to rewrite history. Nothing is pushed, so nothing is broken — but the mismatch is
real and recorded here deliberately.

## Verification protocol

Per task: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
Orchestrator re-runs the broad gate itself on every worker report; a worker's summary is not evidence.
