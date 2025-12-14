use assert_cmd::Command;
use assert_cmd::cargo;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_main_with_default_output() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path().join("schemas");

    Command::new(cargo::cargo_bin!("vespertide-schema-gen"))
        .arg("--out")
        .arg(out_dir.as_os_str())
        .assert()
        .success()
        .stdout(predicate::str::contains("Wrote schemas:"));

    assert!(out_dir.exists());
    assert!(out_dir.join("model.schema.json").exists());
    assert!(out_dir.join("migration.schema.json").exists());
}

#[test]
fn test_main_with_custom_output() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path().join("custom_schemas");

    Command::new(cargo::cargo_bin!("vespertide-schema-gen"))
        .arg("-o")
        .arg(out_dir.as_os_str())
        .assert()
        .success()
        .stdout(predicate::str::contains("Wrote schemas:"));

    assert!(out_dir.exists());
    assert!(out_dir.join("model.schema.json").exists());
    assert!(out_dir.join("migration.schema.json").exists());
}

#[test]
fn test_main_creates_directory_if_not_exists() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path().join("new_dir").join("schemas");

    assert!(!out_dir.exists());

    Command::new(cargo::cargo_bin!("vespertide-schema-gen"))
        .arg("--out")
        .arg(out_dir.as_os_str())
        .assert()
        .success();

    assert!(out_dir.exists());
}

#[test]
fn test_main_with_help_flag() {
    Command::new(cargo::cargo_bin!("vespertide-schema-gen"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("vespertide-schema-gen"));
}
