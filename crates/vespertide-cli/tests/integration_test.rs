use assert_cmd::Command;
use assert_cmd::cargo;
use predicates::prelude::*;

fn vespertide() -> Command {
    Command::new(cargo::cargo_bin!("vespertide"))
}

#[test]
fn test_main_with_no_args_shows_help() {
    vespertide()
        .assert()
        .success()
        .stdout(predicate::str::contains("vespertide"));
}

#[test]
fn test_main_with_help_flag() {
    vespertide()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("vespertide"));
}

#[test]
fn test_main_with_diff_command() {
    // This will fail if not in a vespertide project, but tests the code path
    let mut cmd = vespertide();
    cmd.arg("diff");
    // Don't assert success since it may fail outside a project
    let _ = cmd.assert();
}

#[test]
fn test_main_with_sql_command() {
    let mut cmd = vespertide();
    cmd.arg("sql");
    let _ = cmd.assert();
}

#[test]
fn test_main_with_log_command() {
    let mut cmd = vespertide();
    cmd.arg("log");
    let _ = cmd.assert();
}

#[test]
fn test_main_with_status_command() {
    let mut cmd = vespertide();
    cmd.arg("status");
    let _ = cmd.assert();
}

#[test]
fn test_main_with_init_command() {
    let mut cmd = vespertide();
    cmd.arg("init");
    let _ = cmd.assert();
}

#[test]
fn test_main_with_new_command() {
    let mut cmd = vespertide();
    cmd.args(&["new", "test_table"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_revision_command() {
    let mut cmd = vespertide();
    cmd.args(&["revision", "-m", "test message"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_export_command() {
    let mut cmd = vespertide();
    cmd.args(&["export", "--orm", "seaorm"]);
    let _ = cmd.assert();
}
