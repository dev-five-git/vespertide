use serde_json::json;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::ls_types::{
    CodeActionOrCommand, CodeActionParams, CompletionParams, CompletionResponse,
    GotoDefinitionParams, GotoDefinitionResponse, HoverParams, InlayHintLabel, InlayHintParams,
    Location, ReferenceParams, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::harness::{
    POST_MODEL, POST_URI, TEXT_URI, UNKNOWN_URI, USER_MODEL, USER_URI, block_on_with_trace,
    full_range, make_service, open_doc, params, position, uri, workspace_fixture,
};

const PARTIAL_TYPE_MODEL: &str =
    r#"{"name":"u","columns":[{"name":"id","type":"i","nullable":false}]}"#;

async fn open_user_and_post(backend: &super::super::Backend) {
    open_doc(backend, &uri(USER_URI), "json", USER_MODEL).await;
    open_doc(backend, &uri(POST_URI), "json", POST_MODEL).await;
}

#[test]
fn traced_navigation_handlers_cover_logging_fields() {
    block_on_with_trace(async {
        let (service, _socket) = make_service();
        let backend = service.inner();
        let user_uri = uri(USER_URI);
        open_doc(backend, &user_uri, "json", PARTIAL_TYPE_MODEL).await;

        let completion = backend
            .completion(params::<CompletionParams>(json!({
                "textDocument": { "uri": user_uri.as_str() },
                "position": position(PARTIAL_TYPE_MODEL, r#""type":"i""#, 10),
            })))
            .await
            .unwrap();
        assert!(completion.is_some());
        assert!(backend.completion(params::<CompletionParams>(json!({ "textDocument": { "uri": TEXT_URI }, "position": { "line": 0, "character": 0 } }))).await.unwrap().is_none());

        open_user_and_post(backend).await;
        assert!(backend.goto_definition(params::<GotoDefinitionParams>(json!({ "textDocument": { "uri": POST_URI }, "position": position(POST_MODEL, r#""ref_table":"user""#, 14) }))).await.unwrap().is_some());
        assert!(backend.goto_definition(params::<GotoDefinitionParams>(json!({ "textDocument": { "uri": POST_URI }, "position": { "line": 0, "character": 0 } }))).await.unwrap().is_none());
        assert!(backend.references(params::<ReferenceParams>(json!({ "textDocument": { "uri": USER_URI }, "position": position(USER_MODEL, r#""name":"user""#, 9), "context": { "includeDeclaration": false } }))).await.unwrap().is_some());
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
    });
}

#[tokio::test(flavor = "current_thread")]
async fn completion_returns_items_and_empty_paths() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", PARTIAL_TYPE_MODEL).await;

    let completion = backend
        .completion(params::<CompletionParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "position": position(PARTIAL_TYPE_MODEL, r#""type":"i""#, 10),
        })))
        .await
        .unwrap();
    let Some(CompletionResponse::Array(items)) = completion else {
        panic!("completion should return an item array");
    };
    assert!(items.iter().any(|item| item.label == "integer"));

    let unsupported = backend
        .completion(params::<CompletionParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());

    let missing = backend
        .completion(params::<CompletionParams>(json!({
            "textDocument": { "uri": UNKNOWN_URI },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn hover_returns_markup_and_empty_paths() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;

    let hover = backend
        .hover(params::<HoverParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "position": position(USER_MODEL, r#""name":"id""#, 9),
        })))
        .await
        .unwrap()
        .expect("hover on a column should return markup");
    assert!(format!("{:?}", hover.contents).contains("id"));

    let unsupported = backend
        .hover(params::<HoverParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());

    let outside_symbol = backend
        .hover(params::<HoverParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(outside_symbol.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn check_expression_hover_and_code_action_lower_ranges_and_edits() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;

    let check_cursor = position(USER_MODEL, "BETWEEN", 2);
    let hover = backend
        .hover(params::<HoverParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "position": check_cursor,
        })))
        .await
        .unwrap()
        .expect("hover inside a CHECK expression should produce markdown");
    assert!(format!("{:?}", hover.contents).contains("BETWEEN range predicate"));
    assert!(hover.range.is_some());

    let actions = backend
        .code_action(params::<CodeActionParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "range": { "start": check_cursor, "end": check_cursor },
            "context": { "diagnostics": [] },
        })))
        .await
        .unwrap()
        .expect("reversed BETWEEN should produce a code action");
    assert!(actions.iter().any(|action| match action {
        CodeActionOrCommand::CodeAction(action) => action.title == "Swap reversed BETWEEN bounds",
        CodeActionOrCommand::Command(_) => false,
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn goto_definition_resolves_open_and_disk_targets() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    open_user_and_post(backend).await;

    let open_target = backend
        .goto_definition(params::<GotoDefinitionParams>(json!({
            "textDocument": { "uri": POST_URI },
            "position": position(POST_MODEL, r#""ref_table":"user""#, 14),
        })))
        .await
        .unwrap()
        .expect("ref_table should resolve to the open user model");
    let GotoDefinitionResponse::Scalar(Location {
        uri: target_uri, ..
    }) = open_target
    else {
        panic!("expected a scalar definition location");
    };
    assert_eq!(target_uri, uri(USER_URI));

    let missing_target = backend
        .goto_definition(params::<GotoDefinitionParams>(json!({
            "textDocument": { "uri": POST_URI },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(missing_target.is_none());

    let unsupported = backend
        .goto_definition(params::<GotoDefinitionParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());

    let fixture = workspace_fixture();
    let (disk_service, _disk_socket) = make_service();
    let disk_backend = disk_service.inner();
    disk_backend
        .initialize(params(json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
        })))
        .await
        .unwrap();
    open_doc(disk_backend, &fixture.post_uri, "json", POST_MODEL).await;

    let disk_target = disk_backend
        .goto_definition(params::<GotoDefinitionParams>(json!({
            "textDocument": { "uri": fixture.post_uri.as_str() },
            "position": position(POST_MODEL, r#""ref_table":"user""#, 14),
        })))
        .await
        .unwrap()
        .expect("closed disk target should still resolve");
    let GotoDefinitionResponse::Scalar(Location {
        uri: target_uri,
        range,
    }) = disk_target
    else {
        panic!("expected a scalar disk definition location");
    };
    assert_eq!(target_uri, fixture.user_uri);
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn references_return_locations_and_none_for_empty_results() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    open_user_and_post(backend).await;

    let refs = backend
        .references(params::<ReferenceParams>(json!({
            "textDocument": { "uri": USER_URI },
            "position": position(USER_MODEL, r#""name":"user""#, 9),
            "context": { "includeDeclaration": true },
        })))
        .await
        .unwrap()
        .expect("table references should include declaration and FK usage");
    assert!(refs.iter().any(|loc| loc.uri == uri(USER_URI)));
    assert!(refs.iter().any(|loc| loc.uri == uri(POST_URI)));

    let no_symbol = backend
        .references(params::<ReferenceParams>(json!({
            "textDocument": { "uri": USER_URI },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": true },
        })))
        .await
        .unwrap();
    assert!(no_symbol.is_none());

    let unsupported = backend
        .references(params::<ReferenceParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": true },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());

    let missing_doc = backend
        .references(params::<ReferenceParams>(json!({
            "textDocument": { "uri": UNKNOWN_URI },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": true },
        })))
        .await
        .unwrap();
    assert!(missing_doc.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn code_action_returns_workspace_edits_and_none_for_empty_results() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;

    let cursor = position(USER_MODEL, r#""name":"id""#, 9);
    let actions = backend
        .code_action(params::<CodeActionParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "range": { "start": cursor, "end": cursor },
            "context": { "diagnostics": [] },
        })))
        .await
        .unwrap()
        .expect("column cursor should produce refactor actions");
    assert!(actions.iter().any(|action| match action {
        CodeActionOrCommand::CodeAction(action) => action.title == "Unmark primary key",
        CodeActionOrCommand::Command(_) => false,
    }));

    let table_cursor = position(USER_MODEL, r#""name":"user""#, 9);
    let none = backend
        .code_action(params::<CodeActionParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "range": { "start": table_cursor, "end": table_cursor },
            "context": { "diagnostics": [] },
        })))
        .await
        .unwrap();
    assert!(none.is_none());

    let unsupported = backend
        .code_action(params::<CodeActionParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
            "context": { "diagnostics": [] },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());

    let missing_doc = backend
        .code_action(params::<CodeActionParams>(json!({
            "textDocument": { "uri": UNKNOWN_URI },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
            "context": { "diagnostics": [] },
        })))
        .await
        .unwrap();
    assert!(missing_doc.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn inlay_hint_returns_labels_and_none_for_empty_results() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;

    let hints = backend
        .inlay_hint(params::<InlayHintParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "range": full_range(USER_MODEL),
        })))
        .await
        .unwrap()
        .expect("PK/CHECK model should produce inlay hints");
    assert!(hints.iter().any(|hint| matches!(
        &hint.label,
        InlayHintLabel::String(label) if label.contains("PK") || label.contains(": integer")
    )));

    let empty_range = backend
        .inlay_hint(params::<InlayHintParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
        })))
        .await
        .unwrap();
    assert!(empty_range.is_none());

    let unsupported = backend
        .inlay_hint(params::<InlayHintParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());

    let missing_doc = backend
        .inlay_hint(params::<InlayHintParams>(json!({
            "textDocument": { "uri": UNKNOWN_URI },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
        })))
        .await
        .unwrap();
    assert!(missing_doc.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_symbol_returns_flat_symbols_and_none_for_no_match() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    open_user_and_post(backend).await;

    let symbols = backend
        .symbol(params::<WorkspaceSymbolParams>(json!({ "query": "user" })))
        .await
        .unwrap()
        .expect("workspace symbol should find the user table");
    let WorkspaceSymbolResponse::Flat(symbols) = symbols else {
        panic!("workspace symbols should use the flat response");
    };
    assert!(symbols.iter().any(|symbol| symbol.name == "user"));

    let none = backend
        .symbol(params::<WorkspaceSymbolParams>(
            json!({ "query": "does-not-exist" }),
        ))
        .await
        .unwrap();
    assert!(none.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_symbol_includes_closed_disk_workspace_tables() {
    let fixture = workspace_fixture();
    let (service, _socket) = make_service();
    let backend = service.inner();
    backend
        .initialize(params(json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": fixture.root_uri.as_str(), "name": "fixture" }],
        })))
        .await
        .unwrap();
    open_doc(backend, &fixture.user_uri, "json", USER_MODEL).await;

    let symbols = backend
        .symbol(params::<WorkspaceSymbolParams>(
            json!({ "query": "author_id" }),
        ))
        .await
        .unwrap()
        .expect("closed disk post model should contribute workspace symbols");
    let WorkspaceSymbolResponse::Flat(symbols) = symbols else {
        panic!("workspace symbols should use the flat response");
    };
    assert!(
        symbols.iter().any(|symbol| {
            symbol.name == "author_id" && symbol.location.uri == fixture.post_uri
        })
    );
}
