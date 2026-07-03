//! Shared test-only helpers consolidated from inline `#[cfg(test)] mod
//! tests { ... }` blocks across the crate.
//!
//! Gated by `#[cfg(test)]` so it never ships in release builds. Items
//! are `pub(crate)` so any inline `mod tests` in this crate can reach
//! them via `use crate::test_support::*;`.
//!
//! Only genuinely-identical duplicates are hoisted here. Module-specific
//! helpers (e.g. `parse_json` variants that use different `Tree` aliases,
//! or `uri()` variants that take a raw text rather than a file path)
//! remain inline so behaviour and ergonomics are preserved.

use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;

use crate::parser::{DocumentFormat, ParserPool};

/// Build a `file:///{path}` URI. Consolidates the identical 7-copy
/// helper that lived in `workspace_index`, `symbols`, `store`,
/// `hover::foreign_key`, `semantic_tokens::handler`, `rename`,
/// `references::resolver`, and `diagnostics::validation` inline tests.
pub(crate) fn uri(path: &str) -> Uri {
    Uri::from_str(&format!("file:///{path}")).unwrap()
}

/// Parse a source string with the given document format. Consolidates
/// the identical `(src, format) -> Tree` helper that lived in
/// `definition::foreign_key`, `completion::context`,
/// `references::search`, and `diagnostics::validation::types` inline
/// tests.
pub(crate) fn parse(src: &str, format: DocumentFormat) -> tree_sitter::Tree {
    ParserPool::new().parse(src, format).unwrap()
}

/// Parse a source string as JSON. Consolidates the identical
/// `(src) -> Tree` helper that lived in `code_actions`, `inlay_hints`,
/// `diagnostics::validation::visitors`, `references::resolver`, and
/// `check_expr_locate` inline tests.
pub(crate) fn parse_json(src: &str) -> tree_sitter::Tree {
    ParserPool::new().parse(src, DocumentFormat::Json).unwrap()
}

/// Parse a source string as YAML. Consolidates the identical
/// `(src) -> Tree` YAML helper that lived in `inlay_hints` and
/// `references::resolver` inline tests.
pub(crate) fn parse_yaml(src: &str) -> tree_sitter::Tree {
    ParserPool::new().parse(src, DocumentFormat::Yaml).unwrap()
}
