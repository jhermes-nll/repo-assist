//! `ra` — repo-assist binary entry point.
//!
//! Phase 0.1 only wires up the CLI surface: argument parsing, `--version`/`--help`, and a
//! dispatcher that recognizes every documented subcommand but runs only those implemented in
//! the current phase. See [`cli::run`].

mod cli;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    match cli::run(cli) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    }
}
