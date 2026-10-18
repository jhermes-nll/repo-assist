//! Acceptance for Task 0.1: CLI skeleton.
//!
//! Verifies that `ra --version` and `ra --help` work, that every documented subcommand is
//! *recognized*, and that unrecognized subcommands fail clearly.

use assert_cmd::Command;

/// `ra --version` prints the tool name and version and exits 0.
#[test]
fn version_prints_tool_and_version() {
    let mut cmd = Command::cargo_bin("ra").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("ra"))
        .stdout(predicates::str::contains("0.1.0"));
}

/// `ra --help` lists every documented subcommand.
#[test]
fn help_lists_every_documented_subcommand() {
    let mut cmd = Command::cargo_bin("ra").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("daemon"))
        .stdout(predicates::str::contains("status"))
        .stdout(predicates::str::contains("worktrees"))
        .stdout(predicates::str::contains("plans"))
        .stdout(predicates::str::contains("plan"))
        .stdout(predicates::str::contains("drift"))
        .stdout(predicates::str::contains("conflicts"))
        .stdout(predicates::str::contains("who-touches"))
        .stdout(predicates::str::contains("findings"))
        .stdout(predicates::str::contains("ack"))
        .stdout(predicates::str::contains("sessions"))
        .stdout(predicates::str::contains("tui"))
        .stdout(predicates::str::contains("doctor"))
        .stdout(predicates::str::contains("render"));
}

/// Every recognized subcommand exits with code 2 (not implemented in phase 0.1) and a
/// stable "not yet implemented (scheduled for Task N)" message naming that subcommand.
#[test]
fn every_subcommand_is_recognized_and_reports_not_implemented() {
    // (subcommand, extra positional args needed to pass clap parsing).
    let cases: &[(&str, &[&str])] = &[
        ("daemon", &[]),
        ("status", &[]),
        ("worktrees", &[]),
        ("plans", &[]),
        ("plan", &["<id>"]),
        ("drift", &[]),
        ("conflicts", &[]),
        ("who-touches", &["/some/path"]),
        ("findings", &[]),
        ("ack", &["<id>"]),
        ("sessions", &[]),
        ("tui", &[]),
        ("doctor", &[]),
        ("render", &[]),
    ];
    for (sub, args) in cases {
        let mut cmd = Command::cargo_bin("ra").unwrap();
        cmd.arg(sub);
        for a in *args {
            cmd.arg(a);
        }
        cmd.assert()
            .failure()
            .code(2)
            .stderr(predicates::str::contains("not yet implemented"))
            .stderr(predicates::str::contains(*sub));
    }
}

/// Unknown subcommands fail cleanly (clap error), never panic.
#[test]
fn unknown_subcommand_fails_cleanly() {
    let mut cmd = Command::cargo_bin("ra").unwrap();
    cmd.arg("bogus-subcommand");
    cmd.assert().failure();
}
