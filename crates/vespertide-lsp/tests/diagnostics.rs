//! Integration tests for diagnostic computation end-to-end.

use std::path::PathBuf;

use vespertide_lsp::{DocumentFormat, ParserPool, compute_diagnostics};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push(name);
    path
}

fn read_fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read fixture {}: {err}", path.display()))
}

#[test]
fn valid_user_fixture_zero_diagnostics() {
    let pool = ParserPool::new();
    let text = read_fixture("valid_user.json");
    let tree = pool.parse(&text, DocumentFormat::Json);
    let diags = compute_diagnostics(&text, DocumentFormat::Json, tree.as_ref());

    assert!(diags.is_empty(), "expected zero, got {diags:?}");
}

#[test]
fn truncated_json_emits_diagnostic() {
    let pool = ParserPool::new();
    let text = r#"{"name": "x","#;
    let tree = pool.parse(text, DocumentFormat::Json);
    let diags = compute_diagnostics(text, DocumentFormat::Json, tree.as_ref());

    assert!(!diags.is_empty());
}

#[test]
fn cjk_comment_fixture_compiles() {
    let pool = ParserPool::new();
    let text = read_fixture("cjk_comment.json");
    let tree = pool.parse(&text, DocumentFormat::Json);
    let _diags = compute_diagnostics(&text, DocumentFormat::Json, tree.as_ref());
    // No assertion on diagnostic count — just verifying no panic with CJK.
}
