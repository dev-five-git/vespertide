//! Coverage-closure integration tests that must remain cross-file or disk-backed.

use std::fs;
use std::str::FromStr;

use rstest::rstest;
use tempfile::tempdir;
use tower_lsp_server::ls_types::Uri;

use vespertide_lsp::diagnostics::WorkspaceTable;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, DriftCache, ParserPool, WorkspaceIndex, WorkspaceTables,
    compute_drift, compute_drift_with_cache, compute_workspace_diagnostics,
    compute_workspace_symbols,
};

mod common;
use common::uri;

fn setup_disk_workspace(tmp_dir: &std::path::Path, models: &[(&str, &str)]) {
    let models_dir = tmp_dir.join("models");
    fs::create_dir_all(&models_dir).unwrap();
    fs::write(tmp_dir.join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    for (name, content) in models {
        fs::write(models_dir.join(name), content).unwrap();
    }
}

#[rstest]
#[case::direct(false)]
#[case::cached(true)]
fn drift_compute_no_config_empty_cases(#[case] cached: bool) {
    let tmp = tempdir().unwrap();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    if cached {
        let cache = DriftCache::new();
        let first = compute_drift_with_cache(tmp.path(), &idx, &docs, &cache);
        let second = compute_drift_with_cache(tmp.path(), &idx, &docs, &cache);
        assert_eq!(first, second);
    } else {
        assert!(
            compute_drift(tmp.path(), &idx, &docs).is_empty(),
            "no vespertide.json → no drift"
        );
    }
}

#[test]
fn drift_detects_added_table_in_model_not_in_migrations() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");
    let migrations_dir = tmp.path().join("migrations");
    fs::create_dir_all(&models_dir).unwrap();
    fs::create_dir_all(&migrations_dir).unwrap();
    fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(tmp.path(), &idx, &docs);

    assert!(
        !drifts.is_empty(),
        "model has user, migrations have nothing → drift"
    );
}

#[test]
fn drift_returns_empty_when_planner_diff_rejects_loaded_models() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");
    let migrations_dir = tmp.path().join("migrations");
    fs::create_dir_all(&models_dir).unwrap();
    fs::create_dir_all(&migrations_dir).unwrap();
    fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    fs::write(
        models_dir.join("bad.json"),
        r#"{"name":"bad","columns":[{"name":"id","type":"integer"},{"name":"id","type":"text"}]}"#,
    )
    .unwrap();

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let cache = DriftCache::new();
    let drifts = compute_drift_with_cache(tmp.path(), &idx, &docs, &cache);

    assert!(
        drifts.is_empty(),
        "diff_schemas error should be best-effort empty drift"
    );
}

#[test]
fn workspace_symbols_includes_disk_tables_when_unopened() {
    let tmp = tempdir().unwrap();
    setup_disk_workspace(
        tmp.path(),
        &[(
            "product.json",
            r#"{"name":"product","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"sku","type":"text","nullable":false}]}"#,
        )],
    );
    let tables = WorkspaceTables::new();
    assert!(tables.refresh(tmp.path()));

    let docs = DocumentStore::new();
    let symbols = compute_workspace_symbols("", &docs, Some(&tables));
    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();

    assert!(names.contains(&"product"), "got: {names:?}");
    assert!(names.contains(&"sku"));
}

#[test]
fn workspace_symbols_open_doc_takes_priority_over_disk() {
    let tmp = tempdir().unwrap();
    setup_disk_workspace(
        tmp.path(),
        &[(
            "user.json",
            r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
        )],
    );
    let tables = WorkspaceTables::new();
    assert!(tables.refresh(tmp.path()));

    let docs = DocumentStore::new();
    let model_path = tmp.path().join("models").join("user.json");
    let path_str = model_path.to_string_lossy().replace('\\', "/");
    let prefix = if path_str.starts_with('/') { "" } else { "/" };
    let u = Uri::from_str(&format!("file://{prefix}{path_str}")).unwrap();
    docs.open(u, "json".to_string(), 1, r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#.to_string());

    let symbols = compute_workspace_symbols("", &docs, Some(&tables));
    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();

    assert!(names.contains(&"email"));
}

#[test]
fn workspace_diagnostics_emits_filename_mismatch_warning() {
    let pool = ParserPool::new();
    let u = uri("not_user.json");
    let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json).unwrap();
    let entry = WorkspaceTable {
        uri: u.clone(),
        table: serde_json::from_str(src).expect("parse"),
        source: src.to_string(),
        tree: Some(tree.clone()),
    };
    let diags = compute_workspace_diagnostics(src, DocumentFormat::Json, Some(&tree), &[entry], &u);

    assert!(
        diags.iter().any(|d| d.code == "filename-mismatch"),
        "expected filename-mismatch warning, got: {diags:?}"
    );
}

#[test]
fn workspace_diagnostics_cross_file_fk_target_missing() {
    use vespertide_core::TableDef;

    let pool = ParserPool::new();
    let post_src = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let other_src = r#"{"name":"unrelated","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let post_uri = uri("post.json");
    let other_uri = uri("unrelated.json");
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let other_tree = pool.parse(other_src, DocumentFormat::Json).unwrap();

    let post_table: TableDef = serde_json::from_str(post_src)
        .and_then(|t: TableDef| {
            t.normalize()
                .map_err(|e| serde::de::Error::custom(e.to_string()))
        })
        .unwrap();
    let other_table: TableDef = serde_json::from_str(other_src)
        .and_then(|t: TableDef| {
            t.normalize()
                .map_err(|e| serde::de::Error::custom(e.to_string()))
        })
        .unwrap();
    let workspace = vec![
        WorkspaceTable {
            uri: post_uri.clone(),
            table: post_table,
            source: post_src.to_string(),
            tree: Some(post_tree.clone()),
        },
        WorkspaceTable {
            uri: other_uri,
            table: other_table,
            source: other_src.to_string(),
            tree: Some(other_tree),
        },
    ];
    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Json,
        Some(&post_tree),
        &workspace,
        &post_uri,
    );

    assert!(
        diags.iter().any(|d| d.code == "validate-schema"),
        "missing FK target should produce validate-schema, got: {diags:?}"
    );
}
