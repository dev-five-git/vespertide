use serde_json::json;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::ls_types::{
    DocumentFormattingParams, DocumentHighlightKind, DocumentHighlightParams, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRangeParams, SelectionRangeParams, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
};

use super::harness::{
    MULTILINE_MODEL, TEXT_URI, UNKNOWN_URI, USER_MODEL, USER_URI, full_range, make_service,
    open_doc, params, position, uri,
};

const HIGHLIGHT_MODEL: &str = r#"{"name":"post","columns":[{"name":"email","type":"text"},{"name":"author_email","type":"text","foreign_key":{"ref_table":"post","ref_columns":["email"]}}]}"#;
const EMPTY_JSON_URI: &str = "file:///workspace/empty.json";
const HIGHLIGHT_URI: &str = "file:///workspace/highlight.json";
const PRETTY_URI: &str = "file:///workspace/pretty.json";
const INVALID_URI: &str = "file:///workspace/invalid.json";

#[tokio::test(flavor = "current_thread")]
async fn document_symbol_returns_nested_symbols_and_none_for_unknown_uri() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;

    let response = backend
        .document_symbol(params::<DocumentSymbolParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
        })))
        .await
        .unwrap()
        .expect("document symbols should exist for a valid model");
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("document symbols should use the nested response");
    };
    assert_eq!(symbols[0].name, "user");
    assert!(
        symbols[0]
            .children
            .as_ref()
            .is_some_and(|children| { children.iter().any(|child| child.name == "id") })
    );

    let missing = backend
        .document_symbol(params::<DocumentSymbolParams>(json!({
            "textDocument": { "uri": UNKNOWN_URI },
        })))
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn folding_range_returns_multiline_folds_and_none_for_single_line() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let multiline_uri = uri(USER_URI);
    let single_line_uri = uri("file:///workspace/single-line.json");
    open_doc(backend, &multiline_uri, "json", MULTILINE_MODEL).await;
    open_doc(backend, &single_line_uri, "json", USER_MODEL).await;

    let folds = backend
        .folding_range(params::<FoldingRangeParams>(json!({
            "textDocument": { "uri": multiline_uri.as_str() },
        })))
        .await
        .unwrap()
        .expect("multi-line JSON should produce folding ranges");
    assert!(folds.iter().any(|fold| fold.end_line > fold.start_line));

    let none = backend
        .folding_range(params::<FoldingRangeParams>(json!({
            "textDocument": { "uri": single_line_uri.as_str() },
        })))
        .await
        .unwrap();
    assert!(none.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn document_highlight_maps_read_and_reference_hits() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let highlight_uri = uri(HIGHLIGHT_URI);
    open_doc(backend, &highlight_uri, "json", HIGHLIGHT_MODEL).await;

    let hits = backend
        .document_highlight(params::<DocumentHighlightParams>(json!({
            "textDocument": { "uri": highlight_uri.as_str() },
            "position": position(HIGHLIGHT_MODEL, r#""name":"email""#, 10),
        })))
        .await
        .unwrap()
        .expect("email declaration should highlight declaration and ref_columns usage");
    assert!(
        hits.iter()
            .any(|hit| hit.kind == Some(DocumentHighlightKind::READ))
    );
    assert!(
        hits.iter()
            .any(|hit| hit.kind == Some(DocumentHighlightKind::TEXT))
    );

    let none = backend
        .document_highlight(params::<DocumentHighlightParams>(json!({
            "textDocument": { "uri": highlight_uri.as_str() },
            "position": { "line": 0, "character": 0 },
        })))
        .await
        .unwrap();
    assert!(none.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn selection_range_returns_ancestor_chain_and_zero_width_fallback() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    let empty_uri = uri(EMPTY_JSON_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;
    open_doc(backend, &empty_uri, "json", "").await;

    let ranges = backend
        .selection_range(params::<SelectionRangeParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "positions": [position(USER_MODEL, r#""name":"id""#, 9)],
        })))
        .await
        .unwrap()
        .expect("selection range should return one entry per requested position");
    assert_eq!(ranges.len(), 1);
    assert!(ranges[0].parent.is_some());

    let fallback = backend
        .selection_range(params::<SelectionRangeParams>(json!({
            "textDocument": { "uri": empty_uri.as_str() },
            "positions": [{ "line": 0, "character": 0 }],
        })))
        .await
        .unwrap()
        .expect("empty parse tree still yields a zero-width selection range");
    assert_eq!(fallback[0].range.start, fallback[0].range.end);

    let missing = backend
        .selection_range(params::<SelectionRangeParams>(json!({
            "textDocument": { "uri": UNKNOWN_URI },
            "positions": [{ "line": 0, "character": 0 }],
        })))
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn semantic_tokens_full_and_range_return_encoded_tokens() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let user_uri = uri(USER_URI);
    open_doc(backend, &user_uri, "json", USER_MODEL).await;

    let full = backend
        .semantic_tokens_full(params::<SemanticTokensParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
        })))
        .await
        .unwrap()
        .expect("full semantic tokens should be present");
    let SemanticTokensResult::Tokens(full) = full else {
        panic!("full response should contain semantic tokens");
    };
    assert!(!full.data.is_empty());

    let ranged = backend
        .semantic_tokens_range(params::<SemanticTokensRangeParams>(json!({
            "textDocument": { "uri": user_uri.as_str() },
            "range": full_range(USER_MODEL),
        })))
        .await
        .unwrap()
        .expect("range semantic tokens should be present");
    let SemanticTokensRangeResult::Tokens(ranged) = ranged else {
        panic!("range response should contain semantic tokens");
    };
    assert!(!ranged.data.is_empty());

    let unsupported = backend
        .semantic_tokens_full(params::<SemanticTokensParams>(json!({
            "textDocument": { "uri": TEXT_URI },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn formatting_returns_edits_empty_vec_and_none_paths() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let compact_uri = uri(USER_URI);
    let pretty_uri = uri(PRETTY_URI);
    let invalid_uri = uri(INVALID_URI);
    let pretty = serde_json::to_string_pretty(
        &serde_json::from_str::<serde_json::Value>(USER_MODEL).unwrap(),
    )
    .map(|mut text| {
        text.push('\n');
        text
    })
    .unwrap();

    open_doc(backend, &compact_uri, "json", USER_MODEL).await;
    open_doc(backend, &pretty_uri, "json", &pretty).await;
    open_doc(backend, &invalid_uri, "json", "{not json}").await;

    let edits = backend
        .formatting(params::<DocumentFormattingParams>(json!({
            "textDocument": { "uri": compact_uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        })))
        .await
        .unwrap()
        .expect("compact JSON should format into a replacement edit");
    assert_eq!(edits.len(), 1);

    let no_edits = backend
        .formatting(params::<DocumentFormattingParams>(json!({
            "textDocument": { "uri": pretty_uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        })))
        .await
        .unwrap()
        .expect("already formatted JSON should return an empty edit list");
    assert!(no_edits.is_empty());

    let invalid = backend
        .formatting(params::<DocumentFormattingParams>(json!({
            "textDocument": { "uri": invalid_uri.as_str() },
            "options": { "tabSize": 2, "insertSpaces": true },
        })))
        .await
        .unwrap();
    assert!(invalid.is_none());

    let unsupported = backend
        .formatting(params::<DocumentFormattingParams>(json!({
            "textDocument": { "uri": TEXT_URI },
            "options": { "tabSize": 2, "insertSpaces": true },
        })))
        .await
        .unwrap();
    assert!(unsupported.is_none());
}
