//! Integration tests for `vespertide-lsp`.
//!
//! Wave 1 verifies only that the scaffold compiles, the fixture corpus is
//! present on disk, and the library re-exports the [`Backend`] type. Real
//! LSP request/response cycles, document state, and diagnostics are layered
//! in by Wave 2+.
//!
//! Fixture files mirror the four supported shapes:
//!
//! - `valid_user.json` — happy-path JSON model
//! - `invalid_fk.json` — FK references a nonexistent table (diagnostic target)
//! - `cjk_comment.json` — multi-byte chars (UTF-16 column mapping target)
//! - `valid_user.yaml` — YAML model
//!
//! Keep this file thin; per-feature tests should live next to their module
//! once it lands (e.g. `tests/diagnostics.rs`, `tests/hover.rs`).

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p
}

#[test]
fn fixtures_dir_exists() {
    let dir = fixtures_dir();
    assert!(dir.exists(), "fixtures directory missing: {dir:?}");
    for name in [
        "valid_user.json",
        "invalid_fk.json",
        "cjk_comment.json",
        "valid_user.yaml",
    ] {
        let p = dir.join(name);
        assert!(p.exists(), "fixture missing: {p:?}");
    }
}

#[test]
fn fixture_valid_user_json_parses_as_json() {
    let path = fixtures_dir().join("valid_user.json");
    let text = std::fs::read_to_string(&path).expect("read valid_user.json");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("valid_user.json must be valid JSON");
    assert_eq!(
        value.get("name").and_then(serde_json::Value::as_str),
        Some("user")
    );
}

#[test]
fn fixture_cjk_comment_preserves_multibyte_chars() {
    let path = fixtures_dir().join("cjk_comment.json");
    let text = std::fs::read_to_string(&path).expect("read cjk_comment.json");
    // Sanity checks for the UTF-16 / multi-byte mitigation corpus.
    assert!(text.contains("도서"), "Korean characters must be preserved");
    assert!(
        text.contains("中文"),
        "Chinese characters must be preserved"
    );
    assert!(
        text.contains("🚀"),
        "emoji (4-byte UTF-8) must be preserved"
    );
}

/// Trait-bound asserter used purely at type-check time to lock in
/// `Backend: Send + Sync`. Calling it has no runtime effect.
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn backend_type_is_publicly_re_exported() {
    // Compile-time assertion that the public API surface is what callers
    // (`LspService::new(Backend::new)` in `main.rs`) expect.
    assert_send_sync::<vespertide_lsp::Backend>();
}

#[test]
fn fixtures_have_valid_top_level_name() {
    // Sanity check: the workspace index should be able to extract a `name`
    // from our JSON fixtures. (Direct WorkspaceIndex unit tests live in
    // `workspace_index::tests`.)
    use std::fs;
    for name in ["valid_user.json", "invalid_fk.json", "cjk_comment.json"] {
        let path = fixtures_dir().join(name);
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("\"name\""),
            "fixture {name} missing name field"
        );
    }
}
