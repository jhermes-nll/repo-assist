# Ledger — status refresh + smoke run

**Session:** 01a15a3d-5899-7075-a365-f7ee3262c642
**Plan:** `docs/superpowers/plans/2026-10-19-status-refresh-and-smoke-run.md`
**Started:** 2026-10-19

## Baseline established before dispatch

| Repo | HEAD | Tree | Tests | fmt | clippy |
|---|---|---|---|---|---|
| `repo-assist` | `8e04e4f` | clean, at origin/main | 48 passed / 0 failed | clean | exit 0 |
| `ai-dashboard` | `b26139e` | clean, at origin/main | 21 passed / 0 failed | n/a | **RED — 7 cosmetic lints** |

`ai-dashboard` also had no `node_modules/`, so `npm run build` could not run.

## In flight

| Agent | Repo | Scope | Status |
|---|---|---|---|
| `ra-status-smoke` (404fd167) | repo-assist | rewrite `status.md`; smoke-run every implemented `ra` subcommand | running |
| `dash-status-launch` (4fd6eb4d) | ai-dashboard | `npm install`, build, rewrite `STATUS.md`; launch app + mock mode, screenshot | running |

Disjoint repos, no shared state — safe to run concurrently.

## Known hazards handed to the workers

- Leaked `ra daemon --foreground` (pid 19349) from an earlier test run; `ra-status-smoke` is tasked with killing it. Hard rule in this repo: no stray daemons at the end.
- `repo-assist/tests/cli.rs` is fragile to its launch environment: 4/4 failures under the sandboxed exec shell ("daemon did not create its socket within 5s"), 4/4 passes under a normal shell. Environment artifact of detached-process spawning, not a code defect — but it means a sandboxed CI runner would fail this suite.
- `dash-status-launch` is forbidden from fixing the clippy lints; the doc must report that gate red.

## Out of scope (recorded so it is not mistaken for done)

- repo-assist `ra tui`, `ra doctor`, `ra render` — still unimplemented stubs.
- ai-dashboard Task 8 manual launch checklist as a formal signed-off item.
- The 7 ai-dashboard clippy lints.
- Any commit or push.

## Outcome

### repo-assist — done, verified

- `status.md` rewritten to match HEAD `8e04e4f` (11/14 commands implemented, findings engine + 9 finding kinds, store methods, RPC surface, suite table, commit table).
- Smoke run of the real binary against a scratch repo with 3 worktrees: `status`, `worktrees`, `plans`/`plan`, `drift`, `conflicts`, `who-touches`, `findings`/`--open`/`--all`, `ack`, `sessions` all ran; `tui`/`doctor`/`render` correctly stub with exit 2.
- **Bug found and fixed (user-approved):** `ra findings --kind <K>` crashed with `transport error: socket closed`. Two causes: `src/store/mod.rs:695` bound `?3` with one parameter; `src/ipc/server.rs` `Findings`/`Ack` handlers propagated store errors out of `handle_connection` via `?`, dropping the socket instead of replying. Fixed to `?1` plus a structured `store_error` reply. Regression test `tests/findings_kind.rs` drives the real daemon over IPC.
- Gates re-run by me after the fix: `fmt` clean, `clippy -D warnings` exit 0, `cargo test` **49 passed / 0 failed**.
- No stray `ra daemon` processes (the leaked pid 19349 from the earlier session is gone).

### ai-dashboard — builds and launches, but renders no data

- `npm install --registry https://registry.npmjs.org/` → 56 packages; `npm run build` → exit 0.
- `cargo test` → 21 passed / 0 failed. `cargo clippy -D warnings` → **RED, 7 cosmetic lints** (left unfixed by scope; recorded in `STATUS.md`).
- `STATUS.md` §6 rewritten honestly, including the red clippy gate.
- App launches: `npm run tauri dev`, Vite on 5173, two windows present, clean shutdown, no stray processes.
- **Bug found by running it (@vision on the screenshots):** the compact overlay rendered only `…watching…`. The first worker's report ("two windows, titles verified") was true but hid that they were empty.
- **Root cause (fixed, uncommitted):** `App.svelte` passes `snapshot={$snapshot}`, but `Compact.svelte` and `Expanded.svelte` destructured `const { s: snap } = $props()` — a prop named `s` that the parent never passes, so `snap` was permanently `undefined` and the `{#if !snap}` branch rendered forever. Plain `const` also made the values non-reactive. Fix: destructure `snapshot`, wrap the four derived values in `$derived`. The event pipeline, store, emit loop and event name were all correct.
- **Verified at runtime:** mock mode renders 3 compact rows and a 6-row main-window table; real mode ingests live `pi` data (`PRAGMA user_version` 1; 2 `pi` sessions / 172 `pi` events vs 0 before) and the compact overlay shows a live `pi · deepseek-v4.1-flash:1017 · 9s ago` row while a pi process is running, returning to its empty state afterwards.
- Gates: `npm run build` exit 0; `cargo test` **21 passed / 0 failed**; `cargo clippy -D warnings` still **RED (7 cosmetic lints)**, left unfixed by scope and recorded in `STATUS.md`.
- App (pid 76409) and vite (pid 76204) intentionally left running for inspection.

### Newly found by looking at the pixels (not fixed, not approved)

- The main window header renders `{sessions.length} live · {live.length} active`, so with 6 total sessions and 0 live it displays **"6 live · 0 active"** — it labels the total as "live". `Expanded.svelte`.
- Every row shows a secondary `ended` label next to its state, producing combinations like `BUSY ended`. `SessionRow.svelte`.
- The `tokens` column header did not appear in the capture; may be clipped at the panel edge.


### Environment gotcha (confirmed)

`npm config get registry` on this Mac returns `https://npm.apple.com`. Install with an explicit `--registry https://registry.npmjs.org/`. Stored in memory.

## Correction to an earlier worker claim

The first ai-dashboard worker reported "two windows, titles verified" and marked rendering "unverified visually". The window titles were real (System Events), but the windows are **empty**. Vision inspection is what caught it. Lesson: a window existing is not evidence that it renders data.

