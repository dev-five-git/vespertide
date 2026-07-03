use std::collections::BTreeMap;

use serde_json::json;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::ls_types::{PrepareRenameResponse, RenameParams, TextDocumentPositionParams};

use super::harness::{
    POST_MODEL, POST_URI, TEXT_URI, UNKNOWN_URI, USER_MODEL, USER_URI, block_on_with_trace,
    make_service, open_doc, params, position, uri,
};

async fn open_user_and_post(backend: &super::super::Backend) {
    open_doc(backend, &uri(USER_URI), "json", USER_MODEL).await;
    open_doc(backend, &uri(POST_URI), "json", POST_MODEL).await;
}

#[test]
fn traced_rename_handlers_cover_logging_fields() {
    block_on_with_trace(async {
        let (service, _socket) = make_service();
        let backend = service.inner();
        open_user_and_post(backend).await;

        assert!(backend.prepare_rename(params::<TextDocumentPositionParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(USER_MODEL, r#""name":"user""#, 9) }))).await.unwrap().is_some());
        assert!(backend.prepare_rename(params::<TextDocumentPositionParams>(json!({ "textDocument": { "uri": USER_URI }, "position": { "line": 0, "character": 0 } }))).await.unwrap().is_none());
        assert!(backend.rename(params::<RenameParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(USER_MODEL, r#""name":"user""#, 9), "newName": "account" }))).await.unwrap().is_some());
    });
}

#[test]
fn prepare_rename_returns_placeholder_and_none_paths() {
    block_on_with_trace(async {
        let (service, _socket) = make_service();
        let backend = service.inner();
        open_user_and_post(backend).await;

        let prepared = backend
            .prepare_rename(params::<TextDocumentPositionParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": position(USER_MODEL, r#""name":"user""#, 9),
            })))
            .await
            .unwrap()
            .expect("top-level table name should be renameable");
        let PrepareRenameResponse::RangeWithPlaceholder { placeholder, range } = prepared else {
            panic!("prepareRename should include a placeholder");
        };
        assert_eq!(placeholder, "user");
        assert!(range.end.character > range.start.character);

        let outside_symbol = backend
            .prepare_rename(params::<TextDocumentPositionParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": { "line": 0, "character": 0 },
            })))
            .await
            .unwrap();
        assert!(outside_symbol.is_none());

        let missing_doc = backend
            .prepare_rename(params::<TextDocumentPositionParams>(json!({
                "textDocument": { "uri": UNKNOWN_URI },
                "position": { "line": 0, "character": 0 },
            })))
            .await
            .unwrap();
        assert!(missing_doc.is_none());

        let unsupported = backend
            .prepare_rename(params::<TextDocumentPositionParams>(json!({
                "textDocument": { "uri": TEXT_URI },
                "position": { "line": 0, "character": 0 },
            })))
            .await
            .unwrap();
        assert!(unsupported.is_none());
    });
}

#[test]
fn rename_returns_workspace_edit_and_none_paths() {
    block_on_with_trace(async {
        let (service, _socket) = make_service();
        let backend = service.inner();
        open_user_and_post(backend).await;

        let edit = backend
            .rename(params::<RenameParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": position(USER_MODEL, r#""name":"user""#, 9),
                "newName": "account",
            })))
            .await
            .unwrap()
            .expect("renaming the table should produce a workspace edit");
        let changes = edit.changes.expect("workspace edit should use changes map");
        assert!(changes.contains_key(&uri(USER_URI)));
        assert!(changes.contains_key(&uri(POST_URI)));
        assert!(
            changes
                .values()
                .flatten()
                .all(|edit| edit.new_text == "account")
        );

        let same_name = backend
            .rename(params::<RenameParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": position(USER_MODEL, r#""name":"user""#, 9),
                "newName": "user",
            })))
            .await
            .unwrap();
        assert!(same_name.is_none());

        let outside_symbol = backend
            .rename(params::<RenameParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": { "line": 0, "character": 0 },
                "newName": "account",
            })))
            .await
            .unwrap();
        assert!(outside_symbol.is_none());

        let unsupported = backend
            .rename(params::<RenameParams>(json!({
                "textDocument": { "uri": TEXT_URI },
                "position": { "line": 0, "character": 0 },
                "newName": "account",
            })))
            .await
            .unwrap();
        assert!(unsupported.is_none());

        let missing_doc = backend
            .rename(params::<RenameParams>(json!({
                "textDocument": { "uri": UNKNOWN_URI },
                "position": { "line": 0, "character": 0 },
                "newName": "account",
            })))
            .await
            .unwrap();
        assert!(missing_doc.is_none());
    });
}

#[test]
fn rename_column_rewrites_check_expression_references() {
    block_on_with_trace(async {
        let (service, _socket) = make_service();
        let backend = service.inner();
        open_user_and_post(backend).await;

        let prepared = backend
            .prepare_rename(params::<TextDocumentPositionParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": position(USER_MODEL, r#""name":"age""#, 9),
            })))
            .await
            .unwrap()
            .expect("column declaration should be renameable");
        let PrepareRenameResponse::RangeWithPlaceholder { placeholder, range } = prepared else {
            panic!("prepareRename should include the column placeholder");
        };
        assert_eq!(placeholder, "age");
        assert!(range.end.character > range.start.character);

        let edit = backend
            .rename(params::<RenameParams>(json!({
                "textDocument": { "uri": USER_URI },
                "position": position(USER_MODEL, r#""name":"age""#, 9),
                "newName": "years",
            })))
            .await
            .unwrap()
            .expect("renaming a column should produce declaration and CHECK edits");
        let changes = edit.changes.expect("workspace edit should use changes map");
        let user_edits = changes
            .get(&uri(USER_URI))
            .expect("column rename edits should target the declaring model");
        assert!(
            user_edits.len() >= 2,
            "column declaration plus CHECK expression reference should be edited: {user_edits:?}"
        );
        assert!(user_edits.iter().all(|edit| edit.new_text == "years"));
    });
}

#[test]
fn lowered_rename_changes_skips_unlowerable_targets_and_empty_results() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let open_uri = uri(USER_URI);
    backend.store.open(
        open_uri.clone(),
        "json".to_string(),
        1,
        USER_MODEL.to_string(),
    );

    let mut mixed = BTreeMap::new();
    mixed.insert(
        open_uri.clone(),
        vec![crate::rename::DomainTextEdit {
            byte_range: 0..1,
            new_text: "x".to_string(),
        }],
    );
    mixed.insert(
        uri("file:///workspace/missing.json"),
        vec![crate::rename::DomainTextEdit {
            byte_range: 0..1,
            new_text: "x".to_string(),
        }],
    );

    let changes = super::super::handler_rename::lowered_rename_changes(mixed, backend)
        .expect("open-document edit should survive lowering");
    assert_eq!(changes.len(), 1);
    assert!(changes.contains_key(&open_uri));

    let mut missing_only = BTreeMap::new();
    missing_only.insert(
        uri("file:///workspace/missing.json"),
        vec![crate::rename::DomainTextEdit {
            byte_range: 0..1,
            new_text: "x".to_string(),
        }],
    );
    assert!(super::super::handler_rename::lowered_rename_changes(missing_only, backend).is_none());
}
