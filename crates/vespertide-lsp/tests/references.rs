//! Integration tests for find-references.

use std::fs;
use std::str::FromStr;

use tempfile::tempdir;
use tower_lsp_server::ls_types::Uri;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, WorkspaceTables, compute_references,
};

mod common;
use common::uri;

#[test]
fn references_for_table_find_cross_file_ref_table_usages() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_src =
        r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
    let user_uri = uri("user.json");
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(
        user_uri.clone(),
        "json".to_string(),
        1,
        user_src.to_string(),
    );

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let post_uri = uri("post.json");
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    idx.upsert(&post_uri, post_src, &post_tree);
    docs.open(
        post_uri.clone(),
        "json".to_string(),
        1,
        post_src.to_string(),
    );

    let comment_src = r#"{"name":"comment","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let comment_uri = uri("comment.json");
    let comment_tree = pool.parse(comment_src, DocumentFormat::Json).unwrap();
    idx.upsert(&comment_uri, comment_src, &comment_tree);
    docs.open(
        comment_uri.clone(),
        "json".to_string(),
        1,
        comment_src.to_string(),
    );

    // Cursor on user's top-level `name` value.
    let pos = user_src.find(r#""name":"user""#).unwrap() + 9;
    let refs = compute_references(
        user_src,
        DocumentFormat::Json,
        Some(&user_tree),
        &user_uri,
        &idx,
        &docs,
        None,
        pos,
        true,
    );

    // Should return: user.json declaration + post.json ref_table + comment.json ref_table.
    let uris: Vec<String> = refs.iter().map(|r| r.uri.as_str().to_string()).collect();
    assert!(
        uris.iter().any(|u| u.ends_with("/user.json")),
        "declaration missing, got: {uris:?}"
    );
    assert!(
        uris.iter().any(|u| u.ends_with("/post.json")),
        "post.json ref missing, got: {uris:?}"
    );
    assert!(
        uris.iter().any(|u| u.ends_with("/comment.json")),
        "comment.json ref missing, got: {uris:?}"
    );
}

#[test]
fn references_for_column_find_cross_file_ref_columns_only_for_matching_table() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true},{"name":"email","type":"text"}]}"#;
    let user_uri = uri("user.json");
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(
        user_uri.clone(),
        "json".to_string(),
        1,
        user_src.to_string(),
    );

    // post.json references user.email — this should match.
    let post_src = r#"{"name":"post","columns":[{"name":"author_email","type":"text","foreign_key":{"ref_table":"user","ref_columns":["email"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let post_uri = uri("post.json");
    idx.upsert(&post_uri, post_src, &post_tree);
    docs.open(
        post_uri.clone(),
        "json".to_string(),
        1,
        post_src.to_string(),
    );

    // other.json has a column literally named "email" but it's not a FK to user — must NOT match.
    let other_src = r#"{"name":"other","columns":[{"name":"email","type":"text"}]}"#;
    let other_uri = uri("other.json");
    let other_tree = pool.parse(other_src, DocumentFormat::Json).unwrap();
    idx.upsert(&other_uri, other_src, &other_tree);
    docs.open(
        other_uri.clone(),
        "json".to_string(),
        1,
        other_src.to_string(),
    );

    // Cursor on user.email column name (declaration).
    let pos = user_src.find(r#""name":"email""#).unwrap() + 10;
    let refs = compute_references(
        user_src,
        DocumentFormat::Json,
        Some(&user_tree),
        &user_uri,
        &idx,
        &docs,
        None,
        pos,
        true,
    );

    let by_uri: Vec<_> = refs.iter().map(|r| r.uri.as_str().to_string()).collect();
    assert!(
        by_uri.iter().any(|u| u.ends_with("/post.json")),
        "should find FK use in post.json, got: {by_uri:?}"
    );
    // other.json has a same-named column but belongs to a different table —
    // it MUST NOT appear (the symbol is qualified as `user.email`).
    let other_count = by_uri.iter().filter(|u| u.ends_with("/other.json")).count();
    assert_eq!(
        other_count, 0,
        "unrelated `other.email` column must not be reported, got: {by_uri:?}"
    );
}

#[test]
fn references_include_disk_only_target_files() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");
    fs::create_dir_all(&models_dir).unwrap();
    fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();
    // Disk-only post file references user.
    fs::write(models_dir.join("post.json"), r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#).unwrap();

    let disk = WorkspaceTables::new();
    assert!(disk.refresh(tmp.path()));

    // The user file IS opened in the editor; post is disk-only.
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let user_src =
        r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
    let user_uri_path = models_dir.join("user.json");
    let user_uri = Uri::from_str(&format!(
        "file:///{}",
        user_uri_path.to_string_lossy().replace('\\', "/")
    ))
    .unwrap();
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(
        user_uri.clone(),
        "json".to_string(),
        1,
        user_src.to_string(),
    );

    let pos = user_src.find(r#""name":"user""#).unwrap() + 9;
    let refs = compute_references(
        user_src,
        DocumentFormat::Json,
        Some(&user_tree),
        &user_uri,
        &idx,
        &docs,
        Some(&disk),
        pos,
        true,
    );
    let uris: Vec<_> = refs.iter().map(|r| r.uri.as_str().to_string()).collect();
    assert!(
        uris.iter().any(|u| u.contains("post.json")),
        "disk-only post.json reference should be reported, got: {uris:?}"
    );
}

#[test]
fn references_yaml_cross_file_table() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    let user_src = "name: user\ncolumns:\n  - name: id\n    type: integer\n    primary_key: true\n";
    let user_uri = uri("user.yaml");
    let user_tree = pool.parse(user_src, DocumentFormat::Yaml).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(
        user_uri.clone(),
        "yaml".to_string(),
        1,
        user_src.to_string(),
    );

    let post_src = "name: post\ncolumns:\n  - name: author_id\n    type: integer\n    foreign_key:\n      ref_table: user\n      ref_columns: [id]\n";
    let post_uri = uri("post.yaml");
    let post_tree = pool.parse(post_src, DocumentFormat::Yaml).unwrap();
    idx.upsert(&post_uri, post_src, &post_tree);
    docs.open(
        post_uri.clone(),
        "yaml".to_string(),
        1,
        post_src.to_string(),
    );

    let pos = user_src.find("name: user").unwrap() + 6;
    let refs = compute_references(
        user_src,
        DocumentFormat::Yaml,
        Some(&user_tree),
        &user_uri,
        &idx,
        &docs,
        None,
        pos,
        true,
    );

    let uris: Vec<_> = refs.iter().map(|r| r.uri.as_str().to_string()).collect();
    assert!(
        uris.iter().any(|u| u.ends_with("/post.yaml")),
        "YAML cross-file ref should appear, got: {uris:?}"
    );
}
