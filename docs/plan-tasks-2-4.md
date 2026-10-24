# Task 2.4: Plans data model

Grounded against the code on disk at commit `5163673` (Task 2.3: worktree event bus). This
spec covers **only** Task 2.4: store-level CRUD for `plans → tasks → evidence`, plus the
RPC + CLI wiring so `ra plans` and `ra plan <id>` stop returning "not yet implemented". It does
**not** cover parsing real plan files (that is the superpowers/checklist importer, referenced in
`docs/verified-facts.md` Item 2 as "Task 3.1") or drift computation (`ra drift`, `cli.rs` `Spec{task: 3}`).

## 1. Scope, precisely

Verified current state:
- `src/store/schema.sql:25-57` already defines `plans`, `tasks`, `evidence` with `CREATE TABLE IF
  NOT EXISTS`, applied once at `user_version` 0→1 (`src/store/migrations.rs:8`,
  `CURRENT_VERSION = 1`). `grep` across `src/` finds no `INSERT INTO plans|tasks|evidence`
  anywhere — nothing writes these tables today.
- `src/ipc/mod.rs:76-77` declares `Method::Plans` and `Method::Plan`; `src/ipc/server.rs:195`
  routes every method not explicitly matched (which includes both) to a generic
  `not_implemented` error reply.
- `src/cli.rs:58-71` already defines `PlansArgs { json }` and `PlanArgs { id, json }`. `src/cli.rs:209-216`
  maps `Command::Plans`/`Command::Plan` to `Spec { implemented: false, task: 3 }`, so
  `run()` (`src/cli.rs:261-275`) falls through to `run_implemented`, which returns
  `"`plans` is not yet implemented (scheduled for Task 3)"`.

Task 2.4 delivers:
1. Three `Store` write methods (`upsert_plan`, `upsert_task`, `append_evidence`) — persistence
   only, no importer.
2. Two read-side JSON builders in `src/ipc/server.rs` (`plans_json`, `plan_json`), following the
   existing `status_json` precedent (`src/ipc/server.rs:199-237`): raw SQL inside
   `store.read(...)`, assembled with `serde_json::json!`.
3. `Method::Plans` / `Method::Plan` match arms in `dispatch_request` (`src/ipc/server.rs:138-197`).
4. `run_plans` / `run_plan` functions in `src/cli.rs`, following `run_status`
   (`src/cli.rs:359-375`) — `resolve_and_connect`, `client.call(...)`, print JSON or a human table.
5. Flip `Plans(_)` and `Plan(_)` to `Spec { implemented: true }` in `cli.rs::spec()`, and add
   their arms to `run()`'s match (next to `Daemon`/`Status`). `Drift(_)` stays
   `{ implemented: false, task: 3 }` — out of scope (see §6).

No plan-generator, parser, or subcommand that produces `PlanRow`/`TaskRow` values ships in this
task. Tests exercise the store API directly, exactly as `tests/task_2_3.rs` drives
`bus::start_bus` directly without a CLI entry point.

## 2. Schema changes

**No new tables.** `plans.worktree_id` (already a FK to `worktrees.id`) plus
`tasks.expected_paths` (already a JSON array column) are sufficient for the future drift view
(`worktree_files` for that worktree vs. the union of `expected_paths` across its plans' tasks) —
that's a derived read query for Task 3, not stored state Task 2.4 needs to maintain. A dedicated
"plan↔worktree changed-file" table would just duplicate `worktree_files` and would need its own
invalidation; not needed.

**One new index**, because `evidence` has no index on its lookup column and `ra plan <id>` will
query `evidence WHERE task_id IN (...)` on every call:

```sql
CREATE INDEX IF NOT EXISTS idx_evidence_task ON evidence(task_id);
```

Add this line to the bottom of `src/store/schema.sql` (next to the existing
`idx_tasks_plan`/`idx_tool_calls_session` indexes). Because `migrate()` only reapplies
`schema.sql` when the file's `user_version` is below `CURRENT_VERSION`
(`src/store/migrations.rs:44-55`), and a database created under Task 1.1/2.x is already at
version 1, this index would silently never be created on existing on-disk databases unless the
version is bumped:

```rust
// src/store/migrations.rs
pub const CURRENT_VERSION: u32 = 2; // was 1
```

`apply_schema` (`src/store/migrations.rs:59-67`) reruns the *entire* bundled `schema.sql` via
`execute_batch`; every statement in it is `CREATE TABLE/INDEX IF NOT EXISTS`, so rerunning it
against an existing version-1 database is safe and additive — it only creates the new index, not
destructive. No custom `ALTER`/data-migration step is needed.

## 3. Store API surface (`src/store/mod.rs`)

New types and methods added to `Store`, following the `WtRow`/`upsert_worktree` and
`append_event` precedents in `src/sources/git.rs:124-131,163-178`.

```rust
/// One row to upsert into `plans` (Task 2.4). Keyed on `id`.
pub struct PlanRow<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub title: &'a str,
    pub file: Option<&'a str>,
    pub worktree_id: Option<i64>,
    pub content_hash: Option<&'a str>,
    pub state: &'a str,
}

/// One row to upsert into `tasks` (Task 2.4). Keyed on `id`.
pub struct TaskRow<'a> {
    pub id: &'a str,
    pub plan_id: &'a str,
    pub ordinal: i64,
    pub title: &'a str,
    pub declared: &'a str,
    /// Serialized to the `expected_paths` JSON array column.
    pub expected_paths: &'a [String],
    /// Serialized to the `depends_on` JSON array column.
    pub depends_on: &'a [String],
}

/// One row to append to `evidence` (Task 2.4). No natural key; always inserted, never upserted.
pub struct EvidenceRow<'a> {
    pub task_id: &'a str,
    pub kind: &'a str,
    pub reference: &'a str, // `ref` is a Rust keyword; column is still named `ref`
    pub ts_ms: i64,
    pub confidence: &'a str,
}

impl Store {
    /// Upsert one `plans` row, keyed on `id`. `first_seen_ms` is set on first insert and left
    /// untouched on conflict; `last_seen_ms` is always refreshed to `now_ms`.
    pub fn upsert_plan(&self, row: &PlanRow<'_>, now_ms: i64) -> Result<(), StoreError>;

    /// Upsert one `tasks` row, keyed on `id`. Deliberately does **not** touch `observed` /
    /// `observed_reason` on conflict — those columns are owned by the (future) drift/derive
    /// step, not by whatever calls `upsert_task` (an importer, or a test).
    pub fn upsert_task(&self, row: &TaskRow<'_>) -> Result<(), StoreError>;

    /// Append one `evidence` row; returns its row id.
    pub fn append_evidence(&self, row: &EvidenceRow<'_>) -> Result<i64, StoreError>;
}
```

Concrete SQL:

```sql
-- upsert_plan
INSERT INTO plans(id, source, title, file, worktree_id, content_hash, state, first_seen_ms, last_seen_ms)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
ON CONFLICT(id) DO UPDATE SET
  source = excluded.source, title = excluded.title, file = excluded.file,
  worktree_id = excluded.worktree_id, content_hash = excluded.content_hash,
  state = excluded.state, last_seen_ms = excluded.last_seen_ms;

-- upsert_task
INSERT INTO tasks(id, plan_id, ordinal, title, declared, expected_paths, depends_on)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
ON CONFLICT(id) DO UPDATE SET
  ordinal = excluded.ordinal, title = excluded.title, declared = excluded.declared,
  expected_paths = excluded.expected_paths, depends_on = excluded.depends_on;

-- append_evidence
INSERT INTO evidence(task_id, kind, ref, ts_ms, confidence) VALUES (?1, ?2, ?3, ?4, ?5);
```

`upsert_plan`/`upsert_task` must be real upserts, not `DELETE` + re-`INSERT` (the pattern
`replace_files` uses for `worktree_files`, `src/sources/git.rs:160-175`): both `tasks.plan_id`
and `evidence.task_id` are `ON DELETE CASCADE` FKs, so deleting a `plans` or `tasks` row to
"refresh" it would cascade-delete every task/evidence row underneath, destroying history a
re-import should preserve.

No store-level read methods are added. Composing the `plans`/`plan` JSON views is handler logic
(§4), matching how `status_json` lives in `src/ipc/server.rs` rather than on `Store` — `Store`'s
existing read surface (`read`, `worktree_dirty_view`) stays generic/reusable, while
view-shaping SQL stays next to the IPC method it serves.

## 4. IPC handler contracts and CLI wiring

### `src/ipc/server.rs`

Two new private functions, same shape as `status_json` (`src/ipc/server.rs:199-237`):

```rust
/// Build the result for `Method::Plans`: one summary row per plan.
fn plans_json(store: &Store) -> Result<Value, crate::store::StoreError> {
    store.read(|conn| {
        // SELECT p.*, COUNT(t.id) task_count,
        //   SUM(t.declared='done') declared_done, SUM(t.observed='done') observed_done
        // FROM plans p LEFT JOIN tasks t ON t.plan_id = p.id GROUP BY p.id ORDER BY p.last_seen_ms DESC
        // -> json!({ "plans": [ { id, source, title, file, worktree_id, state,
        //      first_seen_ms, last_seen_ms, task_count, declared_done, observed_done }, ... ] })
    })
}

/// Build the result for `Method::Plan`: `None` when `id` has no row (-> `not_found`).
fn plan_json(store: &Store, id: &str) -> Result<Option<Value>, crate::store::StoreError> {
    store.read(|conn| {
        // 1. SELECT * FROM plans WHERE id = ?1 -> None if no row.
        // 2. SELECT * FROM tasks WHERE plan_id = ?1 ORDER BY ordinal.
        // 3. For each task: SELECT * FROM evidence WHERE task_id = ?1 ORDER BY ts_ms.
        // -> json!({ id, source, title, file, worktree_id, state, first_seen_ms, last_seen_ms,
        //      "tasks": [ { id, ordinal, title, declared, expected_paths, depends_on,
        //        observed, observed_reason, "evidence": [ { id, kind, ref, ts_ms, confidence } ] } ] })
    })
}
```

`dispatch_request` (`src/ipc/server.rs:138-197`) gains two arms, inserted before the generic
`other =>` catch-all:

```rust
Method::Plans => {
    let result = tokio::task::block_in_place(|| plans_json(store))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    write_reply(stream, &json!({ "id": req.id, "result": result })).await?;
}
Method::Plan => {
    let id = req.params.get("id").and_then(Value::as_str);
    match id {
        None => write_reply(stream, &json!({ "id": req.id,
            "error": { "code": "invalid_params", "message": "missing 'id'" } })).await?,
        Some(id) => {
            let found = tokio::task::block_in_place(|| plan_json(store, id))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            match found {
                Some(result) => write_reply(stream, &json!({ "id": req.id, "result": result })).await?,
                None => write_reply(stream, &json!({ "id": req.id,
                    "error": { "code": "not_found", "message": format!("no plan '{id}'") } })).await?,
            }
        }
    }
}
```

### `src/cli.rs`

No new `Args` structs — `PlansArgs`/`PlanArgs` already exist (`src/cli.rs:57-71`). Changes:

1. `spec()` (`src/cli.rs:209-216`): `Plans(_)` and `Plan(_)` → `Spec { implemented: true, task: 2 }`
   (see §6 on the task-number mismatch). `Drift(_)` is unchanged.
2. `run()` (`src/cli.rs:270-274`): add
   `Command::Plans(args) => run_plans(&args), Command::Plan(args) => run_plan(&args),` next to
   the existing `Daemon`/`Status` arms, instead of falling through to `run_implemented`.
3. Two new functions mirroring `run_status`/`print_status_human`
   (`src/cli.rs:359-407`):

```rust
fn run_plans(args: &PlansArgs) -> Result<i32, anyhow::Error> {
    let mut client = resolve_and_connect(true)?;
    let reply = client.call(crate::ipc::Method::Plans, serde_json::Value::Null)?;
    match reply {
        crate::ipc::Reply::Result { result, .. } => {
            if args.json { println!("{}", serde_json::to_string_pretty(&result)?); }
            else { print_plans_human(&result); }
        }
        crate::ipc::Reply::Error { error, .. } => anyhow::bail!("daemon error [{}]: {}", error.code, error.message),
    }
    Ok(0)
}

fn run_plan(args: &PlanArgs) -> Result<i32, anyhow::Error> {
    let mut client = resolve_and_connect(true)?;
    let reply = client.call(crate::ipc::Method::Plan, serde_json::json!({ "id": args.id }))?;
    match reply {
        crate::ipc::Reply::Result { result, .. } => {
            if args.json { println!("{}", serde_json::to_string_pretty(&result)?); }
            else { print_plan_human(&result); }
        }
        // "not_found" surfaces as a plain daemon-error bail, same as any other error code —
        // exit code 2, message on stderr, no separate branch needed.
        crate::ipc::Reply::Error { error, .. } => anyhow::bail!("daemon error [{}]: {}", error.code, error.message),
    }
    Ok(0)
}
```

`PlansArgs`/`PlanArgs` have no `--no-spawn` field (unlike `StatusArgs`), so `run_plans`/`run_plan`
pass `true` to `resolve_and_connect` unconditionally — consistent with the structs as they exist
today; adding `--no-spawn` to them is out of scope here.

## 5. Test plan

New file `tests/task_2_4.rs`, doc-commented like `tests/task_2_3.rs` ("Task 2.4 acceptance: ...").
Write the tests first (they must fail against current `main`), then implement §§2–4 until green.

1. **Store round-trip + upsert semantics** (direct `Store` calls, no CLI/IPC — mirrors
   `tests/store.rs`):
   - `upsert_plan` twice with the same `id` but a different `title`/`state` leaves one row,
     with the second call's values and the *first* call's `first_seen_ms`.
   - `upsert_task` + `append_evidence`, then `upsert_task` again for the same task `id`: the
     evidence row inserted before the second upsert still exists afterward (proves upsert, not
     delete-then-insert, per §3's FK-cascade hazard).
   - `upsert_task` twice does not reset a non-default `observed` value set by a raw `UPDATE`
     between the two calls (proves `upsert_task` never touches `observed`/`observed_reason`).
2. **`ra plans` / `ra plan <id>` over IPC** (spawn a daemon against a `tests/common::GitRepo`,
   call `Store::upsert_plan`/`upsert_task`/`append_evidence` directly against its DB file, then
   drive the CLI or `ipc::Client` exactly like `tests/cli.rs`'s `daemon_roundtrip_status_and_second_daemon_fails`):
   - `ra plans --json` includes the inserted plan with correct `task_count`/`declared_done`.
   - `ra plan <id> --json` returns tasks ordered by `ordinal` and each task's evidence ordered
     by `ts_ms`.
   - `ra plan <unknown-id>` exits 2 and prints the daemon's `not_found` message on stderr (no
     panic, no stack trace).
3. Regression: `ra drift` (still unimplemented) continues to print
   `"`drift` is not yet implemented (scheduled for Task 3)"` — one assertion, guards against
   accidentally flipping the wrong `Spec` arm.

Then, mirroring the Task 2.1–2.3 commit discipline:
```
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
all green before committing.

## 6. Open decisions / assumptions

- **Who writes `plans`/`tasks`/`evidence` rows?** Nobody, in this task. `docs/verified-facts.md`
  Item 2 already earmarks a markdown-plan parser ("Task 3.1", for superpowers/checklist-style
  plan files) as the real producer. Task 2.4 only proves the storage + RPC + CLI layer works,
  exercised by tests calling `upsert_plan`/`upsert_task`/`append_evidence` directly. If a
  plan-generator subcommand (e.g. `ra plan import <file>`) is wanted sooner, that's new scope,
  not part of 2.4.
- **Task-number mismatch.** `cli.rs::spec()` currently labels `Plans`/`Plan`/`Drift` all as
  coarse `task: 3`, but this spec (commit-grain "Task 2.4") implements `Plans`/`Plan` without
  `Drift`. §4 proposes relabeling `Plans`/`Plan` to `task: 2` once implemented and leaving
  `Drift` at `task: 3`. Confirm this is the intended reading of "Task 2.4" before implementing —
  alternatively, `Plans`/`Plan` could stay labeled `task: 3` with only `implemented` flipped, if
  the coarse numbers are meant to track the *original* phase plan rather than delivery order.
- **`observed`/`observed_reason` stay inert.** Every task inserted in this phase keeps the
  schema default `observed = 'unknown'`. Computing a real value (comparing `expected_paths`
  against `worktree_files`/git log) is explicitly Task 3's "drift" job; Task 2.4's
  `upsert_task` must not pre-empt it by guessing.
- **No plan→worktree changed-file table** (§2) — flagged explicitly in the prompt for this spec;
  recommendation is "not needed," but if Task 3's drift query turns out to be too slow as a
  live join (unlikely at expected row counts), revisit as a materialized view, not a new base
  table.
- **`PlansArgs`/`PlanArgs` lack `--no-spawn`.** `run_plans`/`run_plan` therefore always
  auto-spawn the daemon, unlike `run_status`. Left as-is since adding the flag to those structs
  is a CLI-surface change outside "Plans data model."
