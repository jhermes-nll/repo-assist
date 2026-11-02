# Spec Source Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ra` also tracks spec documents (`docs/**/*.md`, minus the plan directory) as rows in the `plans` table with `source = "spec"`; a spec with zero parsed tasks is stored with `state = "unknown"`.

**Architecture:** Add one more `PlanAdapter` (`SpecAdapter`) to `default_adapters()`; `sync_plans` gets two small spec-only rules (skip files under `docs/superpowers/plans/`; zero tasks → `unknown`). Everything downstream (`plans` table, tasks, evidence, IPC, CLI, views, checks) is reused unchanged because every consumer of `plans.state` tests `== "active"` or prints the string verbatim [E24][E25].

**Tech Stack:** Rust (edition per `rust-toolchain.toml`), rusqlite, serde/toml, cargo integration tests under `tests/`. No new dependency.

**Evidence:** docs/superpowers/plans/artifacts/evidence-spec-source-tracking.md (HEAD d345e47)

**Spec:** none on disk. The user-confirmed outcome (three decisions) is reproduced under Global Constraints.

## Global Constraints

- Decision 1: new config glob set `spec_globs`, default `["docs/**/*.md"]`; must EXCLUDE `docs/superpowers/plans/` (incl. its `artifacts/` subdir [E9]); stored in the same `plans` table with distinct `source` value `"spec"`.
- Decision 2: a spec yielding zero parsed tasks is still stored, `state = "unknown"`.
- Decision 3: a machine-readable spec uses the exact plan rule — all tasks done → `done`, else `active`. No new completeness logic.
- **Scope boundary: plan behaviour must not change.** A zero-task file matched by the `superpowers` or `checklist` adapter keeps `state = "active"` [E5]. `unknown` is assigned only when `adapter.name() == "spec"`. Covered by test `zero_task_plans_keep_active_state` (T2).
- Smallest change: reuse `plans` table [E15], reuse parsers [E10-E13], no new abstraction layer, no speculative config options, no new dependency.
- No schema change, no migration, no `user_version` bump (`plans.state` is unconstrained `TEXT NOT NULL`) [E15].
- Verification (every task): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Only files named in a task may be edited by that task.

## Corrections to the brief's "known facts" (stated, not silently planned around)

1. **Line numbers are off** (content right): adapter loop is `discover.rs:157`, state assignment `:178-183`, gone-marking `:213-222`, `discover_files` `:299`, `ParsedPlan` `mod.rs:91`, `stalled` `server.rs:495`, `plans` table `schema.sql:25` [E36].
2. **Config globs are inert at runtime.** `sources.plans.{superpowers_globs,checklist_globs}` are parsed/defaulted but nothing reads them; the daemon calls `default_adapters()` with hardcoded globs in both places [E2][E18][E19]. A new `spec_globs` key would be equally inert unless wired. **See Decision D1.**
3. **Six, not four, zero-task docs**: `agents-snippet.md` and `verified-facts.md` also have no tasks/checkboxes [E28].
4. The "no CHECK constraint" fact is confirmed [E15]. The implementation-plan doc is predicted to land `active` (18 Task headings, 30 unchecked / 1 checked) — a prediction from reading, verified by T2's test [E29].
5. `plans_discover.rs` never asserts a zero-task plan's state, so the scope-boundary test is new [E32]. Test layout otherwise matches the brief [E31].
6. A trap the brief did not mention: the glob language has no exclusion syntax [E8], and `docs/**/*.md` also matches `docs/superpowers/plans/artifacts/*.md`, which the plan adapter's glob does NOT claim [E9]. A "skip files another adapter already claimed" approach would therefore leak artifacts as specs; a path-prefix exclusion is required.

## Decisions needed from the caller (do not guess)

- **D1 — config key is inert (blocking for README wording, not for code).** Options: **(A, plan default)** add `spec_globs` to config exactly like its two siblings (parsed, validated, defaulted; daemon uses built-in default) and say so in the README. Zero ripple. **(B)** also wire `spec_globs` into the daemon: ticker has `state.config` [E18] but `start_bus` has no config param and is called from `tests/task_2_3.rs:49,87` [E18] — needs a signature change plus a `default_adapters_with(...)` constructor; custom spec globs would then work while the two plan keys stay inert (inconsistent). **(C)** wire all three keys (fixes the pre-existing gap; widest; changes plan discovery for users with customised globs). Recommendation: A now, C as a separate change. If B/C is chosen, T4's README note is dropped and a new task is needed (not planned here).
- **D2 — specs enter the findings engine.** Because decision 3 gives open specs `active`, they count toward `status.active_plans`, `plan.stalled`, `plan.overlap` (cross-worktree, same `expected_paths`) and `worktree.abandoned` exactly like plans [E27]. `unknown` specs do not. The repo's own implementation-plan doc (30 unchecked boxes) will trigger `plan.stalled` after `stalled_hours`. This follows decision 3 literally; if unwanted, add `p.source != "spec"` in 4 places (`server.rs:424`, `plan_stalled.rs:17`, `plan_overlap.rs:16`, `checks/mod.rs:308`) — not planned.
- **D3 — `docs/plans/` double-indexing.** The default checklist glob `docs/plans/**/*.md` [E2] is also matched by `docs/**/*.md`, so such files would be both a checklist plan and a spec. Only `docs/superpowers/plans/` was confirmed for exclusion, so the plan excludes only that. To also exclude, add `"docs/plans/"` to `SPEC_EXCLUDE_PREFIXES` (one line; T2 test would need one extra assertion). No `docs/plans/` exists in this repo today [E28].
- **D4 — parser.** Plan uses superpowers-then-checklist (same order as `parse_plan`, [E11]) so a checkbox-only doc is machine-readable. A superpowers-only parser would make such docs `unknown`. Low risk; flagged only.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `src/config.rs` | `PlansSource.spec_globs` + default | T1 |
| `tests/config.rs` | config default / back-compat tests | T1 |
| `src/plans/discover.rs` | `SpecAdapter`, exclusion, `unknown` state, registration | T2 |
| `tests/plans_spec.rs` (new) | end-to-end spec/plan/scope tests via `sync_plans` | T2, T4 |
| `src/cli.rs` | widen STATE column for `unknown` | T3 |
| `tests/views_render.rs` | `render_plans` shows `unknown` | T3 |
| `README.md` | document `spec_globs`, `source = spec`, `unknown` | T4 |

Every `plans.state` consumer, with disposition (from E24/E25):

| Site | Behaviour with `unknown` | Action |
|---|---|---|
| `src/ipc/server.rs:424-425` `WHERE state='active'` count | not counted | none (correct) |
| `src/ipc/server.rs:495` `state == "active"` stalled | `stalled = false` | none (correct) |
| `src/checks/plan_stalled.rs:17`, `plan_overlap.rs:16` `!= "active"` | skipped | none (correct) |
| `src/checks/mod.rs:308` `== "active"` | not matched | none (correct) |
| `src/plans/discover.rs:213-222` gone-marking | `unknown` rows can become `gone` | none |
| `src/plans/discover.rs:178-183` assignment | produces `unknown` | **T2** |
| `src/cli.rs:450-460` `{:<6}` column | 7-char value misaligns by one column | **T3** |
| `src/views/mod.rs:96-101` `PLANS.md` | prints verbatim | none; test in T3 |
| `src/ipc/server.rs:577-` `plan_json`, `store/mod.rs:939-945 PlanInfo`, JSON consumers | pass-through string | none |
| `tasks.observed = 'unknown'` (`schema.sql:38-47`, `derive.rs:49`) | different column/namespace [E26] | none |

---

### Task 1: `spec_globs` config key

**Files:**
- Modify: `src/config.rs:171-180` (`PlansSource`), `:243-251` (default fns), `:283-290` (`default_plans_source`) [E21]
- Test: `tests/config.rs` (append after `user_config_overrides_defaults`, uses existing `write` helper) [E23]

**Interfaces:**
- Consumes: nothing.
- Produces: `Config.sources.plans.spec_globs: Vec<String>`, default `["docs/**/*.md"]`; `fn default_spec_globs() -> Vec<String>` (private).

- [ ] **Step 1: Baseline.** Run `cargo test 2>&1 | tail -15`; record pass/fail in your report (assumption A1: expected green).

- [ ] **Step 2: Write the failing tests** (append to `tests/config.rs`):

```rust
#[test]
fn spec_globs_default_is_docs_md() {
    let c = Config::default_config();
    assert_eq!(c.sources.plans.spec_globs, vec!["docs/**/*.md"]);
}

#[test]
fn existing_configs_without_spec_globs_still_load() {
    // No config files at all.
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    let c = Config::load(home, home.to_str().unwrap()).unwrap();
    assert_eq!(c.sources.plans.spec_globs, vec!["docs/**/*.md"]);

    // A config written before spec_globs existed: [sources.plans] with only the two old keys.
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".config/repo-assist/config.toml",
        "[sources.plans]\nsuperpowers_globs = [\"plans/*.md\"]\nchecklist_globs = [\"TODO.md\"]\n",
    );
    let c = Config::load(home, home.to_str().unwrap()).unwrap();
    assert_eq!(c.sources.plans.superpowers_globs, vec!["plans/*.md"]);
    assert_eq!(c.sources.plans.checklist_globs, vec!["TODO.md"]);
    assert_eq!(c.sources.plans.spec_globs, vec!["docs/**/*.md"]);
}

#[test]
fn spec_globs_can_be_overridden_and_typos_still_rejected() {
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".config/repo-assist/config.toml",
        "[sources.plans]\nspec_globs = [\"specs/*.md\"]\n",
    );
    let c = Config::load(home, home.to_str().unwrap()).unwrap();
    assert_eq!(c.sources.plans.spec_globs, vec!["specs/*.md"]);

    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".config/repo-assist/config.toml",
        "[sources.plans]\nspec_glob = [\"x\"]\n",
    );
    assert!(Config::load(home, home.to_str().unwrap()).is_err());
}
```

- [ ] **Step 3: Run to verify failure.** `cargo test --test config 2>&1 | tail -20` → FAIL: compile error `no field spec_globs on type PlansSource`.

- [ ] **Step 4: Implement.** In `src/config.rs`:
  - Add to `PlansSource` after `checklist_globs`:
    ```rust
        /// spec-document globs (stored with `source = "spec"`; plan dirs are excluded in discovery).
        #[serde(default = "default_spec_globs")]
        pub spec_globs: Vec<String>,
    ```
  - Add after `default_checklist_globs`:
    ```rust
    fn default_spec_globs() -> Vec<String> {
        vec!["docs/**/*.md".to_string()]
    }
    ```
  - In `default_plans_source()` add `spec_globs: default_spec_globs(),`. (It is the only `PlansSource { .. }` literal [E21]; the compiler confirms.)

- [ ] **Step 5: Run to verify pass.** `cargo test --test config 2>&1 | tail -20` → all pass (existing + 3 new). Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.

- [ ] **Step 6: Commit.**
```bash
git add src/config.rs tests/config.rs
git commit -m "feat(config): add sources.plans.spec_globs (default docs/**/*.md)"
```

---

### Task 2: `SpecAdapter`, exclusion, and `unknown` state

**Files:**
- Modify: `src/plans/discover.rs` — add after `ChecklistAdapter` impl (`:100`); `default_adapters` (`:104-109`); `sync_plans` loop (`:157-183`) [E3][E4][E5]
- Create: `tests/plans_spec.rs`

**Interfaces:**
- Consumes: `crate::plans::{superpowers, checklist}::parse(&str) -> ParsedPlan` [E10-E13]; `PlanAdapter` trait [E1]; `Store`, `sync_worktrees`, `common::GitRepo` test helpers [E31].
- Produces (pub, used by T4): `pub const SPEC_SOURCE: &str = "spec"`; `pub struct SpecAdapter { pub globs: Vec<String> }` with `Default` = `["docs/**/*.md"]`; `default_adapters()` returns `[superpowers, checklist, spec]`; `plan_globs()` now includes `docs/**/*.md` (bus trigger, desired [E20]).

- [ ] **Step 1: Write the failing tests.** Create `tests/plans_spec.rs`:

```rust
//! Spec-source tracking: docs/**/*.md indexed as source="spec"; plan dirs excluded;
//! zero-task specs are "unknown"; plan behaviour unchanged.

mod common;

use std::path::{Path, PathBuf};

use repo_assist::plans::discover::{default_adapters, sync_plans, PlanAdapter, SpecAdapter};
use repo_assist::sources::git::sync_worktrees;
use repo_assist::store::Store;

struct Harness {
    _repo: common::GitRepo,
    store: Store,
    wt: PathBuf,
    path: String,
    wt_id: i64,
}

fn setup() -> Harness {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let wt = repo.paths[0].clone();
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = Store::open(&db).expect("open store");
    sync_worktrees(&store, repo.path()).expect("sync_worktrees");
    let (wt_id, path): (i64, String) = store
        .read(|conn| {
            conn.query_row(
                "SELECT id, path FROM worktrees WHERE path LIKE '%/wt-a'",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
        })
        .expect("worktree row");
    Harness { _repo: repo, store, wt, path, wt_id }
}

fn write(h: &Harness, rel: &str, text: &str) {
    let p = h.wt.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn sync(h: &Harness) {
    sync_plans(&h.store, h.wt_id, Path::new(&h.path), default_adapters()).expect("sync_plans");
}

/// (source, file, state) for every plan row of the worktree, ordered by file.
fn rows(h: &Harness) -> Vec<(String, String, String)> {
    h.store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT source, COALESCE(file, ''), state FROM plans WHERE worktree_id = ?1 ORDER BY file",
            )?;
            let mut out = Vec::new();
            for r in stmt.query_map([h.wt_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })? {
                out.push(r?);
            }
            Ok(out)
        })
        .expect("rows")
}

/// (source, state) of the single row for `file`, if any.
fn row_for(h: &Harness, file: &str) -> Option<(String, String)> {
    rows(h)
        .into_iter()
        .find(|r| r.1 == file)
        .map(|r| (r.0, r.2))
}

#[test]
fn spec_adapter_is_registered_last_with_default_glob() {
    assert_eq!(SpecAdapter::default().globs, vec!["docs/**/*.md"]);
    assert_eq!(SpecAdapter::default().name(), "spec");
    let names: Vec<String> = default_adapters()
        .iter()
        .map(|a| a.name().to_string())
        .collect();
    assert_eq!(names, vec!["superpowers", "checklist", "spec"]);
    // Bus trigger glob set must include spec files.
    assert!(repo_assist::plans::plan_globs().contains(&"docs/**/*.md".to_string()));
}

#[test]
fn zero_task_spec_is_unknown() {
    let h = setup();
    write(&h, "docs/status.md", "# Status\n\nJust prose, nothing machine-readable.\n");
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/status.md"),
        Some(("spec".to_string(), "unknown".to_string()))
    );
}

#[test]
fn machine_readable_spec_gets_the_plan_rule() {
    let h = setup();
    write(&h, "docs/open.md", "# Open\n\n### Task 1: a\n- [x] a\n- [ ] b\n");
    write(&h, "docs/finished.md", "# Finished\n\n### Task 1: a\n- [x] a\n");
    write(&h, "docs/list.md", "# List\n\n- [ ] one\n- [x] two\n");
    sync(&h);
    assert_eq!(row_for(&h, "docs/open.md"), Some(("spec".into(), "active".into())));
    assert_eq!(row_for(&h, "docs/finished.md"), Some(("spec".into(), "done".into())));
    assert_eq!(row_for(&h, "docs/list.md"), Some(("spec".into(), "active".into())));
}

#[test]
fn plan_files_are_excluded_from_spec_globs() {
    let h = setup();
    write(&h, "docs/superpowers/plans/with-task.md", "# P\n\n### Task 1: a\n- [ ] a\n");
    write(&h, "docs/superpowers/plans/zero.md", "# Zero\n\nprose only\n");
    write(&h, "docs/superpowers/plans/artifacts/ledger.md", "# Ledger\n\nE1 | x\n");
    write(&h, "docs/notes.md", "# Notes\n");
    sync(&h);
    let all = rows(&h);
    assert!(
        !all.iter().any(|r| r.0 == "spec" && r.1.starts_with("docs/superpowers/plans/")),
        "plan dir leaked into specs: {all:?}"
    );
    assert!(
        !all.iter().any(|r| r.1.contains("artifacts/")),
        "artifacts must not be indexed at all: {all:?}"
    );
    assert_eq!(
        all.iter().filter(|r| r.1 == "docs/superpowers/plans/with-task.md").count(),
        1,
        "exactly one row (no double-indexing): {all:?}"
    );
    assert_eq!(row_for(&h, "docs/notes.md").map(|r| r.0), Some("spec".to_string()));
}

#[test]
fn zero_task_plans_keep_active_state() {
    // SCOPE BOUNDARY: `unknown` is spec-only; plan adapters behave exactly as before.
    let h = setup();
    write(&h, "docs/superpowers/plans/zero.md", "# Zero\n\nprose only\n");
    write(&h, "TODO.md", "# Todo\n\nnothing yet\n");
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/superpowers/plans/zero.md"),
        Some(("superpowers".to_string(), "active".to_string()))
    );
    assert_eq!(
        row_for(&h, "TODO.md"),
        Some(("checklist".to_string(), "active".to_string()))
    );
}

#[test]
fn repos_own_implementation_plan_is_tracked_as_a_real_state_spec() {
    let h = setup();
    write(
        &h,
        "docs/impl.md",
        include_str!("../docs/2026-10-19-repo-assist-implementation-plan.md"),
    );
    sync(&h);
    let (source, state) = row_for(&h, "docs/impl.md").expect("row");
    eprintln!("implementation-plan doc landed as source={source} state={state}");
    assert_eq!(source, "spec");
    // Predicted `active` (18 Task headings, 30 unchecked boxes); `done` is acceptable if
    // someone ticks everything later. It must never be unknown/gone.
    assert!(state == "active" || state == "done", "state was {state}");
    let tasks: i64 = h
        .store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE plan_id IN (SELECT id FROM plans WHERE file = 'docs/impl.md' AND worktree_id = ?1)",
                [h.wt_id],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert!(tasks > 0, "expected parsed tasks, got {tasks}");
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test --test plans_spec 2>&1 | tail -20` → FAIL: unresolved import `SpecAdapter` (compile error).

- [ ] **Step 3: Implement in `src/plans/discover.rs`.**
  1. After the `ChecklistAdapter` `impl PlanAdapter` block (ends ~`:100`) add:
  ```rust
  /// `plans.source` value for spec/design documents.
  pub const SPEC_SOURCE: &str = "spec";

  /// Spec files never come from under these prefixes: plan files (and their `artifacts/`,
  /// which `docs/**/*.md` would otherwise match) live there. The glob language has no
  /// exclusion syntax, so this is a path-prefix check in `sync_plans`.
  const SPEC_EXCLUDE_PREFIXES: &[&str] = &["docs/superpowers/plans/"];

  /// Spec adapter: default glob `docs/**/*.md`. Same task/declared-state rules as plans
  /// (superpowers headings first, then checklist boxes); a spec with zero tasks is stored
  /// `unknown` by `sync_plans`. Uses the silent parsers, not `parse_plan`, which warns on
  /// every zero-task parse and would log on each 60s sync pass.
  #[derive(Clone, Debug)]
  pub struct SpecAdapter {
      pub globs: Vec<String>,
  }

  impl Default for SpecAdapter {
      fn default() -> Self {
          Self {
              globs: vec!["docs/**/*.md".to_string()],
          }
      }
  }

  impl PlanAdapter for SpecAdapter {
      fn name(&self) -> &str {
          SPEC_SOURCE
      }
      fn globs(&self) -> &[String] {
          &self.globs
      }
      fn parse(&self, text: &str) -> ParsedPlan {
          let sp = crate::plans::superpowers::parse(text);
          if !sp.tasks.is_empty() {
              return sp;
          }
          let cl = crate::plans::checklist::parse(text);
          if !cl.tasks.is_empty() {
              return cl;
          }
          ParsedPlan {
              title: if sp.title.is_empty() { cl.title } else { sp.title },
              ..Default::default()
          }
      }
  }
  ```
  2. `default_adapters()`: append `Box::new(SpecAdapter::default()),` after the checklist entry.
  3. In `sync_plans`, replace the head of the per-file loop body (currently: `read_to_string` → `parse` → `rel`) with `rel` first, then exclusion, then read/parse:
  ```rust
              for file in discover_files(wt_path, glob) {
                  let rel = match file.strip_prefix(wt_path) {
                      Ok(r) => r.to_string_lossy().replace('\\', "/"),
                      Err(_) => continue,
                  };
                  if adapter.name() == SPEC_SOURCE
                      && SPEC_EXCLUDE_PREFIXES.iter().any(|p| rel.starts_with(p))
                  {
                      continue;
                  }
                  let Ok(text) = fs::read_to_string(&file) else {
                      continue;
                  };
                  let parsed = adapter.parse(&text);
  ```
  (delete the old later `let rel = match ...` block). Plan adapters are unaffected: same operations, `rel` merely computed earlier (behaviour-neutral).
  4. Replace the state assignment (`:179-183`) with:
  ```rust
                  let state = if declared_state == "done" {
                      "done"
                  } else if adapter.name() == SPEC_SOURCE && parsed.tasks.is_empty() {
                      // Present but not machine-readable. Spec-only: zero-task plans stay "active".
                      "unknown"
                  } else {
                      "active"
                  };
  ```

- [ ] **Step 4: Run to verify pass.** `cargo test --test plans_spec -- --nocapture 2>&1 | tail -30` → 6 pass; note the printed `implementation-plan doc landed as source=spec state=...` line (expected `active`; report the actual value). Then full suite: `cargo test 2>&1 | tail -15` (guards A2/A3: existing `plans_discover`, `task_2_4`, `views_render` stay green), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`.

- [ ] **Step 5: Commit.**
```bash
git add src/plans/discover.rs tests/plans_spec.rs
git commit -m "feat(plans): track docs/**/*.md as source=spec; zero-task specs are state=unknown"
```

---

### Task 3: display of `unknown` (CLI column, PLANS.md)

**Files:**
- Modify: `src/cli.rs:450-460` (`print_plans_human`: header `{:<6}` and row `{:<6}` → `{:<7}`) [E25]
- Test: `tests/views_render.rs` (append; mirror the `ViewData` shape used by `sample_data()` at `:8`) [E33][E34]

**Interfaces:**
- Consumes: `repo_assist::views::{render_plans, ViewData}` (pub) [E33].
- Produces: nothing used by other tasks.

- [ ] **Step 1: Write the failing test** (append to `tests/views_render.rs`; reuse the file's existing `json` import; set the `status`/`findings`/`sessions` fields the same way `sample_data()` does):

```rust
#[test]
fn plans_view_shows_unknown_state_verbatim() {
    let d = ViewData {
        status: json!({}),
        plans: json!({"plans": [
            {"id": "abc123", "title": "Status", "state": "unknown", "declared_done": 0, "task_count": 0}
        ]}),
        findings: json!({"findings": []}),
        sessions: json!({"sessions": []}),
    };
    let out = render_plans(&d, 0);
    assert!(out.contains("abc123  unknown  Status  (0/0)"), "{out}");
}
```

- [ ] **Step 2: Run.** `cargo test --test views_render plans_view_shows_unknown 2>&1 | tail -10`. Expected: PASS already (renderer is state-agnostic [E34]) — this is a regression guard, not a red test. Record that honestly.

- [ ] **Step 3: Implement.** In `src/cli.rs` `print_plans_human`, change both format specs `{:<6}` → `{:<7}` (header line and the per-row line). No test (function is private and prints; `ra plans` needs a live daemon). Verified by compile + review only.

- [ ] **Step 4: Run.** `cargo test 2>&1 | tail -15`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`.

- [ ] **Step 5: Commit.**
```bash
git add src/cli.rs tests/views_render.rs
git commit -m "fix(cli): widen plans STATE column for 'unknown'; guard PLANS.md rendering"
```

---

### Task 4: README and cross-task guard

**Files:**
- Modify: `README.md:45-49` (config block) and add a short paragraph after it [E35]; leave `:91-94` unchanged (see D1)
- Test: `tests/plans_spec.rs` (append one test)

**Interfaces:**
- Consumes: `Config::default_config().sources.plans.spec_globs` (T1); `SpecAdapter` (T2).
- Produces: nothing.

- [ ] **Step 1: Write the failing/guard test** (append to `tests/plans_spec.rs`; add `use repo_assist::config::Config;`):

```rust
#[test]
fn spec_adapter_default_matches_config_default() {
    // The daemon uses the adapter default (config keys are not yet wired — D1), so the two
    // must not drift.
    assert_eq!(
        SpecAdapter::default().globs,
        Config::default_config().sources.plans.spec_globs
    );
}
```
Run `cargo test --test plans_spec spec_adapter_default_matches 2>&1 | tail` → PASS (T1+T2 merged). Requires T1 and T2 complete.

- [ ] **Step 2: Edit README.** In the config block add the third line:
```toml
[sources.plans]
superpowers_globs = ["docs/superpowers/plans/*.md"]
checklist_globs   = ["TODO.md", "PLAN.md", "docs/plans/**/*.md"]
spec_globs        = ["docs/**/*.md"]   # never matches docs/superpowers/plans/
```
and directly after the block add:

> **Specs.** Files matching `spec_globs` are tracked in the same plans list with `source = "spec"` (JSON `source` field; `ra plans` does not show a source column). Parsing and the `done`/`active` rule are identical to plans. A spec with no parseable tasks or checkboxes is listed with state `unknown` ("present but not machine-readable"); it is never counted active or stalled. Anything under `docs/superpowers/plans/` (including `artifacts/`) is a plan, never a spec. Note: as with the two plan keys above, the daemon currently applies the built-in defaults; these keys are validated but not yet consumed.

(If D1 is resolved B/C, drop the final sentence.) Also `rg -n "ra plans|PLANS.md" README.md` and add `unknown` to any list of possible plan states if one exists.

- [ ] **Step 3: Full verification.**
```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | tail -20
```
Expected: all green. Report the three exit statuses and the final `test result:` lines.

- [ ] **Step 4: Commit.**
```bash
git add README.md tests/plans_spec.rs
git commit -m "docs: document spec_globs, source=spec and state=unknown"
```

---

## AMENDMENT — user-confirmed 2026-11-14 (AUTHORITATIVE: supersedes D1–D4 and the task list above)

These four answers were given in chat after the plan was written. Where this section conflicts with anything above, this section wins.

**D1 → built-in default only, NO config key.** Add nothing to `src/config.rs`. Task 1 above is **DROPPED** entirely (no `spec_globs` field, no `default_spec_globs`, no config tests). Spec scanning is unconditional via `default_adapters()`. Also **DROP** T4's `spec_adapter_default_matches_config_default` test (nothing to compare against). README documents that specs are always scanned and that there is no config key for them.

**D2 → specs are excluded from the findings engine.** Add a `source != 'spec'` guard at:
- `src/ipc/server.rs:424-425` (`active_plans` count — specs must not inflate it)
- `src/checks/plan_stalled.rs:17`
- `src/checks/plan_overlap.rs:16`
- `src/checks/mod.rs:308`

Specs still appear in `ra plans` / `plan_json` / `PLANS.md`. Required tests: an **open** spec (`state=active`) produces **no** `plan.stalled` finding and is **not** counted in `active_plans`; a **plan** with the same content and state still does/is. This is a two-sided test — the exclusion must not be implemented so broadly that it silences real plans.

**D3 → skip files a plan adapter already claimed in this scan** (generic rule; NOT a per-directory constant). Track a seen-path set across the adapter loop in `sync_plans`: plan adapters run first (spec is appended last in `default_adapters()`), and the spec adapter skips any relative path already processed in this scan. Assert that ordering in a test so a future reorder cannot silently reintroduce duplicates.

*Additional rule, required by the planning evidence:* `docs/superpowers/plans/artifacts/*.md` is matched by `docs/**/*.md` but claimed by **no** plan adapter, because the plan glob `docs/superpowers/plans/*.md` is non-recursive. The claimed-path rule therefore will not protect it, and every ledger/review record would land as an `unknown` spec. Skip any relative path containing an `/artifacts/` segment. One line. Required test: a file under `docs/superpowers/plans/artifacts/` produces **zero** rows.

**D4 → confirmed as planned:** superpowers parser first, checklist fallback (`SpecAdapter::parse` exactly as specified in T2 Step 3).

### Revised task list (use this, not the table above)

| id | owner | depends on | files |
|---|---|---|---|
| T1 | @worker | — | `src/plans/discover.rs`, `tests/plans_spec.rs` (create) |
| T2 | @worker | T1 | `src/ipc/server.rs`, `src/checks/plan_stalled.rs`, `src/checks/plan_overlap.rs`, `src/checks/mod.rs`, + one test file |
| T3 | @worker | T1 | `src/cli.rs`, `tests/views_render.rs` |
| T4 | @worker | T1, T2, T3 | `README.md` |

T1 is T2-as-written above minus the config field, plus the claimed-path set and the `/artifacts/` skip. T2 is new (findings exclusion). T3 is T3-as-written. T4 is T4-as-written minus the config-default test and minus the `spec_globs` README line.

**Still standing from above:** the scope-boundary test (`zero_task_plans_keep_active_state`), the `unknown` state rule being spec-only, superpowers-then-checklist parsing, the state-consumer table, the per-task verification commands (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`), and the rollback note.

**Explicitly out of scope:** `ra plans` gaining a source column (JSON already carries `source`); wiring the two already-inert plan config keys.

## Rollback

All changes are additive code + one config field. No schema change. Reverting the commits leaves `source='spec'` rows in users' DBs; the next `sync_plans` pass marks them `gone` through the existing vanished-plan UPDATE [E38][E6]. No data migration either way.

## Task table

| id | owner | depends on | parallel group | files |
|---|---|---|---|---|
| T1 | @worker | — | A | `src/config.rs`, `tests/config.rs` |
| T2 | @worker | — | A | `src/plans/discover.rs`, `tests/plans_spec.rs` (create) |
| T3 | @worker | — | A | `src/cli.rs`, `tests/views_render.rs` |
| T4 | @worker | T1, T2, T3 | B | `README.md`, `tests/plans_spec.rs` |

Group A collision check: file sets are disjoint; `PlansSource` is constructed only in `config.rs` [E21], adapters never read `Config` [E2][E19], and `cli.rs`/`views` treat `state` as an opaque string [E24][E25] — so no symbol in T1/T3 is in T2's blast radius. T4 edits `tests/plans_spec.rs` after T2 creates it, hence serial.

## Self-Review

1. **Spec coverage.** D1 spec scope + exclusion → T1 (config), T2 (`SpecAdapter`, `SPEC_EXCLUDE_PREFIXES`, test `plan_files_are_excluded_from_spec_globs`). Same table + distinct source → T2 (`SPEC_SOURCE`). Decision 2 → T2 state rule, test `zero_task_spec_is_unknown`. Decision 3 → no new logic; test `machine_readable_spec_gets_the_plan_rule`. Scope boundary → test `zero_task_plans_keep_active_state`. (i)-(iv): T2 tests, T2 tests, T2 test, T1 test `existing_configs_without_spec_globs_still_load`. Real doc check → test `repos_own_implementation_plan_...`. State-consumer enumeration → table above. README → T4. Verification commands → every task.
2. **Placeholder scan.** No TBD/"similar to"; the only non-code steps are the README prose (given) and `cli.rs` width edit (exact specs given). T3's Step 2 is explicitly a guard, not a red test.
3. **Type consistency.** `SPEC_SOURCE`, `SpecAdapter { globs }`, `default_spec_globs`, `spec_globs` names match across T1/T2/T4; test helper names (`setup`, `write`, `sync`, `rows`, `row_for`) are defined in T2 and reused in T4 only through the appended test, which uses none of them.

## Open questions / unverified

- D1-D4 above (D1 and D2 most material).
- A1-A3 (suite green at HEAD; snapshots and other tests unaffected) are unrun — shell was restricted during planning; each task's verify step runs the full suite.
- E29's "predicted `active`" and E12's task-title quirk (`### Task 0.1:` yields title `.1: ...`) are from reading only; the quirk is pre-existing and out of scope.
- `ra plans` human output has no source column, so specs are indistinguishable from plans there (JSON has `source`). Not requested; flagged.
