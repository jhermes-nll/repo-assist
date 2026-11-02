# Cezar review loops: raise every repeat limit to 5

Status: approved by user in chat (all three loops → 5; backups untouched).

## Outcome

Every repeat limit governing a review cycle in the live cezar workflows is 5.

## Scope — files to edit (live only)

Canonical set under `/Users/jesse/Developer/cezar/workflows`:

| File | Current | Target |
| --- | --- | --- |
| `shared/maintenance.yaml` (~:115, impl↔review fix loop) | `max: 3` | `max: 5` |
| `shared/maintenance.yaml` (~:13, prose "sonnet review loop (3 rounds)") | `3 rounds` | `5 rounds` |
| `shared/superpowers_full.yaml` (~:261, failed-round retry budget) | `max: 4` | `max: 5` |
| `shared/superpowers_full.yaml` (~:321, final review rounds) | `max: 2` | `max: 5` |
| `shared/superpowers_task.yaml` (~:206, task-level failed rounds) | `max: 4` | `max: 5` |
| `lib/pr-review-common.sh` (~:264) | `${PR_REVIEW_MAX_ATTEMPTS:-2}` | `${PR_REVIEW_MAX_ATTEMPTS:-5}` |

Per-project copies that the container actually consumes (`cp -R /workflows/<repo>/. /repos/<repo>/`):

- `ai-dashboard/.ai/cezar/workflows/superpowers_full.yaml`
- `ai-dashboard/.ai/cezar/workflows/superpowers_task.yaml`
- `repo-assist/.ai/cezar/workflows/superpowers_full.yaml`
- `repo-assist/.ai/cezar/workflows/superpowers_task.yaml`
- `subtext/.ai/cezar/workflows/superpowers_full.yaml`
- `subtext/.ai/cezar/workflows/superpowers_task.yaml`

Match by loop context, not by line number — the copies may be offset from `shared/`.
If a copy is a symlink into `shared/`, leave it alone and record that fact.

## Explicitly out of scope

- `shared/legacy-v1/*.superpowers.yaml.bak`
- `shared/superpowers.yaml.bak-pre-*`
- Any `.bak-pre-review-failed` inside project dirs
- Workflow topology, prompts' logic, or anything other than the repeat counts

## Consistency requirement

Any test or helper asserting the old numbers must move with them
(`lib/pr-review-state.test.sh`, `lib/pr-review-testlib.sh`, `lib/superpowers-pr.test.sh`).
Prose that restates a count ("escalate after 3", "two more times") must not contradict the new numbers.

## Verification

1. `for t in /Users/jesse/Developer/cezar/workflows/lib/*.test.sh; do bash "$t" || echo "FAIL $t"; done` — all pass.
2. `grep -n "max: [0-9]" ` over the six live yaml files shows 5 at every review-loop site and no stray 2/3/4.
3. `grep -n "PR_REVIEW_MAX_ATTEMPTS" lib/pr-review-common.sh` shows `:-5`.
4. Backups unchanged: `git status`/`diff` on the `.bak*` paths is empty.
