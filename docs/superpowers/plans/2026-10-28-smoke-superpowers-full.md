# Smoke test: `superpowers_full` end-to-end

**Goal:** prove the `superpowers_full` cezar workflow runs its full per-task loop — implement, spec review, quality review, tick, commit — and opens a draft PR, on a change small enough that any failure is the workflow's fault, not the code's.

**Why a docs-only change:** the point is to exercise the graph, not to land code. Reviewers must accept that the verification below is the whole test obligation.

## Task 1: Append a smoke-test line to README.md

- [x] Append exactly this line to the end of `README.md`: `Smoke test: superpowers_full graph executed end to end.`
  - Files: `README.md` only.
  - No other change. Do not reformat, retitle, or touch any other file.
  - Verify: `grep -c "Smoke test: superpowers_full graph executed end to end." README.md` prints `1` and exits 0.
  - Done when: that command prints `1`.

## Out of scope

Anything except the single line above.
