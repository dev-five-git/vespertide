use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::cargo;
use rstest::rstest;
use tempfile::TempDir;

fn vespertide() -> Command {
    Command::new(cargo::cargo_bin!("vespertide"))
}

#[rstest]
#[case(1)]
#[case(49)]
#[case(50)]
#[case(200)]
fn export_output_is_byte_stable_across_rayon_thread_counts(#[case] table_count: usize) {
    let single_threaded = export_output(table_count, "1");
    let multi_threaded = export_output(table_count, "4");

    assert_eq!(single_threaded, multi_threaded);
}

fn export_output(table_count: usize, rayon_threads: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let temp_dir = TempDir::new().expect("create temp dir");
    write_project(temp_dir.path(), table_count);

    vespertide()
        .current_dir(temp_dir.path())
        .env("RAYON_NUM_THREADS", rayon_threads)
        .args(["export", "--orm", "seaorm", "--export-dir", "generated"])
        .assert()
        .success();

    read_output_tree(&temp_dir.path().join("generated"))
}

fn write_project(root: &Path, table_count: usize) {
    fs::write(
        root.join("vespertide.json"),
        r#"{
  "modelsDir": "models",
  "migrationsDir": "migrations",
  "tableNamingCase": "snake",
  "columnNamingCase": "snake",
  "modelFormat": "json",
  "migrationFormat": "json",
  "modelExportDir": "generated",
  "seaorm": {
    "extraEnumDerives": [],
    "vesperaSchemaType": false
  }
}"#,
    )
    .expect("write config");

    let models_dir = root.join("models");
    fs::create_dir(&models_dir).expect("create models dir");

    for index in 0..table_count {
        let table_name = format!("table_{index:03}");
        let model = format!(
            r#"{{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "{table_name}",
  "columns": [
    {{ "name": "id", "type": "integer", "nullable": false, "primary_key": {{ "auto_increment": true }} }},
    {{ "name": "name", "type": {{ "kind": "varchar", "length": 100 }}, "nullable": false }},
    {{ "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }}
  ]
}}"#
        );
        fs::write(models_dir.join(format!("{table_name}.json")), model).expect("write model");
    }
}

fn read_output_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
    let mut entries = fs::read_dir(dir)
        .expect("read output dir")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect output entries");
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            let rel_path = path
                .strip_prefix(root)
                .expect("relative path")
                .to_path_buf();
            let bytes = fs::read(&path).expect("read output file");
            files.push((rel_path, bytes));
        }
    }
}
