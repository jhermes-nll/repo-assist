//! Data sources feeding the daemon (Section 4): git worktrees (Task 2.1), the filesystem
//! watcher (Task 2.2), plans (Section 8.1), and agent sessions (Section 8.2). Sources emit
//! events onto the bus and never write findings.

pub mod fswatch;
pub mod git;
pub mod sessions;
