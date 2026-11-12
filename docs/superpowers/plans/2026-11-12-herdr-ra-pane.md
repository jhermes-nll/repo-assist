# repo-assist in herdr + fixing what `ra plans` actually reports

**Date:** 2026-11-12 (rev 2)
**Status:** awaiting user choice on order of work
**Scope:** bounded

---

## Confirmed outcome

1. A **button in herdr's UI next to Files/Changes** that surfaces repo-assist data.
2. **Unified TODO tracking across plans** that doesn't show junk.

---

## What changed from rev 1, and why

Rev 1 planned a `[[keys.command]] type="pane"` keybinding in `~/.config/herdr/config.toml`
and a wider plan-glob list. Both were wrong:

- **Wrong layer.** `~/.config/herdr/config.toml` is the **TUI** config. The GUI app the user
  actually uses is a different program with a different extension system.
- **Wrong premise.** "Spec docs are invisible" was stale. A `SpecAdapter` already exists and
  scans `docs/**/*.md`. The real defect is the opposite: over-inclusion plus stale rows.

---

## Evidence ledger

### herdr — two separate programs, two separate extension systems

| Claim | Evidence |
| --- | --- |
| `herdr-app` is the window (spaces/tabs/layouts/panels); `hc` (herdr-core) runs terminals and agents | `~/.config/herdr-app/AGENTS.md:10-18` |
| The folder **is** the interface, redrawn live from files, ~1s | `AGENTS.md:3-9` |
| Mods = `~/.config/herdr-app/mods/<name>/` with `mod.json` + `main.js` | `AGENTS.md:64`, on-disk `mods/tabs/`, `mods/files/`, `mods/changes/` |
| Mods use **gpui-kit**: JS describing GPUI nodes (`div()`, flex, `.on_click`); **no DOM, no HTML/CSS, no Node, no `setTimeout`** | `AGENTS.md:20-28`, `:47-52` |
| Mods **can** "run any app command, run programs (`exec`) and fetch URLs, keep state per mod or shared" | `AGENTS.md:44-46` |
| Buttons are **code, not manifest**. `tabs/main.js` draws the Files/Changes header buttons via `panelButton(id,label,icon)` | `mods/tabs/main.js:92-110` |
| `mod.json` has no button stanza. Real examples: `{"place":"none","title":"Tabs"}`; overlay variant `{"place":"none","title":"Add a machine","overlay":{"kind":"dialog","width":560}}` | `mods/tabs/mod.json`, `mods/machine-add/mod.json` |
| Header buttons switch a stack `panel-body` whose children are **mods** `files` / `changes` | `~/.config/herdr-app/layouts/main.json` (~140-165) |
| Validate a mod: `herdr-app check <mod folder>` — "Never call a mod done without a passing check" | `AGENTS.md:56-60` |
| Screenshot the window: `herdr-app shot [file.png]` | `AGENTS.md:62-74` |
| Invalid file → app keeps last good version, writes `<name>.error` | `AGENTS.md:56-57` |
| Escape hatch: `ctrl-alt-shift-escape` resets layout+keys | `AGENTS.md:58-60` |
| Invalid mods never break the app; check `<name>.error` after every edit | `AGENTS.md:56` |

**TUI plugin v1 explicitly cannot do UI** — verbatim from https://herdr.dev/docs/plugins/:
> "Runtime action registration and native non-terminal plugin UI are not part of plugin v1."

So `herdr-plugin.toml` is irrelevant to this task. Do not touch `~/.config/herdr/`.

### repo-assist — live data (`ra plans --json`, exit 0, 77 rows = 7 spec + 10 superpowers files × 5 worktrees)

| source | rows | tasks | declared done | **observed done** | zero-task rows |
| --- | --- | --- | --- | --- | --- |
| spec | 35 | 90 | 80 | **0** | 30 |
| superpowers | 42 | 60 | 25 | **0** | 28 |

States across 77 rows: `active 42, unknown 30, done 5`. **`observed_done > 0: 0`. `stalled: 6`.**

### The two real defects

**D1 — 60 of 77 rows belong to worktrees that no longer exist.**
- `EXISTS: /Users/jesse/Developer/repos/repo-assist`
- `GONE: .worktrees/plans-tick`, `/private/tmp/cezar-integrate/check-main`,
  `/private/tmp/cezar-integrate/repo-assist`, `/private/tmp/ra-main-check`
- **All 6 `stalled` findings are on deleted worktrees.** Stale rows are never pruned.
- Impact: `ra status --json` / the TUI / any herdr panel currently reports stalled plans
  that are 100% false positives. Building the UI before fixing this ships a liar.

**D2 — `docs/**/*.md` over-includes non-plan documents into `source='spec'`.**
6 of 7 spec files are not plans: `docs/OVERNIGHT-PROMPT.md`, `docs/agents-snippet.md`,
`docs/plan-tasks-2-4.md`, `docs/ra.1.md`, `docs/status.md`, `docs/verified-facts.md`.
Only `docs/2026-10-19-repo-assist-implementation-plan.md` is a real plan.
Spec globs are **hardcoded** in `SpecAdapter::default()` — `Sources` has no `spec` key
(`src/config.rs:148-160`), so this cannot be configured today.

**Not a defect:** `docs/superpowers/plans/artifacts/*.md` (22 files: ledgers, review rounds,
verify logs) are correctly absent. They are evidence, not plans. `docs/superpowers/plans/*.md`
is non-recursive so it skips them — working as intended. Do not "fix" this.

**Also not a defect:** every plan file path still exists on disk (0 missing-file staleness).

---

## Task 1 — Prune stale worktrees (do this first)

**Why first:** until it lands, every `stalled` assertion is false. Any UI built on top inherits
60 phantom rows.

**Files:** daemon sync path in `src/` — locate the `sync_plans` caller and the worktree
enumeration (`src/plans/discover.rs` has `sync_plans`; `src/store/` owns `worktrees` table).
The mechanism to reuse already exists: sync "marks gone" plans, so extend the same
gone-detection to worktrees whose directory no longer exists.

**Steps**
1. Find where worktrees are enumerated and upserted (`worktrees` table, `src/store/schema.sql`).
2. When a previously-seen worktree path no longer exists on disk, prune its rows (or mark
   `gone` and exclude from `status`/`plans`/`stalled`) — match whatever pattern `plans` already
   uses for gone plans rather than inventing a second convention.
3. Ensure `stalled` / `abandoned` computations skip gone worktrees.
4. Tests: a worktree row whose dir is deleted is excluded from stalled counts; a live worktree
   still reports; the pre-existing gone-plans behavior is unchanged.

**Verification**
```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test            # baseline ~149, report new count
./target/debug/ra plans --json   # rows must drop from 77 to ~17 (1 live worktree)
```
Expected: `stalled` count goes to 0 or reflects only the live worktree.

---

## Task 2 — Make spec globs configurable and narrow them (D2)

**Files:** `src/config.rs`, `src/plans/discover.rs`, plus wherever adapters are built from config.

1. `src/config.rs`: add `spec_globs: Vec<String>` to `PlansSource` (`:173-181`) with
   `#[serde(default = "default_spec_globs")]` and a `default_spec_globs()` fn.
   **`deny_unknown_fields` is on every struct** (`:12`, `:72`, `:148`, `:184`) — add the field
   to both `Config`-facing and `RawConfig` paths or absent-key configs start erroring.
2. `src/plans/discover.rs`: let `SpecAdapter` take its globs from config instead of
   `Default`-only. Confirm the real adapter construction site (the daemon's, not
   `default_adapters()` at `:143-149`, which is the unconfigured set) and record its path:line
   here when found.
3. Narrow the default. Proposal: exclude evidence directories. Either
   `["docs/**/*.md", "!docs/superpowers/plans/artifacts/**"]`, or an explicit
   `ignore_paths`-based exclusion if the glob engine has no negation. **Check whether
   `glob_match` supports negation before assuming it** — `src/plans/discover.rs` tests suggest
   a simple matcher, so `ignore_paths` (`src/config.rs:15-17`, already "paths excluded from
   overlap/drift checks") may be the right lever, or spec needs its own exclude list.
4. Tests: custom `spec_globs` honored; absent key → default; a doc under the exclude pattern
   produces no `spec` row; absent-key config still parses (the `deny_unknown_fields` trap).

**Verification:** the 6 non-plan docs produce no `spec` rows; `docs/2026-10-19-repo-assist-implementation-plan.md`
still does; full gate green.

---

## Task 3 — The herdr-app mod (deliverable 1)

**Files:** `~/.config/herdr-app/mods/repo-assist/{mod.json,main.js}` (+ optional icon svg),
and a small edit to surface a button.

1. **Clone before editing structure.** Shipped mods are Herdr's and are overwritten on start
   (`AGENTS.md:3-9`; `mods/my-tabs/` shows the `cloned_from` convention — copy that shape).
   Do **not** edit `mods/tabs/main.js` in place.
2. New mod `repo-assist`:
   - `mod.json`: `{"place": "none", "title": "repo-assist"}` — start with `place: "none"` so
     nothing changes until the panel is proven.
   - `main.js`: `exec` the CLI, parse JSON, render a list with `div()`/`h_flex()`. Two data
     calls max: `ra status --json` and `ra plans --json`, run from the repo path of the
     **current space/pane cwd**, not a hardcoded path.
   - Show only: active plans with open tasks, stalled/drift counts, and the current worktree's
     plan. Not a plan dump.
   - Handle the daemon-down case explicitly — render "daemon not running" rather than an
     empty panel. `ra plans` exits 2 with `daemon did not create its socket within 5s`.
3. **Button:** add a `panelButton("repo-assist", "Assist", <icon>)` alongside the existing
   Files/Changes buttons in the **cloned** header mod, pointing at a stack child whose content
   is `{ "mod": "repo-assist" }` in a layout file. Mirror `panel-body` in `layouts/main.json`
   (~140-165) rather than inventing a new structure.
4. `herdr-app check ~/.config/herdr-app/mods/repo-assist` must pass. Then check the app
   visually and via `herdr-app shot`.
5. Watch `<name>.error` after every write.

**Verification:** `herdr-app check` passes; the button appears next to Files/Changes; clicking
it shows live repo-assist numbers matching `ra plans --json` for the same repo; with the daemon
stopped it shows a clear message and does not render stale numbers.

**Rollback:** mods are additive — delete the mod directory. Layout change: keep a copy of the
original `layouts/main.json` before editing, or `ctrl-alt-shift-escape`.

---

## Ordering recommendation

**Task 1 → Task 2 → Task 3.** The mod is the visible win but the least valuable alone: a panel
that confidently shows six stalled plans that all belong to deleted worktrees is worse than no
panel. Task 3 becomes a 30-minute job once the data is honest.

If the user wants something on screen immediately, a read-only Task 3 against current data is
acceptable *provided* the panel labels stale worktree rows as stale.

---

## Risks

1. **Shipping a lying UI.** Highest risk by far — see D1. 60/77 rows phantom, 6/6 stalled phantom.
2. **Shipped mods get overwritten.** Editing `mods/tabs/` in place loses the work on next app
   start. Clone with `cloned_from`.
3. **`observed done = 0` across every row.** `tasks.observed` is never set to `done` anywhere
   observed. Either the observation pipeline is not wired for this repo, or nothing has run
   since it was added. Worth a look before trusting "declared 80 done vs observed 0" in any UI —
   this may be a second, larger bug than D1.
4. **gpui-kit is not DOM.** No `setTimeout` — use the app's `loop`/frames primitives. Anything
   web-shaped will not work.
5. **Daemon lifecycle.** `--no-spawn` exists; a mod that execs `ra` repeatedly could spawn
   daemons. Decide: is the mod allowed to start the daemon, or must it require one?
6. **Task 2 + `deny_unknown_fields`.** A missed struct breaks valid user configs.
7. **`exec` from a mod is arbitrary process execution.** Keep the command list static and
   argv-shaped; never interpolate paths from terminal contents.
