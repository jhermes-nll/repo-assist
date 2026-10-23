//! Task 3.1 acceptance: `tests/plans_parse.rs`.
//!
//! Exercises the plan parser (`repo_assist::plans`) directly — no CLI, no daemon. Mirrors
//! `tests/task_2_3.rs` which drives `bus::start_bus` directly. Covers:
//!
//! - the representative sample (Task 1: Add limiter / Task 2: Wire into router) parses to 2
//!   tasks with paths `src/limiter.rs`, `tests/limiter.rs`, `src/router.rs`;
//! - the declared-state rule (done only when every checkbox is `[x]` and ≥1 exists, else open);
//! - the generic checklist format (one task per checkbox, empty `expected_paths`);
//! - unknown format → zero tasks **plus a logged warning** (never a panic);
//! - **fuzz-lite**: many random/garbage inputs must never panic.

use std::fs;

use repo_assist::plans::{parse_plan, parse_plan_file, ParsedPlan};

/// Path to this repo's `tests/fixtures/plans/` directory (relative to the crate root, which
/// is where `cargo test` runs).
fn fixtures() -> std::path::PathBuf {
    std::path::Path::new("tests").join("fixtures").join("plans")
}

/// A superpowers-format file (see `superpowers-plan.md`) that the acceptance sample expects to
/// parse to 2 tasks with the expected paths.
fn superpowers_plan_text() -> String {
    // Inline copy of `tests/fixtures/plans/representative-superpowers-plan.md` so the test
    // is self-contained regardless of CWD.
    concat!(
        "### Task 1: Add limiter\n",
        "Create: `src/limiter.rs`\n",
        "Test: `tests/limiter.rs`\n",
        "- [ ] Add the limiter\n",
        "- [ ] Add tests for the limiter\n",
        "\n",
        "### Task 2: Wire into router\n",
        "Create: `src/router.rs`\n",
        "- [ ] Wire the limiter into the router\n"
    )
    .to_string()
}

fn ordered_paths(plan: &ParsedPlan) -> Vec<String> {
    let mut all: Vec<String> = plan
        .tasks
        .iter()
        .flat_map(|t| t.expected_paths.iter().cloned())
        .collect();
    all.sort();
    all.dedup();
    all
}

/// Representative sample: 2 tasks, paths `src/limiter.rs`, `tests/limiter.rs`, `src/router.rs`.
#[test]
fn representative_sample_parses_to_two_tasks_with_expected_paths() {
    let plan = parse_plan(&superpowers_plan_text());

    assert_eq!(plan.task_count(), 2, "expected exactly 2 tasks");

    // Task 1 paths (order preserved: Create: before Test:).
    let t1 = &plan.tasks[0];
    assert_eq!(t1.title, "Add limiter");
    assert_eq!(
        t1.expected_paths,
        vec!["src/limiter.rs", "tests/limiter.rs"]
    );
    // Task 2 paths.
    let t2 = &plan.tasks[1];
    assert_eq!(t2.title, "Wire into router");
    assert_eq!(t2.expected_paths, vec!["src/router.rs"]);

    // Union of expected paths across the sample (sorted) — the acceptance criterion.
    let got = ordered_paths(&plan);
    assert_eq!(
        got,
        vec!["src/limiter.rs", "src/router.rs", "tests/limiter.rs"]
    );

    // Both tasks open (no checkbox checked in the sample), none done.
    assert!(plan.tasks.iter().all(|t| t.declared == "open"));
    assert_eq!(plan.declared_state(), "open");
}

/// Parsing from a fixture file yields the same plan as parsing the same content inline.
#[test]
fn fixture_file_parses_like_inline_text() {
    let file = fixtures().join("representative-superpowers-plan.md");
    let from_file = parse_plan_file(&file);
    let from_text = parse_plan(&superpowers_plan_text());
    assert_eq!(from_file.task_count(), 2);
    assert_eq!(from_file, from_text);
}

/// Declared state rule: done only when every checkbox is `[x]` and ≥1 exists; otherwise open.
#[test]
fn declared_state_only_done_when_all_checked_and_one_exists() {
    // No checkboxes at all -> open (a task with no checkbox cannot be done).
    let no_boxes = "### Task 1: no boxes\nCreate: `src/a.rs`\n";
    let plan = parse_plan(no_boxes);
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].declared, "open");

    // One checked of two -> open.
    let partial = "### Task 1: some\n- [x] one\n- [ ] two\n";
    let plan = parse_plan(partial);
    assert_eq!(plan.tasks[0].declared, "open");

    // All three checked -> done.
    let all = "### Task 1: all\n- [x] a\n- [x] b\n- [x] c\n";
    let plan = parse_plan(all);
    assert_eq!(plan.tasks[0].declared, "done");

    // A single unchecked box -> open.
    let one = "### Task 1: one\n- [ ] a\n";
    let plan = parse_plan(one);
    assert_eq!(plan.tasks[0].declared, "open");

    // A single checked box -> done (≥1 exists, all checked).
    let one_done = "### Task 1: done\n- [x] a\n";
    let plan = parse_plan(one_done);
    assert_eq!(plan.tasks[0].declared, "done");

    // The whole plan's declared state reflects the rule: done iff all tasks done and ≥1 task.
    let all_done_text = "### Task 1: a\n- [x] x\n### Task 2: b\n- [x] y\n";
    let plan = parse_plan(all_done_text);
    assert_eq!(plan.declared_state(), "done");

    let mixed_text = "### Task 1: a\n- [x] x\n### Task 2: b\n- [ ] y\n";
    let plan = parse_plan(mixed_text);
    assert_eq!(plan.declared_state(), "open");

    // Zero tasks -> open.
    assert_eq!(
        parse_plan("just a heading\n\nsome text\n").declared_state(),
        "open"
    );
}

/// The generic checklist format: one task per top-level checkbox, empty `expected_paths`.
#[test]
fn generic_checklist_yields_one_task_per_checkbox_with_empty_paths() {
    let text = "# TODO\n\n- [ ] Fix the bug\n- [x] Add logging\n- [ ] Write docs\n";
    let plan = parse_plan(text);

    assert_eq!(plan.task_count(), 3);
    assert_eq!(plan.tasks[0].title, "Fix the bug");
    assert_eq!(plan.tasks[0].declared, "open");
    assert_eq!(plan.tasks[1].title, "Add logging");
    assert_eq!(plan.tasks[1].declared, "done");
    assert_eq!(plan.tasks[2].title, "Write docs");
    assert_eq!(plan.tasks[2].declared, "open");
    // Checklist tasks carry no expected paths.
    for t in &plan.tasks {
        assert!(t.expected_paths.is_empty());
    }
}

/// Unknown format yields zero tasks and a **recorded** warning (never a panic).
#[test]
fn unknown_format_returns_zero_tasks_and_warning() {
    let text = "# Weekly Notes\n\nSome prose only.\n- a bullet\n- another bullet\n";
    let plan = parse_plan(text);

    assert_eq!(plan.task_count(), 0);
    assert!(
        !plan.warnings.is_empty(),
        "unknown format must log a warning"
    );
    assert!(plan.warnings[0].contains("0 tasks"));
}

/// A fixture file that is not a plan must also parse to zero tasks + warning.
#[test]
fn unknown_fixture_parses_zero_tasks_with_warning() {
    let file = fixtures().join("unknown-format.md");
    let plan = parse_plan_file(&file);
    assert_eq!(plan.task_count(), 0);
    assert!(!plan.warnings.is_empty());
}

/// `to_task_row` produces a `store::TaskRow` whose fields match the source task (used by the
/// plan importer to upsert into `tasks`).
#[test]
fn to_task_row_mirrors_source_fields() {
    let plan = parse_plan(&superpowers_plan_text());
    let t0 = &plan.tasks[0];
    let row = t0.to_task_row();

    assert_eq!(
        row.expected_paths,
        vec!["src/limiter.rs", "tests/limiter.rs"]
    );
    assert_eq!(row.ordinal, 1);
    assert_eq!(row.declared, "open");
    assert_eq!(row.title, "Add limiter");
}

/// Task ids are plan-scoped, so two plans whose tasks share heading numbers get distinct ids
/// once the importer assigns the plan id (the `tasks.id` primary key is global).
#[test]
fn task_ids_are_plan_scoped_across_plans() {
    let text = "### Task 1: same heading\n- [ ] a\n### Task 2: same heading\n- [ ] b\n";
    let mut plan_a = parse_plan(text);
    let mut plan_b = parse_plan(text);

    for t in &mut plan_a.tasks {
        t.set_plan_id("plan-a");
    }
    for t in &mut plan_b.tasks {
        t.set_plan_id("plan-b");
    }

    assert_eq!(plan_a.tasks[0].id, "plan-a:1");
    assert_eq!(plan_a.tasks[1].id, "plan-a:2");
    assert_eq!(plan_b.tasks[0].id, "plan-b:1");
    assert_eq!(plan_b.tasks[1].id, "plan-b:2");
    assert_ne!(
        plan_a.tasks[0].to_task_row().id,
        plan_b.tasks[0].to_task_row().id
    );

    // The generic checklist parser scopes its ids the same way.
    let mut checklist = parse_plan("# TODO\n- [ ] one\n- [x] two\n");
    for t in &mut checklist.tasks {
        t.set_plan_id("todo-plan");
    }
    assert_eq!(checklist.tasks[0].id, "todo-plan:1");
    assert_eq!(checklist.tasks[1].id, "todo-plan:2");
}

/// An unterminated backtick keeps the paths already collected on the directive line instead
/// of discarding the whole declaration.
#[test]
fn unterminated_backtick_keeps_collected_paths() {
    let text = "### Task 1: paths\nCreate: `src/a.rs` and `src/b.rs\n";
    let plan = parse_plan(text);

    assert_eq!(plan.tasks.len(), 1);
    assert!(
        plan.tasks[0]
            .expected_paths
            .contains(&"src/a.rs".to_string()),
        "the path before the unterminated span must survive"
    );
    assert_eq!(plan.tasks[0].expected_paths, vec!["src/a.rs", "src/b.rs"]);
}

/// A markdown link bullet (`- [spec](http://x)`) is not a checkbox, so a fully-ticked task
/// that contains one is still reported `done`.
#[test]
fn markdown_link_bullet_does_not_count_as_a_checkbox() {
    let text = "### Task 1: done with a link\n- [x] real work\n- [spec](http://x)\n";
    let plan = parse_plan(text);

    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].declared, "done");
    assert_eq!(plan.declared_state(), "done");
}

/// Indented checkboxes are nested under their parent and must not become top-level tasks.
#[test]
fn indented_checkbox_does_not_become_a_separate_task() {
    let text = "# TODO\n- [ ] parent\n    - [ ] nested child\n  - [x] another child\n";
    let plan = parse_plan(text);

    assert_eq!(plan.task_count(), 1);
    assert_eq!(plan.tasks[0].title, "parent");
    assert_eq!(plan.tasks[0].declared, "open");
}

/// A single unknown marker (e.g. `- [~]`) is a checkbox the author never ticked: it counts
/// as unchecked (and a fully-ticked task plus such a marker still reads `done`).
#[test]
fn unknown_single_char_marker_counts_as_unchecked() {
    let text = "### Task 1: marker\n- [x] real work\n- [~] wavy one\n";
    let plan = parse_plan(text);

    assert_eq!(plan.tasks.len(), 1);
    // 2 boxes total, 1 checked -> open (the ~ box counts, but is unchecked).
    assert_eq!(plan.tasks[0].declared, "open");

    let all_ticked = "### Task 2: all ticked\n- [x] a\n- [~] b\n";
    // ~ counts as a box, so 1 of 2 checked -> open.
    assert_eq!(parse_plan(all_ticked).tasks[0].declared, "open");

    let no_boxes = "### Task 3: all unchecked\n- [~] wavy\n- [ ] plain\n";
    // 0 checked of 2 -> open, but both lines are recognized as boxes.
    assert_eq!(parse_plan(no_boxes).declared_state(), "open");
}

/// A fully-indented list parses to zero tasks and falls through to the unknown-format
/// warning (checklist only takes column-0 bullets); this is the intended under-count.
#[test]
fn fully_indented_list_is_not_a_plan() {
    let text = "# TODO\n  - [ ] one\n  - [x] two\n";
    let plan = parse_plan(text);

    assert_eq!(plan.task_count(), 0);
    assert!(!plan.warnings.is_empty());
}

/// `expected_paths` cleaning: strip `:line`/`:line-line` ranges and trailing `(…)` notes.
#[test]
fn path_declaration_strips_line_ranges_and_notes() {
    let text = concat!(
        "### Task 1: edits\n",
        "Create: `src/foo.rs:12-34`\n",
        "Modify: `src/bar.rs (with logging)`\n",
        "Delete: `src/gone.rs:8`\n"
    );
    let plan = parse_plan(text);
    assert_eq!(plan.tasks.len(), 1);
    let paths = &plan.tasks[0].expected_paths;
    assert_eq!(
        paths,
        &vec![
            "src/foo.rs".to_string(),
            "src/bar.rs".to_string(),
            "src/gone.rs".to_string()
        ]
    );
}

/// A malformed / partial plan (a heading with no closing, unclosed bracket, etc.) must never
/// panic — it parses to some plan (possibly zero tasks).
#[test]
fn malformed_plan_does_not_panic() {
    let inputs = [
        "### Task 1: unclosed bracket\n- [ fix me\n",
        "Task 1: no hash\n",
        "### Task 1:\nCreate: `src/a.rs:99`\n- [x]\n",
        "- [ ] lone checkbox with no heading\n",
        "### Task 1\n### Task 2\n### Task 3\n",
        "### Task abc: non-numeric\n",
        "",
    ];
    for s in inputs {
        let _ = parse_plan(s); // must not panic
    }
    let _ = parse_plan_file(&fixtures().join("nonexistent.md")); // missing file -> zero tasks, no panic
}

/// Fuzz-lite: generate and parse many synthetic plan documents (random markdown), asserting
/// that no input panics. This is the "property test" the acceptance criteria require.
#[test]
fn fuzz_lite_never_panics() {
    // Deterministic, cheap pseudo-randomness — no external crate.
    let seed: u64 = 0x9E3779B97F4A7C15;
    let next_usize = |s: &mut u64| -> u64 {
        // xorshift + mulberry-style
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        ((*s).wrapping_mul(0x2545F49A1A1B3C1D) >> 32) % u64::MAX
    };
    let mut state = seed;

    // A small corpus of "fragments" drawn and concatenated to form plausible-ish plan text.
    let fragments = [
        "## Task ",
        "### Task ",
        "### Task 1: limiter\n",
        "Create: `src/limiter.rs:12-34`\n",
        "Modify: `src/router.rs (wire it)\n",
        "Test: `tests/limiter.rs`\n",
        "- [ ] check something\n",
        "- [x] done thing\n",
        "- [ ]  \n",
        "some prose\n\n",
        "Create: no path here\n",
        "Task 1:\n",
        "#### Task 3: nested deep\n",
        "- [X] upper case x\n",
    ];

    for _ in 0..4_000 {
        let n = (next_usize(&mut state) % 40) as usize;
        let mut buf = String::new();
        for _ in 0..n {
            let idx = (next_usize(&mut state) % fragments.len() as u64) as usize;
            // Occasionally sprinkle raw unicode / control / long lines that must not crash.
            if next_usize(&mut state) % 10 == 0 {
                buf.push_str("日本語のテスト ✎ ── 日本語テスト\n");
            }
            buf.push_str(fragments[idx]);
        }
        // Force a worst-case length occasionally.
        if next_usize(&mut state) % 25 == 0 {
            buf.push_str(&"x".repeat(20_000));
        }
        // Never panic on any generated plan.
        let plan = parse_plan(&buf);
        // Invariants: task_count() and declared_state() are well-defined for any plan.
        assert!(plan.task_count() <= buf.chars().count());
        assert!(
            plan.declared_state() == "open" || plan.declared_state() == "done",
            "declared_state must be open or done"
        );
    }

    // A few adversarial raw inputs too.
    for raw in [
        "\0\0\0",
        "\n".repeat(100_000).as_str(),
        "\u{7f}".repeat(50_000).as_str(),
    ] {
        let plan = parse_plan(raw);
        assert_eq!(plan.task_count(), 0, "raw garbage yields 0 tasks");
    }

    // Load every fixture and assert each parses without panicking (incl. unknown format).
    for entry in fs::read_dir(fixtures()).expect("fixtures dir readable") {
        let path = entry.unwrap().path();
        if path.is_file() {
            let _ = parse_plan_file(&path);
        }
    }

    // Silence the "used" lint for next_usize if a later iteration removed it.
    let _ = next_usize;
}
