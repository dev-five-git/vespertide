use std::fs;

use serde_json::json;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::ls_types::{
    CodeActionParams, CompletionParams, GotoDefinitionParams, InitializeParams, InitializedParams,
    InlayHintParams, ReferenceParams, RenameParams, TextDocumentPositionParams,
    WorkspaceSymbolParams,
};

use super::harness::{
    POST_MODEL, POST_URI, TEXT_URI, USER_MODEL, USER_URI, block_on_with_trace, did_open_params,
    full_range, make_service, open_doc, params, position, uri, workspace_config, workspace_fixture,
};

const CHANGED_USER_MODEL: &str = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false,"unique":true},{"name":"age","type":"integer","nullable":false},{"name":"nickname","type":"text","nullable":true}],"constraints":[{"type":"check","name":"chk_age","expr":"age BETWEEN 0 AND 100"}]}"#;
const PARTIAL_TYPE_MODEL: &str =
    r#"{"name":"user","columns":[{"name":"id","type":"i","nullable":false}]}"#;

#[test]
fn final_mile_lifecycle_drives_publish_and_notification_logging_fields() {
    block_on_with_trace(async {
        let fixture = workspace_fixture();
        let (service, _socket) = make_service();
        let backend = service.inner();

        backend
            .initialize(params::<InitializeParams>(json!({
                "capabilities": {},
                "clientInfo": { "name": "final-mile", "version": "1" },
                "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
            })))
            .await
            .unwrap();
        backend
            .initialized(params::<InitializedParams>(json!({})))
            .await;
        backend
            .did_open(did_open_params(&fixture.user_uri, "json", 1, USER_MODEL))
            .await;
        backend
            .did_change(params(json!({
                "textDocument": { "uri": fixture.user_uri.as_str(), "version": 2 },
                "contentChanges": [{ "text": CHANGED_USER_MODEL }],
            })))
            .await;
        backend
            .did_save(params(
                json!({ "textDocument": { "uri": fixture.user_uri.as_str() } }),
            ))
            .await;
        backend
            .did_change_watched_files(params(
                json!({ "changes": [{ "uri": fixture.user_uri.as_str(), "type": 2 }] }),
            ))
            .await;
        backend
            .did_close(params(
                json!({ "textDocument": { "uri": fixture.user_uri.as_str() } }),
            ))
            .await;
    });
}

#[test]
fn final_mile_navigation_and_rename_logging_fields_are_enabled() {
    block_on_with_trace(async {
        let (service, _socket) = make_service();
        let backend = service.inner();
        open_doc(backend, &uri(USER_URI), "json", PARTIAL_TYPE_MODEL).await;

        assert!(backend.completion(params::<CompletionParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(PARTIAL_TYPE_MODEL, r#""type":"i""#, 10) }))).await.unwrap().is_some());
        assert!(backend.completion(params::<CompletionParams>(json!({ "textDocument": { "uri": TEXT_URI }, "position": { "line": 0, "character": 0 } }))).await.unwrap().is_none());
        open_doc(backend, &uri(USER_URI), "json", USER_MODEL).await;
        open_doc(backend, &uri(POST_URI), "json", POST_MODEL).await;
        assert!(backend.goto_definition(params::<GotoDefinitionParams>(json!({ "textDocument": { "uri": POST_URI }, "position": position(POST_MODEL, r#""ref_table":"user""#, 14) }))).await.unwrap().is_some());
        assert!(backend.goto_definition(params::<GotoDefinitionParams>(json!({ "textDocument": { "uri": POST_URI }, "position": { "line": 0, "character": 0 } }))).await.unwrap().is_none());
        assert!(backend.references(params::<ReferenceParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(USER_MODEL, r#""name":"user""#, 9), "context": { "includeDeclaration": true } }))).await.unwrap().is_some());
        assert!(backend.code_action(params::<CodeActionParams>(json!({ "textDocument": { "uri": USER_URI }, "range": { "start": position(USER_MODEL, r#""name":"id""#, 9), "end": position(USER_MODEL, r#""name":"id""#, 9) }, "context": { "diagnostics": [] } }))).await.unwrap().is_some());
        assert!(
            backend
                .inlay_hint(params::<InlayHintParams>(
                    json!({ "textDocument": { "uri": USER_URI }, "range": full_range(USER_MODEL) })
                ))
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            backend
                .symbol(params::<WorkspaceSymbolParams>(json!({ "query": "user" })))
                .await
                .unwrap()
                .is_some()
        );
        assert!(backend.prepare_rename(params::<TextDocumentPositionParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(USER_MODEL, r#""name":"user""#, 9) }))).await.unwrap().is_some());
        assert!(backend.prepare_rename(params::<TextDocumentPositionParams>(json!({ "textDocument": { "uri": USER_URI }, "position": { "line": 0, "character": 0 } }))).await.unwrap().is_none());
        assert!(backend.rename(params::<RenameParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(USER_MODEL, r#""name":"user""#, 9), "newName": "account" }))).await.unwrap().is_some());
    });
}

#[test]
fn final_mile_collect_workspace_tables_uses_fallback_uri_for_unescaped_disk_paths() {
    block_on_with_trace(async {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root with space");
        let models = root.join("models");
        fs::create_dir_all(&models).unwrap();
        fs::create_dir_all(root.join("migrations")).unwrap();
        fs::write(root.join("vespertide.json"), workspace_config()).unwrap();
        fs::write(models.join("space_user.json"), r#"{"name":"space_user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let (service, _socket) = make_service();
        let backend = service.inner();
        backend.workspace_tables.refresh(&root);

        let workspace = backend.collect_workspace_tables();
        let uris = workspace
            .iter()
            .map(|entry| entry.uri.as_str())
            .collect::<Vec<_>>();
        assert!(
            uris.contains(&"file:///__disk__/space_user.json"),
            "path with a raw space must fall back to the synthetic disk URI: {uris:?}"
        );
    });
}
