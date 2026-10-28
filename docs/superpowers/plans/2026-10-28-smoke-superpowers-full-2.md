# Smoke test 2: `superpowers_full` with the autosave fixes

**Goal:** confirm against the real engine that the per-task commit contains the task's actual work (not just the plan tick), that no `cezar autosave` commit survives above the task base, and that the reviewers diff against the recorded task base.

**Why a docs-only change:** the point is to exercise the graph. The verification command below is the entire test obligation.

## Task 1: Append a marker line to status.md

- [ ] Append exactly this line to the end of `status.md`: `Smoke test 2: superpowers_full committed this with the task work.`
  - Files: `status.md` only. Do not reformat, retitle, or touch any other file.
  - Verify: `grep -c "Smoke test 2: superpowers_full committed this with the task work." status.md` prints `1` and exits 0.
  - Done when: that command prints `1`.

## Out of scope

Anything except the single line above.
