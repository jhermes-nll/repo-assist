# Plan — `ra render`, `ra doctor`, `ra tui`

**Date:** 2026-10-20
**Repo:** `/Users/jesse/Developer/repos/repo-assist` · base HEAD `8546abe`
**Spec:** `docs/2026-10-19-repo-assist-implementation-plan.md` §3, §4, §6, §7, §9, §11, §15 (Task 2.5, Task 5.1), §16
**Authority on what exists:** `status.md` (rewritten and committed today, accurate at `8546abe`)

## Starting state (verified)

- All gates green: `fmt` clean · `clippy --all-targets --all-features -- -D warnings` exit 0 · `cargo test` **49 passed / 0 failed**.
- `src/cli.rs` marks `Tui`, `Doctor`, `Render` as `implemented: false`; the stub printer at `src/cli.rs:635-640` exits 2.
- `src/ipc/server.rs` falls through to `not_implemented` for `Subscribe` and `RenderViews`.
- `src/paths.rs` `Env::state_files(key)` yields db/pid/socket paths. **No `views/` directory is created anywhere yet.**
- `ratatui`, `crossterm`, `insta` are **not** in `Cargo.toml`.

## User-confirmed decisions (settled — do not re-litigate)

1. **TUI = four tabs**: Worktrees, Plans, Findings, Sessions. `1`-`4` and `Tab` switch.
2. **`ra render` = CLI force-render AND daemon debounced auto-render** per §11.
3. **`ra doctor` exits non-zero when any check fails**, 0 when all pass.
4. **TUI implements the full Task 2.5 keybinding set**, including reconnect-without-crash after a daemon restart.

## Dependency decision

§3 authorizes exactly `ratatui`, `crossterm`, and dev `insta`. Pin:

```toml
ratatui   = "0.29"
crossterm = "0.28"
[dev-dependencies]
insta     = "1"
```

**Why 0.29 and not the latest 0.30.2:** ratatui 0.30 split `CrosstermBackend` into a separate `ratatui-crossterm` crate, which §3 does not authorize. 0.29 still bundles the backend and provides `ratatui::init()` / `ratatui::restore()` with the panic hook. If 0.29 fails to resolve or compile, **stop and report** rather than silently adding `ratatui-crossterm`.

Snapshot tests use `ratatui::backend::TestBackend` plus `assert_buffer_lines([...])`. Hand-written expected strings rather than `insta`'s file-based snapshots where the screen is small — `insta` is used only where a full-screen dump is genuinely clearer. §16 requires no `unwrap()` outside tests.

---

## Task 1 — `ra render` (views module, RPC, CLI, daemon hook)

### Files

- **Create `src/views/mod.rs`** — rendering is pure: takes data, returns `String`.
  - `pub const VIEW_CAP_LINES: usize = 150;`
  - `pub struct ViewData { pub status: …, pub plans: …, pub findings: …, pub sessions: … }` (reuse the existing JSON builders' row types; do not invent parallel structs).
  - `pub fn render_status(d: &ViewData, now_ms: i64) -> String`
  - `pub fn render_plans(d: &ViewData, now_ms: i64) -> String`
  - `pub fn render_findings(d: &ViewData, now_ms: i64) -> String`
  - `fn header(out: &mut String, meta: &ViewMeta)` — writes the three mandated first lines per §11: `generated_at: <iso8601>`, `daemon: <version>`, and the read-only/may-be-stale note.
  - `fn cap(lines: &mut Vec<String>)` — enforce `VIEW_CAP_LINES`, **open findings first** for the findings view.
- **Create `src/views/write.rs`** — `pub fn write_views(dir: &Path, rendered: &[(&str, String)]) -> Result<Vec<PathBuf>, ViewError>`:
  - `create_dir_all(dir)`.
  - Atomic per §11: write `<name>.md.tmp` **in the same directory**, fsync, then `rename` onto `<name>.md`.
  - **Changed-only:** skip the write when the existing file's bytes already match.
  - Returns the paths actually written.
- **Modify `src/paths.rs`** — add `views_dir(&self, repo_key: &str) -> PathBuf` = `state_dir(key).join("views")`.
- **Modify `src/ipc/server.rs`** — replace the `RenderViews` fall-through with a handler that builds `ViewData` from the store (reuse `status_json`/`plans_json`/`findings_json` internals or their underlying queries), renders, writes, and returns `{"written": [...]}`. Follow the error-reply pattern already used by `Findings`/`Ack` (`store_error` reply, never dropping the socket).
- **Modify `src/daemon/mod.rs`** — after a tick that changed anything, call the render path debounced. Debounce state lives in the daemon task; do **not** block the tick. A render failure logs via `tracing` and never kills the loop.
- **Modify `src/cli.rs`** — flip `Render` to `implemented: true`; add `run_render` mirroring `run_sessions`; human output lists the files written.

### Tests — `tests/views_render.rs`

- renders all three views and the header lines are present and first
- the findings view lists open findings before resolved ones
- a view is capped at 150 lines and the cap truncation is visible//tested
- second render with unchanged data writes **nothing** (changed-only)
- changed data rewrites atomically — asserted by checking no `*.md.tmp` remains and the final content is complete
- `views/` is created on demand when absent
- **no write inside a worktree** (§16): snapshot the worktree tree + `git status` before and after a render and assert identical
- RPC `RenderViews` over a real daemon returns the written paths

### Verify

`cargo fmt --check` · `cargo clippy --all-targets --all-features -- -D warnings` · `cargo test` (49 + new) · manual: `ra render` in a scratch repo, then `ls $XDG_STATE_HOME/repo-assist/<key>/views/` and `head -3` each file.

---

## Task 2 — `ra doctor` (environment checks, CLI, RPC)

### Files

- **Create `src/doctor/mod.rs`**
  - `pub enum CheckStatus { Ok, Warn, Fail }`
  - `pub struct Check { pub name: &'static str, pub status: CheckStatus, pub detail: String }`
  - `pub fn run_all(env: &Env, repo_path: &Path, cfg: &Config) -> Vec<Check>`
  - Checks, each returning its own `Check`:
    1. **git version** — parse `git --version`; `Ok` at ≥ 2.38, `Warn` below with the explicit reason *"`worktree.merge_conflict` is disabled: needs git ≥ 2.38 for `git merge-tree --write-tree`"*. This satisfies both §Task 5.1 and the §9 requirement that doctor explain a disabled check.
    2. **state directory writable** — create-and-remove a probe file in `state_dir`.
    3. **socket reachable** — attempt `Client::connect` + `Ping`; `Warn` (not `Fail`) when no daemon is running, since that is normal before first use.
    4. **adapter roots** — pi sessions root resolves and is readable; `Warn` when absent (user may have no pi sessions).
    5. **config validity** — `Config::load` result; report the offending file and key on error.
- **Modify `src/ipc/server.rs`** — add a `Doctor` RPC handler returning the checks as JSON.
- **Modify `src/cli.rs`** — flip `Doctor` to `implemented: true`; `run_doctor` prints a compact aligned table and **returns exit 1 if any check is `Fail`**, 0 otherwise (`Warn` does not fail). `--json` supported like every other read command.

### Tests — `tests/doctor.rs`

- all-ok environment yields exit 0 (build a scratch repo with a live daemon)
- a failing check yields exit 1 — force failure by pointing `XDG_STATE_HOME` at an unwritable path
- old-git path emits the `merge_conflict` disabled explanation (inject the version string through a seam rather than requiring an old git — make the version-parse function pure and table-test it)
- `--json` shape: array of `{name, status, detail}`
- no `unwrap()` added outside tests (reviewer check, not a test)

### Verify

Same three gates, plus manual `ra doctor; echo exit=$?` in both a healthy scratch repo and with `XDG_STATE_HOME` pointed at a read-only directory.

---

## Task 3 — `ra tui` (four tabs, full keybindings, reconnect)

### Files

- **Create `src/tui/mod.rs`** — terminal lifecycle: `ratatui::init()`, `ratatui::restore()`, event loop, panic-safe restore (the panic hook in `init()` handles it; do not re-implement).
- **Create `src/tui/app.rs`** — pure state, no I/O, so it is unit-testable:
  - `pub enum Tab { Worktrees, Plans, Findings, Sessions }`
  - `pub struct App { tab: Tab, cursor: [usize; 4], filter: [String; 4], rows: [Vec<Row>; 4], detail: Option<Detail>, status_line: String, should_quit: bool }`
  - `pub enum Action { Quit, NextTab, PrevTab, SelectTab(Tab), Down, Up, Enter, Ack, Filter(char), Backspace, ClearFilter, Refresh }`
  - `pub fn on_key(&mut self, KeyEvent) -> Option<Action>` — the full Task 2.5 set: `j`/`k`, `Enter`, `a`, `Tab` and `1`-`4`, `/`, `Esc` (dismiss detail / clear filter), `r`, `q`.
- **Create `src/tui/ui.rs`** — `pub fn draw(frame: &mut Frame, app: &App)`; header line carries repo key, daemon version and counts per Task 2.5; a tab bar; the active tab's table via `Table`/`List`; a footer with the key hints.
- **Create `src/tui/client.rs`** — data access + **reconnect**: `pub struct DataSource { sock: PathBuf }` with `pub fn fetch_all(&mut self) -> Result<Snapshot4, TuiError>` that reconnects on failure and sets a status-line message instead of propagating, so the TUI never crashes when the daemon dies. Refresh on a ticker (see ambiguity 1).
- **Modify `src/cli.rs`** — flip `Tui` to `implemented: true`; `run_tui` refuses to start when stdout is not a TTY with a clear message rather than emitting escape codes into a pipe.

`src/lib.rs` gains `pub mod tui;` and `pub mod views;` / `pub mod doctor;` from Tasks 1 and 2.

### Tests — `tests/tui.rs`

`TestBackend` snapshot/assertion tests, one per tab, plus behaviour:

- each of the four tabs renders its expected header + rows (assert with `assert_buffer_lines`)
- `1`-`4` and `Tab` change the rendered tab
- `j`/`k` move the cursor and the cursor stops at both ends
- `Enter` opens detail; `Esc` returns to the list
- `a` on a finding issues an ack and the row's state changes after refresh (drive against a real daemon, or assert the `Action::Ack` produced the right request — say which)
- `/` filters the current tab and `Esc` clears it; filter persists across a refresh (ambiguity 2)
- **reconnect:** start the TUI data source against a live daemon, kill the daemon, assert `fetch_all` returns an error that is surfaced as a status line and the app does not panic; restart the daemon and assert the next `fetch_all` succeeds
- `q` sets `should_quit`

Note `app.rs` is pure, so most of the behaviour list is testable without a terminal; only the render assertions need `TestBackend`.

### Verify

Same three gates, plus manual: `ra tui` in a scratch repo with a live daemon — switch all four tabs, ack a finding, filter, then `kill` the daemon and confirm the TUI shows a status line and recovers when `ra status` respawns it.

---

## Ambiguities and recommendations (decided, flagged for review)

1. **TUI data refresh: poll or `subscribe`?** §6 defines a `subscribe` streaming method but it is a whole protocol addition. **Recommendation: poll the five existing RPCs on a ~1 s ticker**, and leave `subscribe` unimplemented (it is already documented as absent in `status.md`). Reconnect logic is required either way and is simpler to prove with polling. If you want streaming, that is a separate task.
2. **`/` filter scope.** **Recommendation: per-tab**, each tab remembers its own filter, cleared by `Esc` only — so a refresh or tab switch never silently loses it.
3. **Detail dismissal.** **Recommendation: `Esc`.** `Enter` opens, `Esc` backs out; `Esc` with no detail open clears the filter.
4. **`ra render` output when nothing changed.** **Recommendation: succeed with an empty written list and exit 0**, printing "no changes". Non-zero would break daemon callers.
5. **`ra doctor` when no daemon is running.** **Recommendation: `Warn`, exit 0** — absence is the normal pre-first-use state, not a failure.

## Out of scope

`subscribe`/`RenderViews` streaming beyond the force-render RPC; `sysinfo` process metrics; README and `docs/agents-snippet.md` from Task 5.1's file list (that is Task 5.1's documentation half and deserves its own pass); the 7 ai-dashboard clippy lints (different repo).

## Evidence ledger

See `docs/superpowers/plans/artifacts/ledger-tui-doctor-render.md`.
