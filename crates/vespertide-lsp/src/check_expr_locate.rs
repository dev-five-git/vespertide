//! Shared helpers for locating table-level CHECK `expr` strings in the
//! tree-sitter parse tree.
//!
//! Eliminates the 4-way duplication that previously had
//! `references::resolver`, `references::search`, `rename`, and
//! `code_actions` each re-implementing "is this pair a CHECK `expr`?" /
//! "what table owns this CHECK?" walks. Both JSON (`pair` / `object`) and
//! YAML (`block_mapping_pair` / `block_mapping` / `flow_mapping`, with
//! `flow_node` / `block_node` wrappers around values) are handled — every
//! public helper accepts the source as `&[u8]` and returns format-agnostic
//! results.

use std::ops::Range;

use tree_sitter::{Node, Tree};

use crate::text_util::strip_quotes;
use crate::tree_util::{ancestor_pair, node_at_byte};

/// Cursor-based result of [`find_check_expr_at`]: the CHECK `expr` string
/// the cursor sits in and the inner byte range of its predicate text.
pub(crate) struct CheckExprAt {
    /// Byte range of the CHECK predicate text inside the document,
    /// excluding surrounding quotes / YAML block-scalar indicators.
    /// Same range [`crate::check_expr_range::expr_inner_range`] returns.
    pub inner: Range<usize>,
}

/// If `byte_offset` lands inside a table-level CHECK `expr` string,
/// return its [`CheckExprAt`] context. Returns `None` for any non-CHECK
/// position (cursor outside any string, cursor on the key side, cursor on
/// a string that is not a CHECK constraint's `expr` value).
pub(crate) fn find_check_expr_at(
    tree: &Tree,
    source: &[u8],
    byte_offset: usize,
) -> Option<CheckExprAt> {
    let node = node_at_byte(tree, byte_offset)?;
    let string_node = enclosing_string(node)?;

    let pair = ancestor_pair(string_node)?;
    let key = pair.named_child(0)?;
    let key_text = std::str::from_utf8(source.get(key.byte_range())?).ok()?;
    if strip_quotes(key_text) != "expr" {
        return None;
    }
    // The cursor's string must be the VALUE side, not the key.
    let value = pair.named_child(1)?;
    if !value.byte_range().contains(&string_node.start_byte()) {
        return None;
    }
    if !is_check_constraint_pair(source, pair) {
        return None;
    }

    let inner = crate::check_expr_range::expr_inner_range(string_node)?;

    Some(CheckExprAt { inner })
}

/// True when `expr_pair` (a pair whose key is `expr`) belongs to a CHECK
/// constraint object — i.e. it sits next to a sibling `type: "check"`
/// pair inside a `constraints` array element.
///
/// The caller is responsible for verifying that `expr_pair.key == "expr"`;
/// this helper only inspects the sibling `type` value.
pub(crate) fn is_check_constraint_pair(source: &[u8], expr_pair: Node<'_>) -> bool {
    sibling_value(source, expr_pair, "type").is_some_and(|v| v == "check")
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Look up a sibling pair's scalar value within the same mapping (the
/// `pair`'s direct parent, peeling YAML `flow_node`/`block_node` wrappers).
fn sibling_value(source: &[u8], pair: Node<'_>, target_key: &str) -> Option<String> {
    let object_raw = pair.parent()?;
    let object = peel_wrapper(object_raw);
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair")
            && let Some(key) = child.named_child(0)
            && let Some(key_text) = source
                .get(key.byte_range())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            && strip_quotes(key_text) == target_key
        {
            let value = child.named_child(1)?;
            let actual = peel_wrapper(value);
            let text = source
                .get(actual.byte_range())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())?;
            return Some(strip_quotes(text).to_string());
        }
    }
    None
}

/// If `node` is a YAML `flow_node` / `block_node` wrapper, descend into
/// its first named child (the underlying mapping / scalar). Otherwise
/// return `node` unchanged. JSON nodes are passed through.
fn peel_wrapper(node: Node<'_>) -> Node<'_> {
    match node.kind() {
        "flow_node" | "block_node" => node.named_child(0).unwrap_or(node),
        _ => node,
    }
}

/// Closest ancestor that is a JSON / YAML string scalar. Stops at
/// structural boundaries (arrays, objects, mappings, pairs) so a cursor
/// that lives between strings does not accidentally bind to a string
/// further up the tree.
fn enclosing_string(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar"
            | "block_scalar" => return Some(candidate),
            "string_content" => return candidate.parent(),
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_actions::compute as compute_code_actions;
    use crate::parser::DocumentFormat;
    use crate::test_support::parse_json as parse;

    fn find_pair<'tree>(node: Node<'tree>, source: &[u8], key_name: &str) -> Option<Node<'tree>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "pair" | "block_mapping_pair")
                && let Some(key) = child.named_child(0)
                && let Ok(text) = std::str::from_utf8(&source[key.byte_range()])
                && strip_quotes(text) == key_name
            {
                return Some(child);
            }
            if let Some(found) = find_pair(child, source, key_name) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn find_check_expr_rejects_key_side_and_non_check_constraint() {
        let src = r#"{"name":"u","constraints":[{"type":"check","name":"c","expr":"age > 0"}]}"#;
        let tree = parse(src);
        let key_pos = src.find(r#""expr""#).unwrap() + 2;
        assert!(find_check_expr_at(&tree, src.as_bytes(), key_pos).is_none());

        let non_check =
            r#"{"name":"u","constraints":[{"type":"unique","name":"c","expr":"age > 0"}]}"#;
        let tree = parse(non_check);
        let expr_pos = non_check.find("age > 0").unwrap();
        assert!(find_check_expr_at(&tree, non_check.as_bytes(), expr_pos).is_none());
    }

    #[test]
    fn sibling_value_skips_invalid_utf8_type_key() {
        let src = r#"{"name":"u","constraints":[{"type":"check","expr":"age > 0"}]}"#;
        let tree = parse(src);
        let expr_pair = find_pair(tree.root_node(), src.as_bytes(), "expr").unwrap();

        let type_key = src.find("type").unwrap();
        let mut bad_type = src.as_bytes().to_vec();
        bad_type[type_key] = 0xff;
        assert!(sibling_value(&bad_type, expr_pair, "type").is_none());
    }

    #[test]
    fn sibling_value_returns_none_for_missing_target_key() {
        let src = r#"{"name":"u","constraints":[{"type":"check","expr":"age > 0"}]}"#;
        let tree = parse(src);
        let expr_pair = find_pair(tree.root_node(), src.as_bytes(), "expr").unwrap();

        assert!(sibling_value(src.as_bytes(), expr_pair, "missing").is_none());
    }

    #[test]
    fn enclosing_helpers_return_none_at_structural_root() {
        let tree = parse(r#"{"name":"u"}"#);
        let root = tree.root_node();

        assert!(ancestor_pair(root).is_none());
        assert!(enclosing_string(root).is_none());
    }

    #[test]
    fn cursor_on_non_check_pair_offers_no_between_swap() {
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"age BETWEEN 100 AND 0"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"chk""#).unwrap() + 9;

        let actions = compute_code_actions(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds")
        );
    }
}
