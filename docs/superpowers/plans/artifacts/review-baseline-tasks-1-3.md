# Review — baseline repair (commits 002b59c, 950d996, 292c026)

**Reviewer:** @reviewer (GLM-5.3-Flash) · **Date:** 2026-10-19
**Scope:** task-scoped gate on the three baseline-repair commits. Base `962b980` → HEAD `292c026`.
**Verdict: APPROVED — zero MUST-FIX.**

## Independently verified by the orchestrator (not taken on trust)

| Check | Result |
|---|---|
| `git log --oneline -4` | 292c026, 950d996, 002b59c, 962b980 — all present |
| working tree | clean apart from untracked `docs/superpowers/` |
| `cargo fmt --check` | exit 0, clean |
| `cargo clippy --all-targets -- -D warnings` | exit 0 |
| `cargo test` | **31 passed / 0 failed**, exit 0 |

## Reviewer's independent verification

- Confirmed the lock primitive by reading fs2 0.4's **vendored** `unix.rs`: `try_lock_exclusive` is `flock(fd, LOCK_EX|LOCK_NB)`. So flock is released by the kernel on holder death → **acquiring the flock IS the liveness authority**, and deleting `is_alive` loses nothing.
- Traced `fs::write`-vs-flock interaction: `fs::write` opens the same path (no rename/unlink → same inode) with `O_TRUNC` on a separate fd; flock is bound to the open file description and is unaffected. Corroborated empirically (cli test green 4×).
- Traced the guard (`lock.rs:56-65`): garbage/empty pidfile → `held = 0` → `Flock`; pidfile `"0"` → excluded; `held == own pid` → `Flock`, reachable only via same-process double-acquire, which is unreachable today (one call site, `daemon/mod.rs:62`) and is the right outcome anyway.
- Traced `Busy` → user string end-to-end: `lock.rs:27` Display → `daemon/mod.rs:62-63` → `cli.rs:488` → `main.rs` stderr; test asserts on it. Green.
- Confirmed the two real bugs the reorder fixes: on macOS `is_alive` was **always false** → contender clobbered the pidfile and leaked the raw error; on Linux a stale pidfile naming a recycled-live pid caused **false refusal** while the flock was free.
- Repo-wide grep: no remaining `is_alive` or `/proc` in `src`; `lock.rs:58` is the only pidfile read; no code assumes the old write-before-lock ordering.
- `tests/cli.rs` socket poll judged **strictly stronger** than the old check — the old `/proc` check was vacuously true on macOS, i.e. it never verified daemon exit there at all.
- `tests/task_2_3.rs`: traced the key's full provenance (fswatch → git-resolved paths → `bus.rs:71-73` subject → `worktree_dirty_view`). Product is uniformly on the git-resolved form, so canonicalizing the **expected** side is correct; canonicalizing product code would have made it diverge from git. The `unwrap_or_else` fallback **cannot** silently pass — a failed canonicalize yields a key that never matches, so the test times out and fails loudly.

## SHOULD-FIX (non-blocking)

**`src/daemon/lock.rs:51-58` — messaging race in the failure path.** Between the winner's `try_lock_exclusive()` success (:51) and its `fs::write` completing (:53), a contender whose flock attempt fails reads either:
- an **empty** pidfile (first-ever run → `held` parses to 0 → falls through to raw `LockError::Flock`), or
- the **previous holder's dead pid** → `Busy(dead_pid)`.

Message-quality only: every path still refuses to start, the one-daemon invariant is untouched, and the old code was strictly worse on all these paths. But the commit message's "reports the real holder (Busy) on every platform" is not literally always true, and evidence ledger **E62** claims exactly that.

Suggested fix (cheap, no timing logic): reword `LockError::Flock`'s message to hint at a possibly-running daemon, e.g.
`"could not take the pidfile lock (another daemon may be running; could not read the holder pid): {0}"`.
A single retry-read after ~20ms on the `Flock` path would also close most of the window.

## NITS

1. `src/daemon/lock.rs:3-5` — pin the flock assumption in the module doc. Correctness of writing the pid via `fs::write` (a second, short-lived fd) **depends on fs2 using `flock(2)`** (open-file-description-scoped, unaffected by truncation or another fd's close). If this ever migrates to an fcntl/`F_SETLK`-based locker, closing `fs::write`'s fd would **silently drop the lock**. One sentence protects a future fs2→fs4 migration.
2. `docs/superpowers/plans/artifacts/evidence-repo-assist-task-4-sessions.md` (E62) — amend "returns LockError::Busy ... on ALL platforms" with the write-window exception.
3. Windows caveat (only if ever targeted): fs2's Windows path is `LockFileEx` range locking; the contender's read of the locked region can fail outright (`Busy` never fires), and per-handle locks may break the winner's own `fs::write`. Not a supported platform today (IPC is Unix-socket based) — a one-line "Unix-only" note would suffice.

## Reviewer's stated assumptions

- flock-kill semantics follow documented `flock(2)` behavior (locks released on process termination) — reasoned, not empirically probed with SIGKILL.
- Windows `LockFileEx` semantics — reasoned from docs, not demonstrated.

## Disposition

- Zero MUST-FIX → **gate passed**, no fix round required.
- SHOULD-FIX deferred to Task 5 (docs touch `status.md` anyway) or a later cleanup commit — it is a message-wording change in product code, and Task 4 is the higher-value work. User informed.
