# Finding: daemon-restart integration tests flake on a fixed sleep

**Status:** recorded only — NOT fixed. Fix is a separate follow-up task.

## Affected tests

| Test | Location | Failing assertion |
|---|---|---|
| `subscribe_rpc_reconnect_kill_restart` | `tests/subscribe_rpc.rs:228` | `fetch after restart should succeed` |
| `tui_reconnect_survives_daemon_restart` | `tests/tui.rs:300` | `fetch after restart should succeed` (identical message) |

## Shared mechanism (root cause)

Both tests follow the same sequence after restarting the daemon:

1. kill the daemon, spawn a new one;
2. `wait_socket()` — polls only until the **socket file exists** on disk;
3. fixed `sleep(300ms)`;
4. assert `ds.fetch_all().is_ok(), "fetch after restart should succeed"`.

A socket file existing only means the daemon bound the path — not that it is ready to
serve fetches. Under full-gate load the 300 ms grace period can expire before the
daemon is ready, and the assert fires. Race lives in both test bodies (the fixed
sleep), not in `src/`.

## Observed rate & pre-existing evidence

- Post-merge-1 gate at `f1d9f04`: 1 failure (`subscribe_rpc_reconnect_kill_restart`),
  then 3/3 full-gate runs and 5/5 standalone runs green.
- **Pre-merge base `4f67dad`, 1 of 3 full-gate runs failed** in the *sibling* test
  `tui_reconnect_survives_daemon_restart` with the identical assertion — proves the
  race predates any 2026-11-12 merges; it is not introduced by merge `f1d9f04`.
- Overall roughly 1 failure per 4–6 full-gate runs under load.
- Stray-daemon contamination was checked and disproven (`pgrep -x ra` → 0).

## Suggested fix (do not implement here)

In **both** tests, replace the fixed `sleep(300ms)` after restart with a bounded
retry/poll: retry `fetch_all()` with a deadline until it succeeds (or fail with the
last error after the deadline). Same shape as the existing `wait_socket()` loop, but
waiting for *service readiness* rather than *socket-file existence*.
