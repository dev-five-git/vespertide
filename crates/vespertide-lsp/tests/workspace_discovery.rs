//! Disk-backed workspace model discovery tests.

use std::fs;

use tempfile::tempdir;
use vespertide_lsp::WorkspaceTables;

#[test]
fn refresh_discovers_all_models_from_config() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");
    fs::create_dir_all(&models_dir).unwrap();
    fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();
    fs::write(models_dir.join("article.json"), r#"{"name":"article","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#).unwrap();

    let tables = WorkspaceTables::new();

    assert!(tables.refresh(tmp.path()));
    let names = tables.names();
    assert!(names.contains(&"article".to_string()), "names: {names:?}");
    assert!(names.contains(&"user".to_string()), "names: {names:?}");
    assert!(tables.get("user").is_some());
}
