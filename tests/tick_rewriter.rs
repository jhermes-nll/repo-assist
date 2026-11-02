mod common;

use repo_assist::plans::tick::{rewrite_lines, select_tick_lines, tick_one_line};
use std::collections::BTreeSet;

#[test]
fn tick_one_line_table() {
    let cases = vec![
        ("- [ ] foo", Some("- [x] foo")),
        ("- [x] foo", None),
        ("- [X] foo", None),
        ("- [~] foo", None),
        ("- [ ]\r\n", Some("- [x]\r\n")),
        ("  - [ ] nested", Some("  - [x] nested")),
        ("- [spec](http://x) not box", None),
        ("- [ ] already? no", Some("- [x] already? no")),
    ];
    for (input, want) in cases {
        assert_eq!(
            tick_one_line(input),
            want.map(String::from),
            "input {}",
            input
        );
    }
}

#[test]
fn rewrite_preserves_crlf_and_no_trailing_newline() {
    let text = "- [ ] a\r\n- [ ] b";
    let out = rewrite_lines(text, &[0]);
    assert_eq!(out, "- [x] a\r\n- [ ] b");
}

#[test]
fn rewrite_nested_and_already_ticked() {
    let text = "- [ ] parent\n  - [ ] child\n- [x] done\n";
    let out = rewrite_lines(text, &[0, 1]);
    assert_eq!(out, "- [x] parent\n  - [x] child\n- [x] done\n");
}

#[test]
fn rewrite_markdown_link_not_ticked() {
    let text = "- [a](b) link\n- [ ] box\n";
    let out = rewrite_lines(text, &[1]);
    assert_eq!(out, "- [a](b) link\n- [x] box\n");
}

#[test]
fn select_tick_sequential_ordinals_match_literal_and_position() {
    let text = "## Task 1\n- [ ] a\n\n## Task 2\n- [ ] b\n\n## Task 3\n- [ ] c\n";
    let committed: BTreeSet<usize> = [2].into_iter().collect();
    let lines = select_tick_lines(text, &committed);
    let ticked = text.split_inclusive('\n').enumerate()
        .filter(|(i, _)| lines.contains(i)).map(|(_, l)| l).collect::<String>();
    assert_eq!(ticked, "- [ ] b\n");
}

#[test]
fn select_tick_renumbered_ordinals_are_positional() {
    let text = "## Task 1\n- [ ] a\n\n## Task 4\n- [ ] b\n\n## Task 9\n- [ ] c\n";
    let committed: BTreeSet<usize> = [2].into_iter().collect();
    let lines = select_tick_lines(text, &committed);
    let ticked = text.split_inclusive('\n').enumerate()
        .filter(|(i, _)| lines.contains(i)).map(|(_, l)| l).collect::<String>();
    assert_eq!(ticked, "- [ ] b\n");
}

#[test]
fn select_tick_repeated_heading_text_ticks_only_second_block() {
    let text = "## Task 2\n- [ ] first\n\n## Task 2\n- [ ] second\n";
    let committed: BTreeSet<usize> = [2].into_iter().collect();
    let lines = select_tick_lines(text, &committed);
    let ticked = text.split_inclusive('\n').enumerate()
        .filter(|(i, _)| lines.contains(i)).map(|(_, l)| l).collect::<String>();
    assert_eq!(ticked, "- [ ] second\n");
}

#[test]
fn select_tick_missing_ordinal_changes_nothing() {
    let text = "## Task 1\n- [ ] a\n\n## Task 2\n- [ ] b\n";
    let committed: BTreeSet<usize> = [99].into_iter().collect();
    let lines = select_tick_lines(text, &committed);
    assert!(lines.is_empty());
    let out = rewrite_lines(text, &lines);
    assert_eq!(out, text);
}
