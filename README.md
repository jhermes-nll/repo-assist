# repo-assist (`ra`)

Local, advisory, always-on daemon that watches a git repository across all of its worktrees and surfaces agent/plan conflicts, drift, and stalls. Read-only: it never writes inside any work tree.

## Install

```bash
cargo build --release
# Add to PATH, e.g.:
export PATH="$PWD/target/release:$PATH"
ra --version
ra --help
```

rustc 1.85+ (edition 2021) is required.

## Quick start

```bash
cd /path/to/your/git/repo
ra doctor            # environment checks
ra status            # auto-spawns the detached daemon on first use
ra render            # force-render markdown views
```

The daemon auto-spawns on the first client command (unless `--no-spawn` is given) and binds one Unix socket per repository. `ra tui` opens the four-tab dashboard.

## Data and config

Config is layered: built-in defaults < `~/.config/repo-assist/config.toml` < `<repo_root>/.repo-assist.toml`. Unknown keys are an error.

```toml
base_branch = "auto"            # auto = origin/HEAD, else main, else master
ignore_paths = ["Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "**/*.snap"]

[thresholds]
abandoned_hours = 48
stalled_hours = 4
loop_window = 20
loop_repeats = 5
session_token_budget = 0        # 0 = disabled
session_quiet_secs = 120

[sources.git]
poll_secs = 5                   # backstop; fs events drive most updates
[sources.plans]
superpowers_globs = ["docs/superpowers/plans/*.md"]
checklist_globs   = ["TODO.md", "PLAN.md", "docs/plans/**/*.md"]
[sources.pi]
enabled = true
sessions_dir = ""               # empty = auto (see below)
[views]
enabled = true
debounce_ms = 1000
```

**Specs.** Files under `docs/**/*.md` are tracked in the same plans list with `source = "spec"` (visible in JSON; `ra plans` has no source column). Parsing and the `done`/`active` rule are identical to plans. A spec with no parseable tasks or checkboxes is listed with state `unknown` ("present but not machine-readable"), and `unknown` specs are never counted active or stalled and never fire findings. Files already claimed by a plan adapter, and anything under `artifacts/`, are not indexed as specs. Specs are always scanned — there is no config key for them (the two existing plan glob keys are also not yet consumed by the daemon).

pi session root resolution: `$PI_CODING_AGENT_SESSION_DIR` if set; else `${PI_CODING_AGENT_DIR:-~/.pi/agent}/sessions`; `sources.pi.sessions_dir` overrides when non-empty.

## Operational commands

| Command | Purpose |
|---|---|
| `ra doctor` | git version, state dir writable, socket reachable, adapter roots, config validity |
| `ra status` / `ra status --json` | one-screen summary |
| `ra worktrees` | branches, ahead/behind, changed-file counts |
| `ra plans` / `ra plan <id>` | plan and task listing |
| `ra drift` | unignored changed files no task expects |
| `ra conflicts` | worktree + plan overlaps, merge conflicts |
| `ra who-touches <path>` | who touches a path |
| `ra findings` / `ra ack <id>` | findings lifecycle |
| `ra sessions` | pi session telemetry |
| `ra render` | force-render `views/STATUS.md`, `PLANS.md`, `FINDINGS.md` |
| `ra tui` | four-tab dashboard |
| `ra daemon` | run the daemon in the foreground |

`ra doctor` checks: git version (requires ≥ 2.38 for `git merge-tree --write-tree`, used by `worktree.merge_conflict`), state directory writable, socket reachable (warns if no daemon), adapter roots readable, config validity. A git older than 2.38 disables merge-conflict detection but everything else works.

## Performance

Measured on a Linux aarch64 host (OrbStack, 24 CPUs, rustc 1.99.1), release build, 5 worktrees, idle daemon, sample window 10s. Reproduce with `scripts/perf.sh`.

| Metric | Value | Target |
|---|---|---|
| Idle daemon CPU | 0.15% | < 1% |
| Idle daemon RSS (5 worktrees) | 12.09 MB | < 50 MB |
| `ra status` p95 (warm, n=20) | 4.05 ms | < 100 ms |

Retention: `events` table capped to the newest `EVENT_CAP` rows (1000); `tool_calls` per session kept as a ring of `TOOL_CALL_RING_SIZE` (256) newest calls.

## How to add a plan source

1. Add a parser/adapter under `src/plans/` implementing the `PlanRow`/`TaskRow` upsert flow (see `src/plans/superpowers.rs`, `src/plans/checklist.rs`).
2. Register its globs in `[sources.plans]` (`superpowers_globs`, `checklist_globs`) or via config.
3. The daemon rescans plans on the configured tick; `ra plans` / `ra plan <id>` expose the results. Tests use `tests/plans_parse.rs` and `tests/fixtures/plans/`.

## How to add an agent adapter

1. Add a module under `src/sources/` (see `src/sources/sessions.rs` — `ingest_file`, `sync_sessions`, and `Store::upsert_session_with_calls` for the pi reference).
2. Implement read-only ingest into `sessions`/`tool_calls` via `Store::upsert_session_with_calls`, mapping cwd -> worktree.
3. Add config under `[sources.<adapter>]` and tick in the daemon if needed. The daemon never writes inside the work tree, including session files.

## Zero-writes guarantee (tests)

- The daemon and all tests only read repository files; writes go to the state directory (temp dirs in tests via `XDG_STATE_HOME` / `XDG_RUNTIME_DIR`).
- `cargo test` creates temp repos under the OS temp dir; no source-tree or worktree file is modified. Retention caps prevent unbounded growth.
