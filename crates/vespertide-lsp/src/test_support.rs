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

/// Apply a slice of [`crate::rename::DomainTextEdit`]s to `src`, walking
/// back-to-front so earlier-edit byte ranges stay valid after later edits
/// land. Consolidates two byte-identical inline copies that lived in
/// `code_actions::tests::apply` and in the rename test
/// `rn_s1_rename_column_rewrites_check_expr_occurrence`. Iterates by
/// reference to avoid an extra `Vec<DomainTextEdit>` clone — the original
/// `code_actions::apply` called `action.edits.clone()` to sort the owned
/// vector; here only the `&DomainTextEdit` references are sorted.
pub(crate) fn apply_text_edits(src: &str, edits: &[crate::rename::DomainTextEdit]) -> String {
    let mut sorted: Vec<&crate::rename::DomainTextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.byte_range.start));
    let mut out = src.to_string();
    for e in &sorted {
        out.replace_range(e.byte_range.clone(), &e.new_text);
    }
    out
}

/// Apply a single [`crate::rename::DomainTextEdit`] to `src` and return
/// the result. Consolidates four byte-identical inline copies that lived
/// in three `code_actions` tests (`remove_primary_key_when_present`,
/// `text_column_offers_varchar_conversion`,
/// `add_foreign_key_skeleton_offered_when_absent`) and the rename test
/// `rename_quoted_column_name_inside_columns_array`. Preserves the
/// allocate-prefix → push-new-text → push-suffix pattern of the original
/// inline copies (no sort needed — only one edit).
pub(crate) fn apply_text_edit(src: &str, edit: &crate::rename::DomainTextEdit) -> String {
    let mut out = String::with_capacity(
        src.len() - (edit.byte_range.end - edit.byte_range.start) + edit.new_text.len(),
    );
    out.push_str(&src[..edit.byte_range.start]);
    out.push_str(&edit.new_text);
    out.push_str(&src[edit.byte_range.end..]);
    out
}
