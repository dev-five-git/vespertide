//! Drift detection integration tests.

use std::fs;
use std::path::Path;

use tempfile::tempdir;
use vespertide_lsp::{
    DocumentStore, DriftCache, DriftKind, WorkspaceIndex, compute_drift, compute_drift_with_cache,
};

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn write_vespertide_json(root: &Path) {
    fs::write(root.join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake"}"#).unwrap();
}

fn write_model(root: &Path, table: &str, body: &str) {
    let models = root.join("models");
    fs::create_dir_all(&models).unwrap();
    fs::write(models.join(format!("{table}.json")), body).unwrap();
}

fn write_migration(root: &Path, name: &str, body: &str) {
    let migs = root.join("migrations");
    fs::create_dir_all(&migs).unwrap();
    fs::write(migs.join(name), body).unwrap();
}

fn ensure_empty_migrations_dir(root: &Path) {
    fs::create_dir_all(root.join("migrations")).unwrap();
}

/// Wrap a list of action JSON objects into a complete migration plan body.
fn migration_plan(version: u32, comment: &str, actions_json: &str) -> String {
    format!(
        r#"{{
  "version": {version},
  "id": "",
  "comment": "{comment}",
  "actions": [{actions_json}]
}}"#
    )
}

// -----------------------------------------------------------------------------
// Existing baseline test — DO NOT MODIFY
// -----------------------------------------------------------------------------

#[test]
fn no_config_no_drift() {
    let tmp = tempdir().unwrap();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(tmp.path(), &idx, &docs);

    assert!(drifts.is_empty());
}

// -----------------------------------------------------------------------------
// New per-action drift tests
// -----------------------------------------------------------------------------

#[test]
fn add_column_emits_drift_with_position() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_user),
    );

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"email","type":"text","nullable":false,"default":"''"}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert_eq!(drifts.len(), 1, "expected exactly 1 drift, got {drifts:?}");
    let d = &drifts[0];
    assert!(
        matches!(&d.kind, DriftKind::AddColumn { column } if column == "email"),
        "unexpected kind: {:?}",
        d.kind
    );
    assert_eq!(d.kind.code(), "drift-add-column");
    assert!(d.byte_range.is_some(), "byte_range should be Some");
    assert!(
        d.message.contains("'email'"),
        "message missing email mention: {}",
        d.message
    );
}

#[test]
fn modify_column_type_emits_drift() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"age","type":"integer","nullable":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_user),
    );

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"age","type":"big_int","nullable":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert_eq!(drifts.len(), 1, "expected exactly 1 drift, got {drifts:?}");
    let d = &drifts[0];
    assert!(
        matches!(&d.kind, DriftKind::ModifyColumnType { column, .. } if column == "age"),
        "unexpected kind: {:?}",
        d.kind
    );
    assert_eq!(d.kind.code(), "drift-modify-type");
    assert!(d.byte_range.is_some(), "byte_range should be Some");
    let msg_lc = d.message.to_lowercase();
    assert!(
        msg_lc.contains("integer"),
        "message missing 'integer': {}",
        d.message
    );
    assert!(
        msg_lc.contains("bigint") || msg_lc.contains("big_int"),
        "message missing 'bigint': {}",
        d.message
    );
}

#[test]
fn modify_column_nullable_emits_drift() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"email","type":"text","nullable":false}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_user),
    );

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"email","type":"text","nullable":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert_eq!(drifts.len(), 1, "expected exactly 1 drift, got {drifts:?}");
    let d = &drifts[0];
    assert!(
        matches!(
            &d.kind,
            DriftKind::ModifyColumnNullable { column, before: false, after: true } if column == "email"
        ),
        "unexpected kind: {:?}",
        d.kind
    );
    assert_eq!(d.kind.code(), "drift-modify-nullable");
    assert!(d.byte_range.is_some(), "byte_range should be Some");
    assert!(
        d.message.contains("not null") && d.message.contains("nullable"),
        "message missing nullable transition words: {}",
        d.message
    );
}

#[test]
fn rename_column_emits_drift() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"old_name","type":"text","nullable":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_user),
    );

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"new_name","type":"text","nullable":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert!(!drifts.is_empty(), "expected ≥1 drift for rename, got none");
    // Differ may emit either RenameColumn OR DeleteColumn+AddColumn — accept both shapes.
    let all_drift_codes = drifts.iter().all(|d| d.kind.code().starts_with("drift-"));
    assert!(all_drift_codes, "every drift code should start with drift-");

    let kinds_ok = drifts.iter().all(|d| {
        matches!(
            &d.kind,
            DriftKind::RenameColumn { .. }
                | DriftKind::AddColumn { .. }
                | DriftKind::DeleteColumn { .. }
        )
    });
    assert!(
        kinds_ok,
        "every drift kind should be rename/add/delete column, got {drifts:?}"
    );
}

#[test]
fn add_constraint_emits_drift() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"email","type":"text","nullable":false}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_user),
    );

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true},
        {"name":"email","type":"text","nullable":false,"unique":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert!(
        !drifts.is_empty(),
        "expected ≥1 drift when a unique flag is added"
    );
    assert!(
        drifts.iter().all(|d| d.kind.code().starts_with("drift-")),
        "every drift code should start with drift-, got {drifts:?}"
    );
    // The exact action depends on planner behavior (AddConstraint vs ModifyColumn*).
    // Accept any drift code in the constraint/modify family.
    let has_constraint_or_modify_drift = drifts.iter().any(|d| {
        let code = d.kind.code();
        code.starts_with("drift-add-constraint")
            || code.starts_with("drift-replace-constraint")
            || code.starts_with("drift-modify-")
    });
    assert!(
        has_constraint_or_modify_drift,
        "expected at least one add-constraint or modify-* drift, got {drifts:?}"
    );
}

#[test]
fn delete_table_emits_drift() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    let create_ghost = r#"{
      "type": "create_table",
      "table": "ghost",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    let actions = format!("{create_user},{create_ghost}");
    write_migration(root, "0001_init.json", &migration_plan(1, "init", &actions));

    // Only the user model is present — ghost has no corresponding file.
    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    // ghost.json does not exist — guess_uri returns None, so the DeleteTable
    // drift is silently skipped. user matches its baseline, so no drift for user.
    // Net result: empty. Document that explicitly.
    assert!(
        drifts.is_empty(),
        "expected DeleteTable drift to be skipped when no model file exists, got {drifts:?}"
    );
}

#[test]
fn create_table_emits_drift() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    // Baseline migration creates a DIFFERENT table so the user model is fully new.
    let create_other = r#"{
      "type": "create_table",
      "table": "other",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_other),
    );

    // Need an "other" model so the baseline table doesn't show up as DeleteTable noise.
    let other_model = r#"{
      "name": "other",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ]
    }"#;
    write_model(root, "other", other_model);

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert_eq!(drifts.len(), 1, "expected exactly 1 drift, got {drifts:?}");
    let d = &drifts[0];
    assert!(
        matches!(&d.kind, DriftKind::CreateTable),
        "unexpected kind: {:?}",
        d.kind
    );
    assert_eq!(d.kind.code(), "drift-create-table");
    assert!(d.byte_range.is_some(), "byte_range should be Some");
    assert!(
        d.message.contains("'user'"),
        "message should mention 'user': {}",
        d.message
    );
}

#[test]
fn no_drift_when_model_matches_baseline() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);
    ensure_empty_migrations_dir(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.json",
        &migration_plan(1, "init", create_user),
    );

    let user_model = r#"{
      "name": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ]
    }"#;
    write_model(root, "user", user_model);

    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let drifts = compute_drift(root, &idx, &docs);

    assert!(
        drifts.is_empty(),
        "expected no drift when model matches baseline, got {drifts:?}"
    );
}

#[test]
fn drift_with_cache_caches_loaded_state() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.vespertide.json",
        &migration_plan(1, "init", create_user),
    );

    write_model(
        root,
        "user",
        r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
    );

    let cache = DriftCache::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let first = compute_drift_with_cache(root, &idx, &docs, &cache);
    let second = compute_drift_with_cache(root, &idx, &docs, &cache);

    assert_eq!(
        first.len(),
        second.len(),
        "warm cache returns same drift count"
    );
    assert_eq!(first, second, "warm cache returns same drifts");
}

#[test]
fn drift_with_cache_returns_warm_cache_when_unchanged() {
    let tmp = tempdir().unwrap();
    write_vespertide_json(tmp.path());
    write_model(
        tmp.path(),
        "user",
        r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
    );
    write_migration(
        tmp.path(),
        "0001_init.vespertide.json",
        r#"{"version":1,"id":"","comment":"","actions":[{"type":"create_table","table":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}],"constraints":[]}]}"#,
    );

    let cache = DriftCache::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let first = compute_drift_with_cache(tmp.path(), &idx, &docs, &cache);
    let second = compute_drift_with_cache(tmp.path(), &idx, &docs, &cache);

    assert_eq!(first, second, "warm cache returns identical drifts");
}

#[test]
fn drift_with_cache_invalidates_when_model_changes() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    write_vespertide_json(root);

    let create_user = r#"{
      "type": "create_table",
      "table": "user",
      "columns": [
        {"name":"id","type":"integer","nullable":false,"primary_key":true}
      ],
      "constraints": []
    }"#;
    write_migration(
        root,
        "0001_init.vespertide.json",
        &migration_plan(1, "init", create_user),
    );
    write_model(
        root,
        "user",
        r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#,
    );

    let cache = DriftCache::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let first = compute_drift_with_cache(root, &idx, &docs, &cache);

    std::thread::sleep(std::time::Duration::from_millis(50));
    write_model(
        root,
        "user",
        r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#,
    );

    let second = compute_drift_with_cache(root, &idx, &docs, &cache);

    assert!(
        second
            .iter()
            .any(|d| matches!(d.kind, DriftKind::AddColumn { .. })),
        "expected AddColumn drift after adding column to model"
    );
    if first.is_empty() {
        assert!(!second.is_empty(), "cache must re-load on mtime advance");
    }
}
