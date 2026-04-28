use assert_cmd::Command;

#[test]
fn cli_help_shows_run_subcommand() {
    let mut cmd = Command::cargo_bin("monorail").unwrap();
    let assert = cmd.arg("--help").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("run"), "help missing 'run': {out}");
}

#[test]
fn run_without_ticket_fails() {
    let mut cmd = Command::cargo_bin("monorail").unwrap();
    cmd.arg("run").assert().failure();
}

#[test]
fn run_invalid_ticket_format_fails_fast() {
    let mut cmd = Command::cargo_bin("monorail").unwrap();
    cmd.env("LINEAR_API_KEY", "dummy")
        .arg("run")
        .arg("not-a-ticket")
        .assert()
        .failure();
}
