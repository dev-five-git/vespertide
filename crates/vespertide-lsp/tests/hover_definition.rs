use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, compute_definition, compute_hover,
};

mod common;
use common::uri;

#[test]
fn hover_on_fk_ref_table_previews_target_columns() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let user_uri = uri("user.json");
    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(user_uri, "json".to_string(), 1, user_src.to_string());

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#""ref_table":"user""#).unwrap() + 14;
    let hover = compute_hover(post_src, post_tree.as_ref(), &idx, &docs, pos).unwrap();

    assert!(hover.markdown.contains("Target table"));
    assert!(hover.markdown.contains("columns: id"));
}

#[test]
fn cross_file_definition_resolves() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let user_uri = uri("user.json");
    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    idx.upsert(&user_uri, user_src, &user_tree);
    docs.open(
        user_uri.clone(),
        "json".to_string(),
        1,
        user_src.to_string(),
    );

    let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json);
    let pos = post_src.find(r#""ref_table":"user""#).unwrap() + 14;
    let location = compute_definition(post_src, post_tree.as_ref(), &idx, &docs, pos);
    assert!(location.is_some());
    assert_eq!(location.unwrap().uri, user_uri);
}
