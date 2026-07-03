//! Shared byte-range extraction for table-level CHECK expression scalars.

use std::ops::Range;

/// Inner byte range of a CHECK `expr` scalar, excluding syntax delimiters.
///
/// Handles JSON strings and YAML scalar wrappers. For YAML block scalars,
/// the returned range starts after the `|` / `>` indicator and runs to the
/// node end; the CHECK lexer trims per-line indentation/newlines later.
pub(crate) fn expr_inner_range(value_node: tree_sitter::Node<'_>) -> Option<Range<usize>> {
    let raw = value_node.byte_range();
    match value_node.kind() {
        "string" | "double_quote_scalar" | "single_quote_scalar" => {
            (raw.end.saturating_sub(raw.start) >= 2).then(|| (raw.start + 1)..(raw.end - 1))
        }
        "plain_scalar" => value_node
            .named_child(0)
            .filter(|child| child.kind() == "string_scalar")
            .map_or_else(|| Some(raw.clone()), |child| Some(child.byte_range())),
        "string_scalar" => Some(raw),
        "block_scalar" => value_node
            .child(0)
            .map(|indicator| indicator.end_byte()..value_node.end_byte()),
        "flow_node" | "block_node" => value_node.named_child(0).and_then(expr_inner_range),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::test_support::parse_yaml;

    fn first_kind<'tree>(
        node: tree_sitter::Node<'tree>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'tree>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = first_kind(child, kind) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn non_scalar_nodes_have_no_check_expr_inner_range() {
        let tree = ParserPool::new()
            .parse(r#"{"expr":"age > 0"}"#, DocumentFormat::Json)
            .unwrap();

        assert!(expr_inner_range(tree.root_node()).is_none());
    }

    #[test]
    fn yaml_block_scalar_inner_range_starts_after_indicator() {
        let src =
            "name: u\nconstraints:\n  - type: check\n    expr: >-\n      age BETWEEN 100 AND 0\n";
        let tree = parse_yaml(src);
        let block = first_kind(tree.root_node(), "block_scalar").expect("block scalar");
        let inner = expr_inner_range(block).expect("block scalar inner range");

        assert!(src[inner].contains("age BETWEEN 100 AND 0"));
    }
}
