#![deny(clippy::unwrap_used, clippy::expect_used)]
//! repo-assist library surface.
//!
//! The `ra` binary (see [`crate::cli`]) is a thin shell over these modules so integration
//! tests can drive them directly.
//!
//! Spec (DoD #1, workorder 20261022-20): forbid unwrap/expect in non-test source so
//! spec-fidelity issues surface as compile errors (clippy `-D warnings`), never as
//! runtime panics.

#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod checks;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod doctor;
pub mod ipc;
pub mod paths;
pub mod plans;
pub mod sources;
pub mod store;
pub mod tui;
pub mod views;

/// Tool name and version, kept in one place.
pub const TOOL_NAME: &str = "ra";
/// Current crate version.
pub const VERSION: &str = "0.1.0";
