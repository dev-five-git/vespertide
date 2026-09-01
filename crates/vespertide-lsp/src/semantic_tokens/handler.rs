//! LSP wire-shape adapter for semantic tokens. Keeps the byte-level
//! `RawToken` work out of `backend.rs` so the latter stays under the
//! workspace's 1000-line file policy.

use tower_lsp_server::ls_types::{
    SemanticTokens, SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult,
};

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;

/// Compute the full-document semantic tokens response for `uri`.
/// Returns `None` if the document isn't open or doesn't have a parsed
/// tree (e.g. plain text file the client mistakenly handed us).
pub fn compute_full(
    store: &DocumentStore,
    params: &SemanticTokensParams,
) -> Option<SemanticTokensResult> {
    let uri = &params.text_document.uri;
    let format = DocumentFormat::from_uri(uri)?;

    let data = store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let raw = super::classify(text, format, state.tree.as_ref());
        super::encode(&raw, &state.doc)
    })?;
    let token_count = data.len();
    let uri_text = uri.as_str();

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri_text,
        tokens = token_count,
        "semantic_tokens_full"
    );

    Some(SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}

/// Compute the range-scoped response. Range-based requests are cheaper
/// for clients that only need on-screen tokens; we still classify the
/// whole tree (cheap) but filter the output to the requested range.
pub fn compute_range(
    store: &DocumentStore,
    params: &SemanticTokensRangeParams,
) -> Option<SemanticTokensRangeResult> {
    let uri = &params.text_document.uri;
    let format = DocumentFormat::from_uri(uri)?;
    let lsp_range = crate::position::ls_to_lsp_range(params.range);

    let data = store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let start = crate::position::lsp_position_to_byte(&state.doc, lsp_range.start);
        let end = crate::position::lsp_position_to_byte(&state.doc, lsp_range.end);
        let raw = super::classify(text, format, state.tree.as_ref());
        let filtered = super::filter_range(raw, start..end);
        super::encode(&filtered, &state.doc)
    })?;
    let token_count = data.len();
    let uri_text = uri.as_str();

    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri_text,
        tokens = token_count,
        "semantic_tokens_range"
    );

    Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DocumentStore;
    use crate::test_support::*;
    use rstest::rstest;
    use tower_lsp_server::ls_types::{
        PartialResultParams, Position, Range as LsRange, TextDocumentIdentifier,
        WorkDoneProgressParams,
    };

    #[derive(Debug, Clone, Copy)]
    enum HandlerCase {
        FullMultiLine,
        RangeSubset,
    }

    #[test]
    fn compute_full_returns_none_when_document_not_open() {
        let store = DocumentStore::new();
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier {
                uri: uri("missing.json"),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        assert!(compute_full(&store, &params).is_none());
    }

    #[test]
    fn compute_full_returns_none_for_unknown_extension() {
        let store = DocumentStore::new();
        let u = uri("doc.txt");
        store.open(u.clone(), "txt".to_string(), 1, "plain".to_string());
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: u },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        // `DocumentFormat::from_uri` returns None for `.txt` so the handler exits early.
        assert!(compute_full(&store, &params).is_none());
    }

    #[test]
    fn compute_full_returns_tokens_for_open_json_document() {
        let store = DocumentStore::new();
        let u = uri("user.json");
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        store.open(u.clone(), "json".to_string(), 1, src.to_string());

        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: u },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let result = compute_full(&store, &params).expect("open doc must return Some");
        match result {
            SemanticTokensResult::Tokens(tokens) => {
                assert!(!tokens.data.is_empty(), "should encode at least one token");
                assert!(tokens.result_id.is_none());
            }
            SemanticTokensResult::Partial(_) => panic!("must return Tokens variant, not Partial"),
        }
    }

    #[test]
    fn compute_full_handles_yaml_document() {
        let store = DocumentStore::new();
        let u = uri("user.yaml");
        let src = "name: user\ncolumns:\n  - name: id\n    type: integer\n";
        store.open(u.clone(), "yaml".to_string(), 1, src.to_string());

        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: u },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let result = compute_full(&store, &params).expect("yaml doc must return Some");
        match result {
            SemanticTokensResult::Tokens(tokens) => {
                assert!(!tokens.data.is_empty(), "YAML should also encode tokens");
            }
            SemanticTokensResult::Partial(_) => panic!("must return Tokens variant"),
        }
    }

    #[test]
    fn compute_range_returns_none_when_document_not_open() {
        let store = DocumentStore::new();
        let params = SemanticTokensRangeParams {
            text_document: TextDocumentIdentifier {
                uri: uri("missing.json"),
            },
            range: LsRange {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 10,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        assert!(compute_range(&store, &params).is_none());
    }

    #[test]
    fn compute_range_returns_tokens_subset() {
        let store = DocumentStore::new();
        let u = uri("user.json");
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        store.open(u.clone(), "json".to_string(), 1, src.to_string());

        // Range covers the whole document.
        let params = SemanticTokensRangeParams {
            text_document: TextDocumentIdentifier { uri: u.clone() },
            range: LsRange {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: u32::try_from(src.len()).unwrap(),
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let result = compute_range(&store, &params).expect("range result");
        match result {
            SemanticTokensRangeResult::Tokens(tokens) => {
                assert!(!tokens.data.is_empty());
            }
            SemanticTokensRangeResult::Partial(_) => panic!("must be Tokens"),
        }
    }

    #[test]
    fn compute_range_filters_out_tokens_outside_range() {
        let store = DocumentStore::new();
        let u = uri("u.json");
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        store.open(u.clone(), "json".to_string(), 1, src.to_string());

        // Empty zero-width range — must encode zero tokens.
        let params = SemanticTokensRangeParams {
            text_document: TextDocumentIdentifier { uri: u },
            range: LsRange {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let result = compute_range(&store, &params).expect("range result");
        match result {
            SemanticTokensRangeResult::Tokens(tokens) => {
                assert!(tokens.data.is_empty(), "zero-width range emits nothing");
            }
            SemanticTokensRangeResult::Partial(_) => panic!("must be Tokens"),
        }
    }

    #[rstest]
    #[case::full_multi_line(HandlerCase::FullMultiLine)]
    #[case::range_subset(HandlerCase::RangeSubset)]
    fn handler_semantic_token_cases(#[case] case: HandlerCase) {
        match case {
            HandlerCase::FullMultiLine => {
                let store = DocumentStore::new();
                let u = uri("multi.json");
                let src = r#"{
  "name": "doc",
  "columns": [
    {"name": "id", "type": "integer"}
  ]
}"#;
                store.open(u.clone(), "json".to_string(), 1, src.to_string());
                let params = SemanticTokensParams {
                    text_document: TextDocumentIdentifier { uri: u },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                };
                let result =
                    compute_full(&store, &params).expect("multi-line doc must produce tokens");

                match result {
                    SemanticTokensResult::Tokens(tokens) => {
                        assert!(
                            tokens.data.iter().any(|token| token.delta_line > 0),
                            "multi-line tokens should produce line deltas"
                        );
                    }
                    SemanticTokensResult::Partial(_) => panic!("must be Tokens"),
                }
            }
            HandlerCase::RangeSubset => {
                let store = DocumentStore::new();
                let u = uri("range.json");
                let src = r#"{"name":"r","columns":[{"name":"id","type":"integer"}]}"#;
                store.open(u.clone(), "json".to_string(), 1, src.to_string());

                let full_params = SemanticTokensParams {
                    text_document: TextDocumentIdentifier { uri: u.clone() },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                };
                let SemanticTokensResult::Tokens(full_tokens) =
                    compute_full(&store, &full_params).expect("full")
                else {
                    panic!("full Tokens");
                };

                let range_params = SemanticTokensRangeParams {
                    text_document: TextDocumentIdentifier { uri: u },
                    range: LsRange {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: 15,
                        },
                    },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                };
                let SemanticTokensRangeResult::Tokens(range_tokens) =
                    compute_range(&store, &range_params).expect("range")
                else {
                    panic!("range Tokens");
                };
                assert!(
                    range_tokens.data.len() <= full_tokens.data.len(),
                    "range must be subset"
                );
            }
        }
    }

    #[test]
    fn tracing_fields_are_evaluated_for_full_and_range_requests() {
        let subscriber = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::INFO)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let store = DocumentStore::new();
            let u = uri("trace.json");
            let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
            store.open(u.clone(), "json".to_string(), 1, src.to_string());

            let full_params = SemanticTokensParams {
                text_document: TextDocumentIdentifier { uri: u.clone() },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };
            assert!(compute_full(&store, &full_params).is_some());

            let range_params = SemanticTokensRangeParams {
                text_document: TextDocumentIdentifier { uri: u },
                range: LsRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: u32::try_from(src.len()).unwrap(),
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };
            assert!(compute_range(&store, &range_params).is_some());
        });
    }
}
