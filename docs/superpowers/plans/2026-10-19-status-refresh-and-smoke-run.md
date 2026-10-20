# Status refresh + smoke run — repo-assist & ai-dashboard

**Date:** 2026-10-19
**Trigger:** user directive — "update status. try running them!"
**Classification:** bounded ops + doc update. No architecture change.

## Problem

Both repos' status documents describe a state older than their code:

| Repo | Doc claims | Reality (verified this session) |
|---|---|---|
| `repo-assist` | findings/conflicts/who-touches unimplemented; HEAD `a28b553` | 11/14 commands implemented incl. findings engine; HEAD `8e04e4f`; 48 tests green |
| `ai-dashboard` | "Scaffold committed only. No collector logic yet." | Tasks 1–7 of 8 merged; 21 tests green |

Neither project has been *executed* end to end. Every claim so far is test-suite evidence, not run evidence.

## Scope

**In scope**
1. Rewrite `repo-assist/status.md` to the verified current state (commands, findings kinds, test counts, HEAD).
2. Rewrite `ai-dashboard/STATUS.md` §6 to the verified current state (Tasks 1–7, actual test counts, known limitations, real gate status).
3. Install frontend deps and actually run both projects; capture real output.

**Out of scope (explicitly)**
- No new features, no Task 5 leftovers in repo-assist, no Task 8 launch checklist rewrite.
- No fixing of the 7 ai-dashboard clippy lints unless separately approved — the status doc must *report* that gate as red, not hide it.
- No commits or pushes.

## Tasks

### T1 — repo-assist status.md (worker A)
- Rewrite to the verified state. Every number must come from a command run in this session.
- Then smoke-run the `ra` binary for real: build, `ra --help`, and exercise each implemented subcommand against a scratch git repo with two worktrees, plus `ra sessions` against the real pi session root.
- Capture verbatim stdout/stderr and exit codes per command. Report any command that errors, hangs, or returns nonsense.

**Verify:** every command in the doc re-runnable; the doc's test counts match a fresh `cargo test`; smoke output attached.

### T2 — ai-dashboard STATUS.md + run (worker B)
- `npm install` (node 26.12 / npm 11.20 available; none present yet).
- `npm run build` then `cargo test --manifest-path src-tauri/Cargo.toml`.
- Update `STATUS.md` §6 with verified state, real test counts, real gate results, and known limitations.
- Launch the app: `npm run tauri dev` in background; wait for the dev server on 5173 and for the app process; `screencapture` the desktop.

**Verify:** `npm run build` exit code; `cargo test` counts; the app process is alive; a screenshot exists showing the windows (or a truthful "could not launch because X").

### T3 — fix the `ra findings --kind` crash (user-approved during T1)

The T1 smoke run found a real crash, verified against source:

- `src/store/mod.rs:695` builds `AND kind = ?3` while `:698` binds a single parameter. rusqlite numbers parameters by index, so `?3` requires three bound values — the query fails at runtime whenever `--kind` is supplied.
- `src/ipc/server.rs:260-261` (and `:265-266` for `Ack`) do `block_in_place(...).map_err(...)?`. The `?` tears down the connection without ever sending an error reply, so the client reports `transport error: socket closed` instead of the real store error. Every store failure in those handlers is misreported as a transport fault.

**Fix**
1. `?3` → `?1` in `list_findings`.
2. In the IPC handlers, convert a store error into a structured error reply (`{"id","error":{"code","message"}}`) instead of dropping the connection. Keep the change inside the handlers that already use `block_in_place`; do not restructure the accept loop.
3. Regression test: exercise `ra findings --kind <K>` end to end over the real IPC path — a finding of the requested kind is returned, a finding of another kind is not, and the command exits 0 rather than closing the socket. Follow the repo's test convention: `tests/` only, zero inline `#[test]` in `src/`, `mod common`.

**Not in scope:** the default-state semantics on `:694` (`if filter.all {} else if filter.any {...}` applies no state filter when neither flag is passed, so bare `ra findings` shows resolved rows too). The user did not approve changing that behaviour; record it as a known limitation in `status.md` instead.

**Verify:** `cargo fmt --check` · `cargo clippy --all-targets --all-features -- -D warnings` · `cargo test` (48 existing tests must stay green, plus the new ones), and a manual `ra findings --kind` run showing exit 0 with filtered output.

## Evidence rules

- Paste real output. Never assert a run succeeded without the exit code.
- If a step cannot run in this environment (GUI session, sandbox), say so in one line and continue with what is runnable.
- Status docs must be true at the moment of writing, including red gates.

## Risks

- `vite@8` / `@tauri-apps/cli@2` may not resolve under npm 11 + node 26 → `npm install` fails. Report, do not paper over.
- `tauri dev` compiles the whole Tauri tree; first build is slow (several minutes) and may exceed a short timeout.
- A GUI launch from a subagent shell may not attach to the window server; that is a legitimate "unverified" outcome.
