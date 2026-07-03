//! Semantic tokens — LSP `textDocument/semanticTokens/{full,range}`.
//!
//! Vespertide ships its own `TextMate` / tree-sitter highlight queries
//! (`apps/vscode-extension/syntaxes/`, `apps/zed-extension/languages/`)
//! which colour the document by *syntax*. Semantic tokens layer on top:
//! they classify nodes by *meaning* — a `column.name` value vs a
//! `ref_table` value vs an enum value — so themes can paint them
//! distinctly even though all three are JSON strings at the syntax level.
//!
//! Architecture:
//!   * `legend` defines the ordered set of token types and modifiers
//!     reported on `initialize` (LSP requires the indices to be stable
//!     for the lifetime of the connection).
//!   * `classify_*` modules tree-sitter-walk a document and emit a
//!     [`RawToken`] for each significant span.
//!   * [`encode`] sorts and delta-encodes the raw tokens into the
//!     `Vec<SemanticToken>` wire shape.
//!   * The backend pumps a document through `classify_* → encode` for
//!     both `semantic_tokens_full` and `semantic_tokens_range` (range
//!     is a strict subset, computed by pre-filtering on `RawToken`).

mod check_expr_tokens;
mod classify_json;
mod classify_yaml;
mod encode;
pub mod handler;
pub mod legend;

use std::ops::Range;
use std::sync::{Arc, OnceLock};

pub use encode::encode;
pub use legend::legend;

use crate::cache::{RingCache, hash_text};
use crate::parser::DocumentFormat;

/// Cached result of `classify(source, format, _)`. Same shape as
/// `SymbolCache` (HS-7) and `DiagnosticsCache` (HS-8): 128-slot ring,
/// `(fxhash(source), source.len(), format)` key, `Arc<Vec<RawToken>>`
/// value. On the 100-table synthetic workload (50,000 calls across 100
/// unique sources), this is ~100 misses + ~49,900 hits.
type TokenKey = (u64, usize, DocumentFormat);
type TokenCache = RingCache<TokenKey, Vec<RawToken>, 128>;

static TOKEN_CACHE: OnceLock<TokenCache> = OnceLock::new();

fn token_cache() -> &'static TokenCache {
    TOKEN_CACHE.get_or_init(TokenCache::new)
}

/// A single token emitted by a classifier. Byte ranges are over the
/// document's UTF-8 source — [`encode`] resolves them to UTF-16
/// line/character positions using `lsp-textdocument`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToken {
    /// UTF-8 byte range of the token in the source document.
    pub byte_range: Range<usize>,
    /// Index into `TOKEN_TYPE_NAMES`.
    pub token_type: u32,
    /// Bitmask over `TOKEN_MODIFIER_NAMES`.
    pub token_modifiers: u32,
}

/// Classify the entire document. The classifier dispatches on format —
/// JSON and YAML use different tree-sitter grammars with different node
/// kinds.
#[must_use]
pub fn classify_shared(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> Arc<Vec<RawToken>> {
    let Some(tree_ref) = tree else {
        return Arc::new(Vec::new());
    };

    // Cache the result keyed on the source text (the tree is derived from it).
    token_cache().get_or_compute((hash_text(source), source.len(), format), || {
        classify_uncached(source, format, tree_ref)
    })
}

/// Classify the entire document. The classifier dispatches on format —
/// JSON and YAML use different tree-sitter grammars with different node
/// kinds.
#[must_use]
pub fn classify(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> Vec<RawToken> {
    (*classify_shared(source, format, tree)).clone()
}

/// Uncached classify. Used by the cache on miss.
fn classify_uncached(
    source: &str,
    format: DocumentFormat,
    tree: &tree_sitter::Tree,
) -> Vec<RawToken> {
    match format {
        DocumentFormat::Json => classify_json::classify(source, tree),
        DocumentFormat::Yaml => classify_yaml::classify(source, tree),
    }
}

/// Filter a raw token list to those whose byte range overlaps `range`.
/// Used by `semantic_tokens_range` to satisfy the LSP range request.
#[must_use]
pub fn filter_range(tokens: Vec<RawToken>, range: Range<usize>) -> Vec<RawToken> {
    tokens
        .into_iter()
        .filter(|t| t.byte_range.start < range.end && range.start < t.byte_range.end)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParserPool;
    use std::sync::Arc;

    #[test]
    fn classify_cache_hit_returns_same_arc() {
        let pool = ParserPool::new();
        let source = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();
        let cache = TokenCache::default();
        let key = (hash_text(source), source.len(), DocumentFormat::Json);
        let a = cache.get_or_compute(key, || {
            classify_uncached(source, DocumentFormat::Json, &tree)
        });
        let b = cache.get_or_compute(key, || {
            classify_uncached(source, DocumentFormat::Json, &tree)
        });
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn classify_cache_format_disambiguates() {
        let pool = ParserPool::new();
        let source = r#"{"name":"u","columns":[]}"#;
        let json_tree = pool.parse(source, DocumentFormat::Json).unwrap();
        let yaml_tree = pool.parse(source, DocumentFormat::Yaml).unwrap();
        let cache = TokenCache::default();
        let json_key = (hash_text(source), source.len(), DocumentFormat::Json);
        let yaml_key = (hash_text(source), source.len(), DocumentFormat::Yaml);
        let json_tokens = cache.get_or_compute(json_key, || {
            classify_uncached(source, DocumentFormat::Json, &json_tree)
        });
        let yaml_tokens = cache.get_or_compute(yaml_key, || {
            classify_uncached(source, DocumentFormat::Yaml, &yaml_tree)
        });

        // Re-fetch and verify each format hits its own cached entry.
        let json_tokens_2 = cache.get_or_compute(json_key, Vec::new);
        let yaml_tokens_2 = cache.get_or_compute(yaml_key, Vec::new);
        assert!(Arc::ptr_eq(&json_tokens, &json_tokens_2));
        assert!(Arc::ptr_eq(&yaml_tokens, &yaml_tokens_2));
    }

    #[test]
    fn classify_public_api_uses_cache() {
        let pool = ParserPool::new();
        let source = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();
        let a = classify(source, DocumentFormat::Json, Some(&tree));
        let b = classify(source, DocumentFormat::Json, Some(&tree));
        assert_eq!(a, b);
    }

    #[test]
    fn classify_returns_empty_for_none_tree() {
        let source = r#"{"name":"u"}"#;
        let result = classify(source, DocumentFormat::Json, None);
        assert!(result.is_empty());
    }

    #[test]
    fn classify_shared_populates_static_cache_on_miss() {
        // Use a unique-ish source so the static cache hasn't seen it.
        let pool = ParserPool::new();
        let source =
            r#"{"name":"unique_cache_miss_target","columns":[{"name":"x","type":"text"}]}"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();
        let a = classify_shared(source, DocumentFormat::Json, Some(&tree));
        let b = classify_shared(source, DocumentFormat::Json, Some(&tree));
        // Same Arc out of the static cache on the 2nd call.
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!a.is_empty(), "non-empty doc must produce raw tokens");
    }

    #[test]
    fn classify_shared_returns_empty_arc_for_none_tree() {
        let tokens = classify_shared("anything", DocumentFormat::Json, None);
        assert!(tokens.is_empty());
    }

    #[test]
    fn filter_range_keeps_only_overlapping_tokens() {
        let token_a = RawToken {
            byte_range: 0..5,
            token_type: 0,
            token_modifiers: 0,
        };
        let token_b = RawToken {
            byte_range: 10..15,
            token_type: 0,
            token_modifiers: 0,
        };
        let token_c = RawToken {
            byte_range: 20..25,
            token_type: 0,
            token_modifiers: 0,
        };
        let kept = filter_range(vec![token_a, token_b, token_c], 8..18);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].byte_range, 10..15);
    }

    #[test]
    fn classify_uncached_dispatches_yaml_when_format_is_yaml() {
        // Ensures the YAML branch of the match in `classify_uncached` runs.
        let pool = ParserPool::new();
        let src = "name: yaml_dispatch\ncolumns:\n  - name: a\n    type: text\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let tokens = classify(src, DocumentFormat::Yaml, Some(&tree));
        assert!(!tokens.is_empty(), "YAML classifier should emit tokens");
    }
}
