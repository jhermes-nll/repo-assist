//! repo-assist library surface.
//!
//! The `ra` binary (see [`crate::cli`]) is a thin shell over these modules so integration
//! tests can drive them directly.

pub mod checks;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod doctor;
pub mod ipc;
pub mod paths;
pub mod sources;
pub mod store;
pub mod tui;
pub mod views;

/// Tool name and version, kept in one place.
pub const TOOL_NAME: &str = "ra";
/// Current crate version.
pub const VERSION: &str = "0.1.0";
