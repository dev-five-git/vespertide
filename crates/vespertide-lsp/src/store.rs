//! Concurrent document store keyed by URI.
//!
//! [`DashMap`] is justified here as a performance-critical hot path:
//! `textDocument/didChange` arrives per-document and concurrently. All other
//! maps in the workspace use [`BTreeMap`](std::collections::BTreeMap) per the
//! AGENTS.md policy; this is the documented exception.

use dashmap::DashMap;
use tower_lsp_server::ls_types::Uri;

use crate::document::DocumentState;
use crate::parser::{DocumentFormat, ParserPool};

/// Thread-safe map of open documents.
#[derive(Debug)]
pub struct DocumentStore {
    docs: DashMap<Uri, DocumentState>,
    parser_pool: ParserPool,
}

impl DocumentStore {
    /// Build an empty store with a fresh parser pool.
    #[must_use]
    pub fn new() -> Self {
        Self {
            docs: DashMap::new(),
            parser_pool: ParserPool::new(),
        }
    }

    /// Handle `textDocument/didOpen`.
    ///
    /// Format is inferred from the URI extension; unknown extensions default
    /// to [`DocumentFormat::Json`].
    pub fn open(&self, uri: Uri, language_id: String, version: i32, text: String) {
        let format = DocumentFormat::from_uri(&uri).unwrap_or(DocumentFormat::Json);
        let state = DocumentState::new(language_id, version, text, format, &self.parser_pool);
        self.docs.insert(uri, state);
    }

    /// Handle a full-sync `textDocument/didChange`.
    pub fn update_full(&self, uri: &Uri, text: String, version: i32) {
        if let Some(mut entry) = self.docs.get_mut(uri) {
            entry.update_full(text, version, &self.parser_pool);
        }
    }

    /// Handle `textDocument/didClose`.
    pub fn close(&self, uri: &Uri) {
        self.docs.remove(uri);
    }

    /// Number of currently-open documents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// `true` if no documents are open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Borrow a document's current text. Returns `None` if not open.
    pub fn with_text<R>(&self, uri: &Uri, f: impl FnOnce(&str) -> R) -> Option<R> {
        self.docs.get(uri).map(|state| f(state.text()))
    }

    /// Borrow a document's text and tree-sitter tree atomically.
    /// Returns `None` if the document is not open.
    pub fn with_doc<R>(
        &self,
        uri: &Uri,
        f: impl FnOnce(&str, Option<&tree_sitter::Tree>) -> R,
    ) -> Option<R> {
        self.docs
            .get(uri)
            .map(|state| f(state.text(), state.tree.as_ref()))
    }

    /// Apply a closure to the full document state. Returns `None` if not open.
    ///
    /// Used by diagnostic publication to access text, tree, and
    /// [`lsp_textdocument::FullTextDocument`] together.
    pub fn docs_iter_for_uri<R>(
        &self,
        uri: &Uri,
        f: impl FnOnce(&DocumentState) -> R,
    ) -> Option<R> {
        self.docs.get(uri).map(|state| f(&state))
    }

    /// Snapshot all open document URIs, sorted for deterministic iteration.
    #[must_use]
    pub fn open_uris(&self) -> Vec<Uri> {
        let mut uris: Vec<Uri> = self.docs.iter().map(|entry| entry.key().clone()).collect();
        uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        uris
    }

    /// Apply a closure to every open document in sorted URI order.
    pub fn for_each(&self, mut f: impl FnMut(&Uri, &DocumentState)) {
        let uris = self.open_uris();
        for uri in &uris {
            if let Some(entry) = self.docs.get(uri) {
                f(uri, entry.value());
            }
        }
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::uri;

    #[test]
    fn open_insert_update_close() {
        let store = DocumentStore::new();
        let u = uri("test.json");
        assert!(store.is_empty());

        store.open(
            u.clone(),
            "json".to_string(),
            1,
            r#"{"name": "user"}"#.to_string(),
        );
        assert_eq!(store.len(), 1);

        store.update_full(&u, r#"{"name": "post"}"#.to_string(), 2);
        let text = store
            .with_text(&u, std::string::ToString::to_string)
            .unwrap();
        assert!(text.contains("post"));

        store.close(&u);
        assert!(store.is_empty());
    }

    #[test]
    fn open_uris_are_sorted() {
        let store = DocumentStore::new();
        let b = uri("b.json");
        let a = uri("a.json");

        for u in [b.clone(), a.clone()] {
            store.open(
                u,
                "json".to_string(),
                1,
                r#"{"name": "x", "columns": []}"#.to_string(),
            );
        }

        assert_eq!(store.open_uris(), vec![a, b]);
    }

    #[test]
    fn default_constructs_empty_store() {
        let store = DocumentStore::default();

        assert!(store.is_empty());
    }

    #[test]
    fn for_each_iterates_in_uri_order() {
        let store = DocumentStore::new();
        store.open(
            uri("z.json"),
            "json".to_string(),
            1,
            r#"{"name":"z"}"#.to_string(),
        );
        store.open(
            uri("a.json"),
            "json".to_string(),
            1,
            r#"{"name":"a"}"#.to_string(),
        );
        store.open(
            uri("m.json"),
            "json".to_string(),
            1,
            r#"{"name":"m"}"#.to_string(),
        );

        let mut collected = Vec::new();
        store.for_each(|u, _state| collected.push(u.as_str().to_string()));
        let mut sorted = collected.clone();
        sorted.sort();

        assert_eq!(
            collected, sorted,
            "for_each must yield URIs in sorted order"
        );
        assert_eq!(collected.len(), 3);
    }

    #[test]
    fn with_doc_returns_tree_for_open_document() {
        let store = DocumentStore::new();
        let u = uri("user.json");
        store.open(
            u.clone(),
            "json".to_string(),
            1,
            r#"{"name":"user","columns":[]}"#.to_string(),
        );

        let has_tree = store
            .with_doc(&u, |_text, tree| tree.is_some())
            .expect("doc present");

        assert!(has_tree);
    }

    #[test]
    fn docs_iter_for_uri_returns_none_for_missing_document() {
        let store = DocumentStore::new();

        assert!(
            store
                .docs_iter_for_uri(&uri("missing.json"), |_| 42)
                .is_none()
        );
    }
}
