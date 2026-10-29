# Post-merge debt cleanup — repo-assist

**Date:** 2026-10-28
**Base:** `main` @ `5cf6c20` (must be current — this plan assumes the seven `overnight-*`
branches are already merged)
**Context:** All 16 spec tasks are complete and the suite is at 124 tests. This unit closes
small, concrete debt flagged by two review rounds. **No new features, no behavior changes.**

## Tasks

- [ ] **1. Remove dead alias constants.** `src/store/mod.rs:35-70` holds ~25 `const` aliases
  with zero references. Verify each with `rg -n '<NAME>' src/ tests/` before deleting; keep
  any that is still used and note why. Done when no unreferenced `const` remains and
  `cargo build` is clean.

- [ ] **2. Guard the leaking daemon spawn in a test.** `tests/daemon_zero_writes.rs:108`
  spawns `ra daemon --foreground` but only calls `kill()`/`wait()` at end of test, so a
  failed assertion orphans a daemon that spins against a deleted temp dir — the exact flake
  class already fixed in seven other test files. Reuse the kill-on-drop guard + `Stdio::null()`
  pattern at `tests/subscribe_rpc.rs:27-46`.

- [ ] **3. Clear the 5 pre-existing clippy warnings.** `src/lib.rs:11-12` (duplicated
  attribute), `src/plans/discover.rs:122` (empty line after doc comment),
  `src/plans/derive.rs:234` and `:250` (`unnecessary_get_then_check`). If a lint fix would
  change behavior, leave it and report instead.

- [ ] **4. Refresh `docs/status.md`.** It reports a stale test count and commit. Update to
  **124** tests and the current commit, and note that the seven `overnight-*` branches landed.
  Do not delete or strike a "Known limitations" entry unless code or a test backs it.

## Out of scope

No new features. No changes to `scripts/perf.sh` or the README performance numbers. No branch
deletion. No Notion/board changes. No restructuring of `src/store/mod.rs` beyond deleting dead
consts.

## Files owned

`src/store/mod.rs`, `src/lib.rs`, `src/plans/discover.rs`, `src/plans/derive.rs`,
`tests/daemon_zero_writes.rs`, `docs/status.md`

## Verification

```text
cargo test                        # >=124 passed, 0 failed
cargo test                        # repeat 3x consecutively - this suite was flaky
cargo clippy --all-targets        # no warnings from the four files above
rg -n '<REMOVED_CONST>' src/ tests/   # must be empty for every removed constant
```

A single green `cargo test` run is **not** evidence — the suite hung intermittently before the
daemon guards landed.

## If something does not add up

If a "dead" constant turns out to be live, or a clippy fix would change behavior, stop and
report the discrepancy rather than forcing the change.
