//! Tree-sitter parser wrappers for JSON and YAML.
//!
//! `tree_sitter::Parser` is NOT `Send`, so we wrap each parser in a [`Mutex`].
//! For V1, a single global parser per format is sufficient (parse is fast
//! for typical model files < 5KB). V2 may introduce thread-local pools if
//! profiling shows contention.

use std::sync::Mutex;
use tree_sitter::{Parser, Tree};

/// Supported document formats for Vespertide model / migration files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentFormat {
    Json,
    Yaml,
}

impl DocumentFormat {
    /// Detect from file extension (case-insensitive).
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Detect from URI path.
    #[must_use]
    pub fn from_uri(uri: &tower_lsp_server::ls_types::Uri) -> Option<Self> {
        let path = uri.path().as_str();
        let ext = path.rsplit('.').next()?;
        Self::from_extension(ext)
    }
}

/// Pool of tree-sitter parsers keyed by [`DocumentFormat`].
///
/// `Parser` is `!Send`; the [`Mutex`] makes the pool `Send + Sync` so it can
/// live behind an `Arc` in the LSP backend.
pub struct ParserPool {
    json: Mutex<Parser>,
    yaml: Mutex<Parser>,
}

impl ParserPool {
    /// Build a new pool with both grammars loaded.
    ///
    /// # Panics
    ///
    /// Panics if loading either grammar fails — both grammars ship with the
    /// crate and are version-pinned, so this should never happen in practice.
    #[must_use]
    pub fn new() -> Self {
        let mut json = Parser::new();
        json.set_language(&tree_sitter_json::LANGUAGE.into())
            .expect("load json grammar");
        let mut yaml = Parser::new();
        yaml.set_language(&tree_sitter_yaml::LANGUAGE.into())
            .expect("load yaml grammar");
        Self {
            json: Mutex::new(json),
            yaml: Mutex::new(yaml),
        }
    }

    /// Parse a document. V1 always does a full reparse (old tree ignored).
    /// V2 may use `Tree::edit` + incremental reparse if profiling demands.
    #[must_use]
    pub fn parse(&self, text: &str, format: DocumentFormat) -> Option<Tree> {
        let parser = match format {
            DocumentFormat::Json => &self.json,
            DocumentFormat::Yaml => &self.yaml,
        };
        parser
            .lock()
            .expect(
                "ParserPool lock poisoned — invariant: tree_sitter::Parser::parse must not panic",
            )
            .parse(text, None)
    }
}

impl Default for ParserPool {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ParserPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParserPool").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::uri;
    use rstest::rstest;

    #[test]
    fn detect_format_from_extension() {
        assert_eq!(
            DocumentFormat::from_extension("json"),
            Some(DocumentFormat::Json)
        );
        assert_eq!(
            DocumentFormat::from_extension("JSON"),
            Some(DocumentFormat::Json)
        );
        assert_eq!(
            DocumentFormat::from_extension("yaml"),
            Some(DocumentFormat::Yaml)
        );
        assert_eq!(
            DocumentFormat::from_extension("yml"),
            Some(DocumentFormat::Yaml)
        );
        assert_eq!(DocumentFormat::from_extension("rs"), None);
    }

    #[test]
    fn parse_valid_json() {
        let pool = ParserPool::new();
        let text = r#"{"name": "user", "columns": []}"#;
        let tree = pool.parse(text, DocumentFormat::Json);
        assert!(tree.is_some());
        let tree = tree.unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), "document");
        assert!(!root.has_error(), "valid JSON should not have errors");
    }

    #[test]
    fn parse_valid_yaml() {
        let pool = ParserPool::new();
        let text = "name: user\ncolumns: []\n";
        let tree = pool.parse(text, DocumentFormat::Yaml);
        assert!(tree.is_some());
    }

    #[test]
    fn parse_cjk_preserved() {
        let pool = ParserPool::new();
        let text = r#"{"name": "도서", "comment": "中文 🚀"}"#;
        let tree = pool.parse(text, DocumentFormat::Json).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parse_invalid_json_returns_tree_with_error() {
        let pool = ParserPool::new();
        let text = r#"{"name": "user","#; // truncated
        let tree = pool.parse(text, DocumentFormat::Json);
        assert!(tree.is_some());
        // tree-sitter returns a tree even on syntax errors; error nodes mark issues.
        assert!(tree.unwrap().root_node().has_error());
    }

    #[test]
    fn parser_pool_default_constructs() {
        let pool = ParserPool::default();

        assert!(pool.parse("{}", DocumentFormat::Json).is_some());
    }

    #[test]
    fn parser_pool_debug_repr_mentions_type() {
        let pool = ParserPool::new();

        assert!(format!("{pool:?}").contains("ParserPool"));
    }

    #[rstest]
    #[case::yaml("user.yaml", Some(DocumentFormat::Yaml))]
    #[case::yml("user.yml", Some(DocumentFormat::Yaml))]
    #[case::json("user.json", Some(DocumentFormat::Json))]
    #[case::unknown("user.txt", None)]
    fn document_format_from_uri_detects_supported_extensions(
        #[case] path: &str,
        #[case] expected: Option<DocumentFormat>,
    ) {
        assert_eq!(DocumentFormat::from_uri(&uri(path)), expected);
    }
}
