# Verified facts

> Section 13 of the implementation plan lists assumptions that MUST be checked against real
> data on this machine **before** code depends on them. This file records the answers found on
> Sunday 2026-10-18, from the live state at `~/.pi/agent/sessions` and the installed toolchain.
> Everything below was read from real files/commands — not inferred.

Toolchain: `cargo 1.97.1`, `rustc 1.97.1`, host `aarch64-unknown-linux-gnu`.
Git: `git version 2.43.0` (see section 13.4 — `merge-tree --write-tree` tested OK on 2.43).

---

## Item 1 — Real pi session entry shapes (verified ✅)

Captured from the 3 real session files under
`~/.pi/agent/sessions/--home-jesse--/*.jsonl` (now under `tests/fixtures/pi/`, redacted):

- `~/.pi/agent/sessions/--home-jesse--/2026-10-10T06-59-57-613Z_01a1249c-…jsonl` (56 KB, 15 entries)
- `…07-00-26…01a1249c-ec21-…jsonl` (34 KB, 10 entries)
- `…07-00-36…01a1249d-14fb-…jsonl` (52 KB, 13 entries)

No `PI_CODING_AGENT_SESSION_DIR` / `PI_CODING_AGENT_DIR` env vars are set on this machine, so the
effective root resolves to `${PI_CODING_AGENT_DIR:-~/.pi/agent}/sessions` — which matches where the
files actually live (layout `--<encoded-cwd>--/<timestamp>_<uuid>.jsonl`).

### Header (first line, version 3)
```json
{"type":"session","version":3,"id":"01a1249c-…","timestamp":"2026-10-10T06:59:57.613Z","cwd":"/home/jesse"}
```
`parentSession` present on the header is always `null` in the fixtures. Version 3 supported fully.

### Entry kinds actually present
`session` (header), `model_change`, `thinking_level_change`, `message`. **0 parse errors.**
- `model_change`: `{type,id,parentId,timestamp,provider,modelId}` (e.g. `provider="ollama"`,
  `modelId="deepseek-v4.1-flash:cloud"`).
- `thinking_level_change`: `{type,id,parentId,timestamp,thinkingLevel}`.
- `message`: `{type,id,parentId,timestamp,message:{role,content,usage,stopReason,…}}`.

**None of the 3 real files contain a `compaction` or `branch_summary` entry.** Those entry kinds
are handled defensively (parse + warn) per Section 12 but are not exercised by captured data.
Branching is carried **entirely** by the `parentId` linkage on each `message` — there are no
separate "branch" entries. This matches Section 12's "tree handling" but is important to state:
`active_leaf()` must resolve the branch via `parentId`, not by looking for a `compaction`/
`branch_summary` marker.

### Message roles
`system`, `user`, `assistant`, `toolResult`. (No other roles observed.)

### Token usage (verified shapes)
`usage` is nested inside `message.usage` and **only** present on `role == "assistant"` messages.
`model_change` / `thinking_level_change` have no `usage`.
`usage` keys: `input, output, cacheRead, cacheWrite, reasoning, totalTokens, cost`.
`usage.cost` keys (object): `input, output, cacheRead, cacheWrite, total` — always present as an
object; `total` is the field to sum for cost, else leave `cost_usd` null.
On assistant messages there is **no missing-usage** case. Summed `(input+output+cacheRead+cacheWrite)`
across one file ≈ 46329 tokens.

Token total to use: `input + output + cacheRead + cacheWrite` (ignore `reasoning`/`totalTokens`),
matching `src/plans/derive.rs`-adjacent session model in the plan.

### Tool calls (verified shapes)
Tool calls live **inside** assistant messages, in `message.content[]` (an array) as blocks:
```json
{"type":"toolCall","id":"call_08tfd56d","name":"read","arguments":{"path":"/home/…"}}
```
`content[]` may intermix `{"type":"thinking",…}`, `{"type":"text",…}`, and
`{"type":"toolCall",…}` blocks — only blocks with `type == "toolCall"` are tool calls.
Names observed: `read`, `bash` (extensible).

### Tool results (verified shapes)
Tool results are top-level entries with `role == "toolResult"` (NOT `type`). Each carries:
`toolCallId` (matches the toolCall's `id`), `toolName`, `isError` (boolean), `durationMs`, plus
`message.content` (array, may hold `{type:"text"}` blocks). No `usage` on tool-result messages.
So: iterate `message[]` entries; a message with `role=="toolResult"` and a non-null `toolCallId`
is a result; match it to its tool call by `toolCallId == toolCall.id`.

### Discriminators to key on (for the adapter)
| role | usage present? | toolCall block in content? | toolCallId / toolName |
|---|---|---|---|
| `assistant` | yes (object) | yes (subset of `content[]`) | — |
| `toolResult` | no | no (has `message.content[]` with `{type:"text"`) | `toolCallId`, `toolName`, `isError` |
| `system`/`user` | no | no | — |

Consequence for `agent.loop` (same `(tool, args_hash) ≥ loop_repeats` AND (≥80% errors OR identical
`result_hash`)): `isError` is a real boolean on results; `result_hash` is `sha256(canonical JSON of
the toolResult message.content)`. Both derive cleanly from these shapes.

---

## Item 2 — Real superpowers plan markup (verified ❌ — not present on this machine)

No `docs/superpowers/plans/*.md`, no `TODO.md`, no `PLAN.md`, and no `docs/superpowers/` directory
exist anywhere under `/home/jesse` (excluding `node_modules` and `.git`). Real plan files were
**not captured** because there are none here.

Action taken instead: the representative sample from the plan itself (Task 3.1) is treated as the
authoritative spec for the superpowers parser, plus the checklist parser's default globs. The
parser MUST be robust: unknown/other framework files yield a plan with zero tasks + a logged
warning (never a panic), and malformed files never panic (property/fuzz-lite test required per the
plan).

Tuning targets from the sample (must hold once implemented):
- task heading matches `^#{2,4}\s+Task\s+\d+` (sample uses `### Task 1:`);
- `Create:`/`Modify:`/`Test:` lines → `expected_paths`, stripping `:line`/`:line-line` and `(…)`;
- declared state = task `done` only if every checkbox is `[x]` and ≥1 exists; else `open`.

---

## Item 3 — pi session `cwd` == worktree path (verified ❌ from real data)

The 3 captured sessions all have `"cwd":"/home/jesse"`. The git repos present on this machine are
under `~/dev/` (`/home/jesse/dev/subtext`, `…/website`, `…/BBL_SHO`, …). `/home/jesse` (where
sessions point) is **not** any worktree's toplevel, and is not even a git repo.

Conclusion: the "session cwd equals a worktree path" invariant does **not** hold for the
captured data — a session's cwd need not be contained by any discovered worktree. The tree-mapping
step (`worktree_path` is equal to or a prefix of canonicalized cwd) still works correctly: it
simply yields *no* match for these sessions, which is the correct, honest behaviour (a session is
not force-joined to a worktree it doesn't belong to).

Action: the real subtext repo was **not** scaffolded (no `~/dev/subtext` exists on this
machine). The card's "see below" pointer is therefore a dead reference; it has been removed
and the item left in its verified-❌ state. Test fixtures that *do* drive the daemon use
synthetic repos built in `tests/common.rs` (`GitRepo::new()`), not a subtext clone.

---

## Item 4 — `git merge-tree --write-tree` supported (verified ✅)

`git version 2.43.0 ≥ 2.38` (feature present). Tested live on a temp repo:
- Two-feature merge with a real file conflict → exit code **1** and conflict output including
  `CONFLICT (content): Merge conflict in c` (and staged blob lines). ✓
- Non-conflicting merge of two branches → exit 0 with a single tree sha printed. ✓
- `--write-tree` behaves correctly; `--trivial-merge` used for non-conflicts.

So the `worktree.merge_conflict` check can be enabled and `ra doctor` should assert the version.

---

## Item 5 — Plan checkbox updates committed on the plan's branch (verified ❌ — not checkable)

Not checkable from machine state (no real superpowers plans exist). This is a **product/tuning**
question, not a data fact: whether checkbox updates a plan author marks `[x]` are committed before
they show up in `git log <merge_base>..HEAD` on the plan branch. Per the plan, we **do not**
silently suppress this — we tune `plan.checkbox_mismatch` so it reports with `confidence=inferred`
(informational until an explicit task id is pushed by a future `ra report`), and we let
`ra plan`/`plan.stalled` show the discrepancy to the human rather than hide it. No code decision is
locked until real plan data appears.

---

## Bonus observed (not in Section 13, worth recording)

- Effective pi sessions dir (no `PI_*` env) = `~/.pi/agent/sessions` — matches Section 12 default.
- Local sessions use provider `ollama` / model `deepseek-v4.1-flash:cloud`.
- `isError` is a boolean in data (not an int); hash `arguments` via canonical JSON → sha256,
  first 16 hex, as the plan specifies.
