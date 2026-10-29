# Post-merge debt cleanup — repo-assist

**Date:** 2026-10-28
**Base:** `main` @ `080d0e3`
**Context:** Seven long-lived `overnight-*` branches were just merged into `main`
(see `docs/superpowers/plans/2026-10-28-land-overnight-branches.md`), taking the suite from
101 to 124 tests. All 16 spec tasks are now complete. This unit closes the small, concrete
debt that two review rounds flagged — **no new features, no behavior changes.**

## Items

### 1. Remove dead alias constants
`src/store/mod.rs:35-70` holds ~25 `const` aliases with zero references (flagged in two
independent review rounds). Verify each with `rg -n '<NAME>' src/ tests/` before deleting —
if a name has any use, keep it and say why. Deliverable: no unreferenced `const` remains,
and the build still passes.

### 2. Guard the leaking daemon spawn in a test
`tests/daemon_zero_writes.rs:108` spawns `ra daemon --foreground` and only calls `kill()` +
`wait()` at the end of the test, so a failed assertion leaks an orphan daemon that spins
against a deleted temp dir. That is the exact flake class just fixed in the other seven
test files (`tests/subscribe_rpc.rs:27-46` has the reference kill-on-drop guard, plus
`Stdio::null()`). Reuse that pattern.

### 3. Clear the 5 pre-existing clippy warnings in the library
- `src/lib.rs:11-12` — duplicated attribute
- `src/plans/discover.rs:122` — empty line after doc comment
- `src/plans/derive.rs:234` and `:250` — `unnecessary_get_then_check`

These predate the merge; clear them so `cargo clippy` is quiet and future warnings are
visible. Do not change behavior to satisfy a lint — if a lint fix would alter semantics,
leave it and report it.

### 4. Refresh `docs/status.md` to current reality
It still reports stale figures. Update: test count **124** (currently wrong), current
commit (`080d0e3` or later), and add a short note that the seven overnight branches landed.
Keep the existing honest "Known limitations" section — do not delete real open items, and do
not strike a limitation unless code or a test backs it.

## Out of scope

No new features. No changes to the perf numbers or `scripts/perf.sh`. No branch deletion.
No Notion/board changes. No restructuring of `src/store/mod.rs` beyond deleting dead consts.

## Files owned

`src/store/mod.rs`, `src/lib.rs`, `src/plans/discover.rs`, `src/plans/derive.rs`,
`tests/daemon_zero_writes.rs`, `docs/status.md`

## Done when

1. `cargo test` → **≥124 passed, 0 failed**, and it does so on **3 consecutive runs**
   (post-merge the suite was flaky until the daemon guards landed; a single green run is not
   evidence).
2. `cargo clippy --all-targets` → no warnings from the four files listed, or a written
   justification for each one left.
3. `rg` shows zero references for every removed constant.
4. `docs/status.md` states 124 tests and a commit at or before current HEAD, and its
   limitations section contains no claim that code contradicts.

If any item turns out to be wrong (a "dead" constant is live, a clippy fix would change
behavior), stop and report the discrepancy rather than forcing the change.
