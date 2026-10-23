//! Plan discovery, linking, and observed derivation (Section 8.1, Task 3.2).
//!
//! Plans are discovered per worktree by scanning the configurable plan-source globs,
//! linked to the worktree that owns them, and each task's `observed` state is derived from
//! git state alone — independently of any plan-file parser.
//!
//! - [`derive`]: recompute a task's `observed` (Section 9 truth table) + one `plans.id`.
//! - [`discover`]: scan globs per worktree, upsert plans/tasks, mark vanished plans `gone`.

pub mod derive;

pub use derive::{derive, plan_id, run_truth_table, DeriveCase, Observed};
