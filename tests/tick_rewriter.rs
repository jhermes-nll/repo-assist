mod common;

use repo_assist::plans::tick::{rewrite_lines, tick_one_line};

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
