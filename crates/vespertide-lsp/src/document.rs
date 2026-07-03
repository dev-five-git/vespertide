//! Per-document state: text + parsed tree + format metadata.
//!
//! Uses `lsp-textdocument`'s [`FullTextDocument`] for UTF-16 ↔ byte offset
//! mapping (justified: rust-analyzer / nushell switched away from `ropey`
//! because its UTF-8 implementation had real-world bugs).

use lsp_textdocument::FullTextDocument;
use tree_sitter::Tree;

use crate::parser::{DocumentFormat, ParserPool};

/// In-memory representation of a single open document.
#[derive(Debug)]
pub struct DocumentState {
    /// UTF-16-aware text buffer (handles LSP position math).
    pub doc: FullTextDocument,
    /// Latest tree-sitter parse. `None` if parsing failed catastrophically
    /// (grammar returns `None`); syntax errors still yield `Some(tree)` with
    /// error nodes.
    pub tree: Option<Tree>,
    /// Document format, derived from URI extension at open time.
    pub format: DocumentFormat,
}

impl DocumentState {
    /// Build state from an initial full text payload (from `textDocument/didOpen`).
    pub fn new(
        language_id: String,
        version: i32,
        text: String,
        format: DocumentFormat,
        parser_pool: &ParserPool,
    ) -> Self {
        let tree = parser_pool.parse(&text, format);
        let doc = FullTextDocument::new(language_id, version, text);
        Self { doc, tree, format }
    }

    /// Replace text content (full sync). V1 always reparses from scratch.
    ///
    /// Note: `lsp-textdocument`'s API takes the upstream `lsp_types`
    /// 0.97 change-event type, which is distinct from
    /// `tower_lsp_server::ls_types::TextDocumentContentChangeEvent`.
    /// The two layers stay decoupled — handlers translate at the seam.
    pub fn update_full(&mut self, text: String, version: i32, parser_pool: &ParserPool) {
        let changes = [lsp_types::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text,
        }];
        self.doc.update(&changes, version);
        self.tree = parser_pool.parse(self.doc.get_content(None), self.format);
    }

    /// Current text content.
    #[must_use]
    pub fn text(&self) -> &str {
        self.doc.get_content(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_parses_tree() {
        let pool = ParserPool::new();
        let state = DocumentState::new(
            "json".to_string(),
            1,
            r#"{"name": "user"}"#.to_string(),
            DocumentFormat::Json,
            &pool,
        );
        assert!(state.tree.is_some());
        assert!(!state.tree.as_ref().unwrap().root_node().has_error());
    }

    #[test]
    fn update_full_replaces_content() {
        let pool = ParserPool::new();
        let mut state = DocumentState::new(
            "json".to_string(),
            1,
            r#"{"name": "user"}"#.to_string(),
            DocumentFormat::Json,
            &pool,
        );
        state.update_full(r#"{"name": "post"}"#.to_string(), 2, &pool);
        assert!(state.text().contains("post"));
        assert!(state.tree.is_some());
    }

    #[test]
    fn cjk_round_trip_preserved() {
        let pool = ParserPool::new();
        let text = r#"{"name": "도서", "comment": "中文 🚀"}"#;
        let state = DocumentState::new(
            "json".to_string(),
            1,
            text.to_string(),
            DocumentFormat::Json,
            &pool,
        );
        assert_eq!(state.text(), text);
    }
}
