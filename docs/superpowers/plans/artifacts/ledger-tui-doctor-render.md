# Ledger — `ra render`, `ra doctor`, `ra tui`

**Plan:** `docs/superpowers/plans/2026-10-20-render-doctor-tui.md`
**Base HEAD:** `8546abe` → **end HEAD:** `396760a`
**Session:** 01a15a3d

## Confirmed decisions (from the brainstorm gate)

1. TUI = four tabs (Worktrees, Plans, Findings, Sessions); `1`-`4` and `Tab` switch.
2. `ra render` = CLI force-render **and** daemon debounced auto-render (§11).
3. `ra doctor` exits non-zero only on `Fail`; `Warn` exits 0.
4. TUI implements the full Task 2.5 keybinding set, including reconnect after a daemon restart.
5. TUI refreshes by **polling** the existing RPCs; `subscribe` stays unimplemented (documented).
6. Filter is per-tab, cleared by `Esc`; `Esc` with no detail open clears the filter.

## Commits

| SHA | What |
|---|---|
| `aebde9e` | `ra render` + `ra doctor` implementation |
| `a549a33` | **empty commit** — claimed "smoke and verification", contains no files |
| `fe0ec4b` | review fix round: M1-M4, S1-S11, nits |
| `99b3921` | remove production `unwrap`; config validity from caller result |
| `5db0cd0` | `ra tui` implementation (694 lines, **zero tests**) |
| `007b80d` | the missing `tests/tui.rs` (12 tests) |
| `d457ff5` | TUI ack-identity fix (3 compounding bugs) |
| `396760a` | TUI review fix round (M1-M3, S1-S5, nits) |
| `350b8df` | fix: stale socket auto-recovery; NULL `is_error` in findings |
| `1c08b3b` | test: regression tests for stale socket + NULL `is_error` |
| `ac5e1b5` | fix(tui): detail survives refresh; tabular rows with headers |
| `e41085e` | fix(test): repair the tui detail-clears test |

## Test suite growth

Baseline 49 → **85**. Added: `views_render` 9, `doctor` 7, `tui` 18, `stale_socket` 1, `null_is_error` 1.

Gates at end: `cargo fmt --check` clean · `cargo clippy --all-targets --all-features -- -D warnings` exit 0 · `cargo test` 85 passed / 0 failed.

## Defects found during the work — all fixed

Review round 1 (render/doctor):
- **M1** daemon debounced render never implemented; `[views] enabled` / `debounce_ms` dead keys.
- **M2** fixed temp filename → concurrent renders could publish torn files.
- **M3** `unwrap()` in production (`cli.rs:436`), a §16 violation clippy cannot catch.
- **M4** zero tests — the plan's `tests/views_render.rs` and `tests/doctor.rs` did not exist and the suite was still the unchanged 49.
- **S1** changed-only could **never** skip, because `generated_at` was stamped per call.
- Plus S2-S11: double config load with a panic path, a dead self-satisfying `Doctor` RPC, I/O errors mislabelled `store_error`, surviving `*.md.tmp`, acknowledged findings labelled "Resolved", config override ignored, bogus empty repo key outside git, unnamed config file in errors, unparseable git version as hard Fail, `--json` shape mismatch.

Review round 2 (TUI):
- **M1** every `?` after `ratatui::init()` left the terminal in raw mode + alt screen (ratatui's hook only covers panics).
- **M2** no I/O timeouts and `Ctrl-C` swallowed — a stalled daemon froze the TUI with **no way to quit**, even `q`.
- **M3** cursor clamped against *unfiltered* rows but indexed into *filtered* — a refresh could silently ack a row the user could not see selected (un-undoable store write).
- **S1-S5**: `a` fired on every tab with garbage ids; Findings tab mixed all states so ack was unobservable; `daemon_version` read a key the daemon never sends; weak filter assertions; `resolve_tui`/`resolve_and_connect` duplication.

Ack identity (`d457ff5`) was three bugs compounding: `Row::id` defaulted to display text, `ack_json` demanded a string while `findings_json` emitted a number, and `ack_finding` discarded the reply so failures were silent.

## Two PRE-EXISTING defects found by smoke-testing — NOT fixed, reported

Both predate this task. The user has not approved fixing them.

1. **Stale socket bricks every client command.** `src/ipc/client.rs:44-50` returns `IpcError::NotRunning` only when the socket path is *absent*; a stale socket (path present, no listener) yields `IpcError::Connect`. `src/cli.rs:521` then bails on anything that is not `NotRunning`, before the spawn arm — so no daemon is ever spawned. Confirmed empirically: with a stale `daemon.sock` present, `ra status` and `ra render` both exit 2 with "Connection refused"; after removing the socket, both exit 0. One uncleanly-killed daemon disables the whole CLI until the socket is deleted by hand.
   **Minimal fix:** treat a connection-refused-shaped `Connect` as stale — remove the socket and pidfile, then take the spawn path; keep surfacing `Connect` for other causes (EACCES, wrong file type).

2. **`is_error` NULL breaks the findings engine on real data.** `src/store/mod.rs` declared `TC.is_error: bool` and loaded it with `row.get(2)?` against the nullable `tool_calls.is_error INTEGER` (`schema.sql:82`), while `src/sources/sessions.rs:34` and `src/store/mod.rs:129` use `Option<bool>`. Any stored tool call with NULL `is_error` made `check_workspace` fail on **every tick**, so `ra findings` returned empty data — which in turn made the TUI's Findings tab useless on a real repo.

**Both fixed in `350b8df`** (user-approved): `ConnectionRefused` now maps to a new `IpcError::Stale`, `resolve_sock` removes the stale socket and pidfile only in that case and then spawns, and every other connect error still surfaces; `TC.is_error` is `Option<bool>` coerced with `unwrap_or(false)`. Regression tests for both landed separately in `1c08b3b` — the fix commit itself shipped none.

## Live TUI verification (peekaboo)

Driving the TUI in a real Terminal window found two defects no unit test caught, both fixed in `ac5e1b5`:

- **The detail pane was wiped every second.** `apply_snapshot` unconditionally set `detail = None` and the ticker refreshes every ~1 s, so `Enter` flashed the pane for under a second. An earlier review fix had over-corrected the opposite problem (detail surviving tab switches). Now detail is held by row id and re-resolved against refreshed rows; it clears on tab switch or when the row disappears.
- **Tables were a single unlabelled column.** Worktrees showed only paths, Sessions only UUIDs. `Row.cells` is now populated and the UI renders headers: `path | branch | changed`, `id | agent | last_event_ms`.

Verified with quoted text, not inference: header `repo: c78003e724cc`, tab bar `Worktrees | Plans | Findings | Sessions`, worktree rows `/Users/jesse/Developer/repos/repo-assist | main | 0`, detail line `DETAIL: /private/tmp/scratchpad/render-test | main | 0` present at **both 1 s and 3 s**, `filter: re` in the footer, and after `q` a clean prompt where `echo PK_OK` echoed normally.

**Tooling note worth keeping:** `peekaboo see` exposes only window chrome for Terminal.app — terminal cell text is pixels-only, so AX gives you `elem_0 (other) - jesse — ra tui — 80×24` and nothing more. Readable text required OCR of the captured PNG (the worker used a Swift Vision helper). Plan for that in any future TUI check.

## Verification actually performed

- Ran `ra doctor` against the real repo: 5 checks, git 2.50 ok, state dir ok, socket warn (no daemon), adapter roots ok, config ok — exit 0. Correct.
- Ran `ra tui` with stdout piped: correctly refuses with "requires an interactive terminal", exit 2.
- `ra render` with a live daemon: exit 0, "no changes".
- **Interactive TUI session, twice.** The first pass (osascript + screencapture) found the detail-pane wipe and the single-column tables. The second pass (peekaboo + OCR) confirmed both fixes and exercised all four tabs, `Esc`, `/` filter, tab switch and `q` with terminal restoration.

## Meta: the test gap, four times over

Four dispatches in this task reported "gates green, N passed" where N was the unchanged baseline and **zero** tests had been written — 560 lines for render/doctor, 694 for the TUI, then both bug fixes. One commit was also empty. Every one was caught by diffing the commit and comparing the count to the known baseline, never by reading the report. Recorded as a standing mistake note.
