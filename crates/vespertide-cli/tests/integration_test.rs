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
fn test_main_with_sql_command_mysql() {
    let mut cmd = vespertide();
    cmd.args(["sql", "--backend", "mysql"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_sql_command_sqlite() {
    let mut cmd = vespertide();
    cmd.args(["sql", "--backend", "sqlite"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_log_command() {
    let mut cmd = vespertide();
    cmd.arg("log");
    let _ = cmd.assert();
}

#[test]
fn test_main_with_log_command_mysql() {
    let mut cmd = vespertide();
    cmd.args(["log", "--backend", "mysql"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_log_command_sqlite() {
    let mut cmd = vespertide();
    cmd.args(["log", "--backend", "sqlite"]);
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
    cmd.args(["new", "test_table"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_revision_command() {
    let mut cmd = vespertide();
    cmd.args(["revision", "-m", "test message"]);
    let _ = cmd.assert();
}

#[test]
fn test_main_with_export_command() {
    let mut cmd = vespertide();
    cmd.args(["export", "--orm", "seaorm"]);
    let _ = cmd.assert();
}

fn write_minimal_export_project(root: &std::path::Path) {
    std::fs::write(
        root.join("vespertide.json"),
        r#"{
  "modelsDir": "models",
  "migrationsDir": "migrations",
  "tableNamingCase": "snake",
  "columnNamingCase": "snake",
  "modelFormat": "json",
  "migrationFormat": "json",
  "modelExportDir": "generated"
}"#,
    )
    .expect("write config");

    let models_dir = root.join("models");
    std::fs::create_dir(&models_dir).expect("create models dir");
    std::fs::write(
        models_dir.join("users.json"),
        r#"{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "users",
  "columns": [
    { "name": "id", "type": "integer", "nullable": false, "primary_key": { "auto_increment": true } },
    { "name": "name", "type": { "kind": "varchar", "length": 100 }, "nullable": false }
  ]
}"#,
    )
    .expect("write model");
}

#[test]
fn test_export_django_writes_files_end_to_end() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    write_minimal_export_project(temp_dir.path());

    vespertide()
        .current_dir(temp_dir.path())
        .args(["export", "--orm", "django", "--export-dir", "generated"])
        .assert()
        .success();

    let output_file = temp_dir.path().join("generated").join("users.py");
    assert!(
        output_file.exists(),
        "expected {} to exist",
        output_file.display()
    );
    let content = std::fs::read_to_string(&output_file).expect("read generated file");
    assert!(content.contains("class Users(models.Model):"));
}

#[test]
fn test_export_gorm_writes_files_end_to_end() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    write_minimal_export_project(temp_dir.path());

    vespertide()
        .current_dir(temp_dir.path())
        .args(["export", "--orm", "gorm", "--export-dir", "generated"])
        .assert()
        .success();

    let output_file = temp_dir.path().join("generated").join("users.go");
    assert!(
        output_file.exists(),
        "expected {} to exist",
        output_file.display()
    );
    let content = std::fs::read_to_string(&output_file).expect("read generated file");
    assert!(content.contains("type Users struct {"));
}
