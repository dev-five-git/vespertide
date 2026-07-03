use tempfile::tempdir;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, WorkspaceTables, compute_completion,
    compute_completion_with_workspace_tables,
};

mod common;
use common::uri;

#[test]
fn cross_file_ref_columns() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_uri = uri("user.json");
    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(user_uri, "json".to_string(), 1, user_src.to_string());

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":[""]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#"["""#).unwrap() + 2;
    let items = compute_completion(
        post_src,
        DocumentFormat::Json,
        post_tree.as_ref(),
        &idx,
        &docs,
        pos,
    );
    let labels = items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(
        labels.contains(&"id"),
        "should suggest 'id' column. got: {labels:?}"
    );
    assert!(
        labels.contains(&"email"),
        "should suggest 'email' column. got: {labels:?}"
    );
}

#[test]
fn disk_workspace_tables_feed_ref_column_completion() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");
    std::fs::create_dir_all(&models_dir).unwrap();
    std::fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    std::fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#).unwrap();

    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let disk_tables = WorkspaceTables::new();
    assert!(disk_tables.refresh(tmp.path()));

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":[""]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#"["""#).unwrap() + 2;
    let items = compute_completion_with_workspace_tables(
        post_src,
        DocumentFormat::Json,
        post_tree.as_ref(),
        &idx,
        &docs,
        &disk_tables,
        pos,
    );
    let labels = items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"id"), "labels: {labels:?}");
    assert!(labels.contains(&"email"), "labels: {labels:?}");
}

#[test]
fn yaml_ref_table_offers_workspace_tables() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_uri = uri("user.yaml");
    let user_src = "name: user\ncolumns:\n  - name: id\n    type: integer\n    primary_key: true\n";
    let user_tree = pool.parse(user_src, DocumentFormat::Yaml).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);

    let post_src = "name: post\ncolumns:\n  - name: author_id\n    type: integer\n    foreign_key:\n      ref_table: \"\"\n      ref_columns: [id]\n";
    let post_tree = pool.parse(post_src, DocumentFormat::Yaml);
    let pos = post_src.find(r#"ref_table: """#).unwrap() + 12;
    let items = compute_completion(
        post_src,
        DocumentFormat::Yaml,
        post_tree.as_ref(),
        &idx,
        &docs,
        pos,
    );

    assert!(
        items.iter().any(|i| i.label == "user"),
        "YAML ref_table should suggest workspace tables, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}
