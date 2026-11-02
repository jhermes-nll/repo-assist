use serde_json::json;

#[test]
fn plans_tick_cli_exit_code_zero_on_dry_run_or_success() {
    let result = json!({"plans":[{"id":"p","apply_error":"Permission denied (os error 13)"}]});
    // Dry run never exits non-zero, even when an apply_error would be present.
    assert_eq!(repo_assist::cli::plans_tick_exit_code(&result, false), 0);
    // A successful apply (no apply_error) exits 0.
    let ok = json!({"plans":[{"id":"p"}]});
    assert_eq!(repo_assist::cli::plans_tick_exit_code(&ok, true), 0);
}

#[test]
fn plans_tick_cli_exit_code_one_when_apply_fails() {
    let result = json!({"plans":[{"id":"p","apply_error":"Permission denied (os error 13)"}]});
    assert_eq!(repo_assist::cli::plans_tick_exit_code(&result, true), 1);
}
