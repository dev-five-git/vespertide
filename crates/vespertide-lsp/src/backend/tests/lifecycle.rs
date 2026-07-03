use std::fs;
use std::path::Path;

use serde_json::json;
use tower::{Service, ServiceExt};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::{InitializeParams, InitializedParams};

use super::harness::{
    POST_MODEL, USER_MODEL, USER_YAML, block_on_with_trace, did_open_params, initialize_service,
    make_service, notify, open_doc, params, workspace_config, workspace_fixture,
};

const BAD_JSON: &str = "{not json}";
const BAD_FK_MODEL: &str =
    r#"{"name":"bad_fk","columns":[{"name":"owner_id","type":"integer","foreign_key":"invalid"}]}"#;
const CHANGED_USER_MODEL: &str = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false,"unique":true},{"name":"age","type":"integer","nullable":false},{"name":"nickname","type":"text","nullable":true}],"constraints":[{"type":"check","name":"chk_age","expr":"age BETWEEN 0 AND 100"}]}"#;
const DUP_INDEX_MODEL: &str = r#"{"name":"bad_index","columns":[{"name":"id","type":"integer","index":["ix_dup","ix_dup"]}]}"#;

#[tokio::test(flavor = "current_thread")]
async fn direct_lifecycle_methods_return_capabilities_and_log_without_mocking_client() {
    let fixture = workspace_fixture();
    let (service, _socket) = make_service();
    let backend = service.inner();

    let no_root = backend
        .initialize(params::<InitializeParams>(json!({ "capabilities": {} })))
        .await
        .unwrap();
    assert_eq!(no_root.server_info.unwrap().name, "vespertide-lsp");

    let root_uri = backend
        .initialize(params::<InitializeParams>(json!({
            "capabilities": {},
            "rootUri": fixture.root_uri.as_str(),
        })))
        .await
        .unwrap();
    assert!(root_uri.capabilities.text_document_sync.is_some());

    let workspace_folder = backend
        .initialize(params::<InitializeParams>(json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
        })))
        .await
        .unwrap();
    assert!(
        workspace_folder
            .capabilities
            .workspace_symbol_provider
            .is_some()
    );

    backend
        .initialized(params::<InitializedParams>(json!({})))
        .await;
    backend.shutdown().await.unwrap();

    let relative = super::super::Backend::path_to_uri(Path::new("relative-model.json"))
        .expect("relative paths should still lower to synthetic file URIs");
    assert!(relative.as_str().starts_with("file:///"));
    let fallback = super::super::Backend::fallback_disk_uri("ghost");
    assert_eq!(fallback.as_str(), "file:///__disk__/ghost.json");
}

#[test]
fn traced_lifecycle_handlers_cover_logging_fields() {
    block_on_with_trace(async {
        let fixture = workspace_fixture();
        let (service, _socket) = make_service();
        let backend = service.inner();

        backend
            .initialize(params::<InitializeParams>(json!({
                "capabilities": {},
                "clientInfo": { "name": "trace-client", "version": "1" },
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
fn workspace_collection_skips_normalize_failures_and_keeps_disk_tables() {
    block_on_with_trace(async {
        let fixture = workspace_fixture();
        let (service, _socket) = make_service();
        let backend = service.inner();
        backend
            .initialize(params::<InitializeParams>(json!({
                "capabilities": {},
                "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
            })))
            .await
            .unwrap();

        let bad_uri =
            super::super::Backend::path_to_uri(&fixture._tmp.path().join("models/bad_index.json"))
                .unwrap();
        backend
            .did_open(did_open_params(&bad_uri, "json", 1, DUP_INDEX_MODEL))
            .await;

        let workspace = backend.collect_workspace_tables();
        assert!(
            workspace
                .iter()
                .any(|entry| entry.uri == fixture.user_uri && entry.tree.is_none()),
            "disk user table should be collected from workspace_tables"
        );
        assert!(
            workspace
                .iter()
                .all(|entry| entry.table.name != "bad_index"),
            "open model that fails normalize must be skipped"
        );
    });
}

#[test]
fn collect_workspace_tables_prefers_open_document_over_disk_duplicate() {
    block_on_with_trace(async {
        let fixture = workspace_fixture();
        let (service, _socket) = make_service();
        let backend = service.inner();
        backend
            .initialize(params::<InitializeParams>(json!({
                "capabilities": {},
                "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
            })))
            .await
            .unwrap();
        open_doc(backend, &fixture.user_uri, "json", USER_MODEL).await;

        let workspace = backend.collect_workspace_tables();
        let user_entries = workspace
            .iter()
            .filter(|entry| entry.table.name == "user")
            .collect::<Vec<_>>();
        assert_eq!(user_entries.len(), 1);
        assert!(
            user_entries[0].tree.is_some(),
            "open document should win over the duplicate disk registration"
        );
        assert!(
            workspace
                .iter()
                .any(|entry| { entry.table.name == "post" && entry.tree.is_none() })
        );
    });
}

#[test]
fn workspace_table_helpers_cover_degenerate_open_and_disk_state() {
    let pool = crate::parser::ParserPool::new();
    let source = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let uri = super::harness::uri("file:///workspace/user.json");
    let valid = crate::document::DocumentState::new(
        "json".to_string(),
        1,
        source.to_string(),
        crate::parser::DocumentFormat::Json,
        &pool,
    );
    let open_entry = super::super::open_workspace_table(&uri, &valid)
        .expect("valid open document should become a workspace table");
    assert_eq!(open_entry.table.name, "user");
    assert!(open_entry.tree.is_some());

    let no_tree = crate::document::DocumentState {
        doc: lsp_textdocument::FullTextDocument::new("json".to_string(), 1, source.to_string()),
        tree: None,
        format: crate::parser::DocumentFormat::Json,
    };
    assert!(super::super::open_workspace_table(&uri, &no_tree).is_none());

    let normalize_failure = crate::document::DocumentState::new(
        "json".to_string(),
        1,
        DUP_INDEX_MODEL.to_string(),
        crate::parser::DocumentFormat::Json,
        &pool,
    );
    assert!(super::super::open_workspace_table(&uri, &normalize_failure).is_none());

    let table = serde_json::from_str::<vespertide_core::TableDef>(source)
        .unwrap()
        .normalize()
        .unwrap();
    assert!(super::super::disk_workspace_table("user", table.clone(), None).is_none());

    let path = std::path::PathBuf::from("/workspace/models/user.json");
    let (disk_path, disk_entry) =
        super::super::disk_workspace_table("user", table, Some(path.clone()))
            .expect("disk table with a model path should lower");
    assert_eq!(disk_path, path);
    assert!(disk_entry.tree.is_none());
    assert_eq!(disk_entry.table.name, "user");
}

#[tokio::test(flavor = "current_thread")]
async fn service_lifecycle_and_text_document_notifications_exercise_publish_paths() {
    let fixture = workspace_fixture();
    let yaml_uri =
        super::super::Backend::path_to_uri(&fixture._tmp.path().join("models/account.yaml"))
            .unwrap();
    let bad_json_uri =
        super::super::Backend::path_to_uri(&fixture._tmp.path().join("models/bad.json")).unwrap();
    let bad_fk_uri =
        super::super::Backend::path_to_uri(&fixture._tmp.path().join("models/bad_fk.json"))
            .unwrap();
    let text_uri =
        super::super::Backend::path_to_uri(&fixture._tmp.path().join("notes.txt")).unwrap();

    let (mut service, socket) = make_service();
    initialize_service(
        &mut service,
        json!({
            "capabilities": {},
            "clientInfo": { "name": "service-client", "version": "1" },
            "rootUri": fixture.root_uri.as_str(),
        }),
    )
    .await;
    drop(socket);

    notify(&mut service, "initialized", json!({})).await;
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": fixture.user_uri.as_str(),
                "languageId": "json",
                "version": 1,
                "text": USER_MODEL,
            }
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": fixture.post_uri.as_str(),
                "languageId": "json",
                "version": 1,
                "text": POST_MODEL,
            }
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": yaml_uri.as_str(),
                "languageId": "yaml",
                "version": 1,
                "text": USER_YAML,
            }
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": bad_json_uri.as_str(),
                "languageId": "json",
                "version": 1,
                "text": BAD_JSON,
            }
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": bad_fk_uri.as_str(),
                "languageId": "json",
                "version": 1,
                "text": BAD_FK_MODEL,
            }
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": text_uri.as_str(),
                "languageId": "text",
                "version": 1,
                "text": "plain text",
            }
        }),
    )
    .await;

    notify(
        &mut service,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": fixture.user_uri.as_str(), "version": 2 },
            "contentChanges": [{ "text": CHANGED_USER_MODEL }],
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": fixture.user_uri.as_str(), "version": 3 },
            "contentChanges": [],
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didSave",
        json!({ "textDocument": { "uri": fixture.user_uri.as_str() } }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didClose",
        json!({ "textDocument": { "uri": fixture.post_uri.as_str() } }),
    )
    .await;

    let shutdown = Request::build("shutdown").id(2).finish();
    let response = service.ready().await.unwrap().call(shutdown).await.unwrap();
    assert!(response.is_some(), "shutdown should be a JSON-RPC response");
}

#[tokio::test(flavor = "current_thread")]
async fn service_notifications_reindex_and_publish_related_open_documents() {
    let fixture = workspace_fixture();
    let (mut service, socket) = make_service();
    initialize_service(
        &mut service,
        json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
        }),
    )
    .await;
    drop(socket);

    notify(&mut service, "initialized", json!({})).await;
    notify(
        &mut service,
        "textDocument/didOpen",
        serde_json::to_value(did_open_params(&fixture.user_uri, "json", 1, USER_MODEL)).unwrap(),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didOpen",
        serde_json::to_value(did_open_params(&fixture.post_uri, "json", 1, POST_MODEL)).unwrap(),
    )
    .await;

    {
        let backend = service.inner();
        assert_eq!(backend.index.lookup("user").unwrap().uri, fixture.user_uri);
        assert_eq!(backend.index.lookup("post").unwrap().uri, fixture.post_uri);
    }

    notify(
        &mut service,
        "textDocument/didSave",
        json!({ "textDocument": { "uri": fixture.user_uri.as_str() } }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": fixture.user_uri.as_str(), "version": 2 },
            "contentChanges": [{ "text": CHANGED_USER_MODEL }],
        }),
    )
    .await;
    notify(
        &mut service,
        "textDocument/didClose",
        json!({ "textDocument": { "uri": fixture.user_uri.as_str() } }),
    )
    .await;

    let backend = service.inner();
    assert!(backend.index.lookup("user").is_none());
    assert_eq!(backend.index.lookup("post").unwrap().uri, fixture.post_uri);
}

#[tokio::test(flavor = "current_thread")]
async fn did_change_watched_files_covers_rootless_untouched_and_refresh_paths() {
    let (mut rootless, rootless_socket) = make_service();
    initialize_service(&mut rootless, json!({ "capabilities": {} })).await;
    drop(rootless_socket);
    notify(
        &mut rootless,
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": "untitled:///scratch.json", "type": 2 }] }),
    )
    .await;

    let fixture = workspace_fixture();
    let (mut service, socket) = make_service();
    initialize_service(
        &mut service,
        json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
        }),
    )
    .await;
    drop(socket);

    notify(
        &mut service,
        "textDocument/didOpen",
        serde_json::to_value(did_open_params(&fixture.user_uri, "json", 1, USER_MODEL)).unwrap(),
    )
    .await;
    notify(
        &mut service,
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": "untitled:///scratch.json", "type": 2 }] }),
    )
    .await;
    notify(
        &mut service,
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": fixture.outside_uri.as_str(), "type": 2 }] }),
    )
    .await;
    notify(
        &mut service,
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": fixture.user_uri.as_str(), "type": 2 }] }),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_refresh_from_document_uri_finds_config_above_models_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let models = tmp.path().join("models");
    fs::create_dir_all(&models).unwrap();
    fs::write(tmp.path().join("vespertide.json"), workspace_config()).unwrap();
    let user_path = models.join("user.json");
    fs::write(&user_path, USER_MODEL).unwrap();
    let user_uri = super::super::Backend::path_to_uri(&user_path).unwrap();

    let (service, _socket) = make_service();
    let backend = service.inner();
    backend
        .did_open(did_open_params(&user_uri, "json", 1, USER_MODEL))
        .await;

    assert!(
        backend
            .workspace_tables
            .names()
            .contains(&"user".to_string())
    );
}
