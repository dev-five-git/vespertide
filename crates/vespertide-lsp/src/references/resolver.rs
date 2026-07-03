//! Resolve "what symbol is the cursor on?" for the references provider.

use crate::text_util::strip_quotes;
use tower_lsp_server::ls_types::Uri;

use super::ReferenceSymbol;

/// Walk up from the cursor and decide whether it sits on a table or column
/// reference. Returns `None` for non-reference positions.
pub(super) fn resolve(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    byte_offset: usize,
) -> Option<ReferenceSymbol> {
    let _ = current_uri;
    let tree = tree?;
    let node = node_at_byte(tree, byte_offset)?;
    let string_node = enclosing_string(node)?;
    let raw = source.get(string_node.byte_range())?;
    let symbol_text = strip_quotes(raw);
    if symbol_text.is_empty() {
        return None;
    }

    // What pair owns the string?
    let pair = enclosing_pair(string_node)?;
    let key = pair.named_child(0)?;
    let key_text = strip_quotes(source.get(key.byte_range())?);

    // Make sure the cursor is on the VALUE side, not the key side.
    let value = pair.named_child(1)?;
    if !value.byte_range().contains(&string_node.start_byte()) {
        return None;
    }

    match key_text {
        // Top-level table name OR foreign_key.ref_table.
        "name" if is_top_level_pair(pair) => Some(ReferenceSymbol::Table {
            name: symbol_text.to_string(),
        }),
        "ref_table" => Some(ReferenceSymbol::Table {
            name: symbol_text.to_string(),
        }),
        // Column reference: either inside a column object's `name` pair, or
        // inside the `ref_columns` array element.
        "name" => {
            let owning_table = enclosing_table_name(pair, source)?;
            Some(ReferenceSymbol::Column {
                table: owning_table,
                column: symbol_text.to_string(),
            })
        }
        "ref_columns" => {
            let fk_object = pair.parent().and_then(skip_yaml_wrappers)?;
            let ref_table_raw = direct_child_value(fk_object, source, "ref_table")?;
            Some(ReferenceSymbol::Column {
                table: strip_quotes(ref_table_raw).to_string(),
                column: symbol_text.to_string(),
            })
        }
        // Cursor inside a table-level CHECK `expr` string. The bare
        // identifier the cursor sits on is a reference to a column of the
        // CHECK's owning table.
        "expr" if is_check_constraint_pair(pair, source) => {
            resolve_check_expr_column(string_node, pair, source, byte_offset)
        }
        _ => None,
    }
}

/// True when this `expr` pair belongs to a CHECK constraint object — i.e.
/// it sits next to a sibling `type: "check"` pair inside a `constraints`
/// array element.
fn is_check_constraint_pair(expr_pair: tree_sitter::Node<'_>, source: &str) -> bool {
    let Some(constraint_object) = expr_pair.parent().and_then(skip_yaml_wrappers) else {
        return false;
    };
    direct_child_value(constraint_object, source, "type")
        .is_some_and(|raw| strip_quotes(raw) == "check")
}

/// Given the cursor byte offset inside a CHECK `expr` string, lex the
/// expression and, if the cursor sits on a column identifier, resolve it to
/// the owning table's column.
fn resolve_check_expr_column(
    string_node: tree_sitter::Node<'_>,
    expr_pair: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Option<ReferenceSymbol> {
    let inner = crate::check_expr_range::expr_inner_range(string_node)?;
    let expr_text = source.get(inner.clone())?;
    let rel = byte_offset.checked_sub(inner.start)?;
    let column = vespertide_planner::lex_check_expr(expr_text)
        .into_iter()
        .find(|tok| {
            tok.kind == vespertide_planner::CheckTokenKind::Column && tok.span.contains(&rel)
        })
        .map(|tok| expr_text.get(tok.span).map(str::to_string))??;

    let owning_table = enclosing_table_name(expr_pair, source)?;
    Some(ReferenceSymbol::Column {
        table: owning_table,
        column,
    })
}

fn enclosing_string(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => return Some(candidate),
            "string_content" => return candidate.parent(),
            // Stop at structural boundaries.
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn enclosing_pair(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "pair" | "block_mapping_pair") {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// A pair is "top level" when its direct ancestor mapping is itself the
/// outermost mapping of the document. This is how we distinguish the
/// table's own `name: ...` from a column's `name: ...`.
fn is_top_level_pair(pair: tree_sitter::Node<'_>) -> bool {
    let Some(parent_mapping) = pair.parent() else {
        return false;
    };
    if !matches!(
        parent_mapping.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return false;
    }
    // Walk above the parent mapping; if we encounter another mapping with
    // the same kind, we are nested and therefore not top level.
    let mut current = parent_mapping.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return false;
        }
        current = candidate.parent();
    }
    true
}

/// Given a column's `name` pair, find the owning table's top-level name.
fn enclosing_table_name(name_pair: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    // The pair we got is inside a column object. Walk up to the document's
    // outermost mapping and return its direct `name` value.
    let mut current = name_pair.parent();
    let mut outer = None;
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            outer = Some(candidate);
        }
        current = candidate.parent();
    }
    let outer = outer?;

    let mut cursor = outer.walk();
    for child in outer.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair") {
            let key = child.named_child(0)?;
            let key_text = strip_quotes(source.get(key.byte_range())?);
            if key_text == "name" {
                let value = child.named_child(1)?;
                return Some(strip_quotes(source.get(value.byte_range())?).to_string());
            }
        }
    }
    None
}

fn direct_child_value<'a>(
    object: tree_sitter::Node<'_>,
    source: &'a str,
    target_key: &str,
) -> Option<&'a str> {
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair")
            && let Some(key) = child.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && strip_quotes(key_text) == target_key
            && let Some(value) = child.named_child(1)
            && let Some(text) = source.get(value.byte_range())
        {
            return Some(text);
        }
    }
    None
}

fn skip_yaml_wrappers(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node") {
        let parent = current.parent();
        parent?;
        current = parent.expect("YAML wrapper reached from a mapping pair has a parent");
    }
    Some(current)
}

fn node_at_byte(tree: &tree_sitter::Tree, byte_offset: usize) -> Option<tree_sitter::Node<'_>> {
    let root = tree.root_node();
    let mut current = root;
    'outer: loop {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.byte_range().contains(&byte_offset) {
                current = child;
                continue 'outer;
            }
        }
        return Some(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use rstest::rstest;

    #[test]
    fn resolve_returns_none_when_tree_is_none() {
        let src = r#"{"name":"u"}"#;

        assert!(resolve(src, None, &uri("u.json"), 0).is_none());
    }

    #[rstest]
    #[case::top_level_name(r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#, r#""name":"user""#, 9, Some(ReferenceSymbol::Table { name: "user".to_string() }))]
    #[case::column_name(r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#, r#""name":"email""#, 10, Some(ReferenceSymbol::Column { table: "user".to_string(), column: "email".to_string() }))]
    #[case::key_side(r#"{"name":"u"}"#, r#""name""#, 2, None)]
    #[case::empty_string(r#"{"name":""}"#, r#""name":"""#, 8, None)]
    #[case::ref_table(r#"{"name":"p","columns":[{"name":"x","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#, r#""ref_table":"user""#, 14, Some(ReferenceSymbol::Table { name: "user".to_string() }))]
    #[case::ref_columns_entry(r#"{"name":"p","columns":[{"name":"x","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["email"]}}]}"#, r#"["email"]"#, 3, Some(ReferenceSymbol::Column { table: "user".to_string(), column: "email".to_string() }))]
    #[case::check_expr_balance(r#"{"name":"acct","columns":[{"name":"balance","type":"integer"}],"constraints":[{"type":"check","name":"c","expr":"balance > 0"}]}"#, r#""expr":"balance > 0""#, 8, Some(ReferenceSymbol::Column { table: "acct".to_string(), column: "balance".to_string() }))]
    #[case::check_expr_age(r#"{"name":"user","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#, r#""expr":"age > 0""#, 8, Some(ReferenceSymbol::Column { table: "user".to_string(), column: "age".to_string() }))]
    #[case::check_expr_operator(r#"{"name":"acct","columns":[{"name":"balance","type":"integer"}],"constraints":[{"type":"check","name":"c","expr":"balance > 0"}]}"#, "balance > 0", "balance ".len(), None)]
    fn resolve_json_cursor_cases(
        #[case] src: &str,
        #[case] needle: &str,
        #[case] cursor_delta: usize,
        #[case] expected: Option<ReferenceSymbol>,
    ) {
        let tree = parse_json(src);
        let pos = src.find(needle).unwrap() + cursor_delta;

        assert_eq!(resolve(src, Some(&tree), &uri("u.json"), pos), expected);
    }

    #[rstest]
    #[case::top_level_name("name: user\ncolumns:\n  - name: id\n    type: integer\n", "name: user", 6, ReferenceSymbol::Table { name: "user".to_string() })]
    #[case::column_name("name: user\ncolumns:\n  - name: email\n    type: text\n", "name: email", 6, ReferenceSymbol::Column { table: "user".to_string(), column: "email".to_string() })]
    fn resolve_yaml_cursor_cases(
        #[case] src: &str,
        #[case] needle: &str,
        #[case] cursor_delta: usize,
        #[case] expected: ReferenceSymbol,
    ) {
        let tree = parse_yaml(src);
        let pos = src.find(needle).unwrap() + cursor_delta;

        assert_eq!(
            resolve(src, Some(&tree), &uri("u.yaml"), pos),
            Some(expected)
        );
    }

    fn first_node<'tree>(
        node: tree_sitter::Node<'tree>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'tree>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = first_node(child, kind) {
                return Some(found);
            }
        }
        None
    }

    fn first_pair(tree: &tree_sitter::Tree) -> tree_sitter::Node<'_> {
        first_node(tree.root_node(), "pair").expect("pair")
    }

    #[test]
    fn resolve_returns_none_for_unhandled_pair_key() {
        let src = r#"{"comment":"hello"}"#;
        let tree = parse_json(src);
        let pos = src.find("hello").unwrap();

        assert!(resolve(src, Some(&tree), &uri("u.json"), pos).is_none());
    }

    #[test]
    fn private_helpers_return_none_for_structural_nodes() {
        let tree = parse_json(r#"{"name":"u"}"#);
        let root = tree.root_node();

        assert!(!is_check_constraint_pair(root, r#"{"name":"u"}"#));
        assert!(enclosing_string(root).is_none());
        assert!(enclosing_pair(root).is_none());
        assert!(!is_top_level_pair(root));
    }

    #[test]
    fn is_top_level_pair_rejects_nested_non_mapping_parent() {
        let src = r#"{"name":"u"}"#;
        let tree = parse_json(src);
        let string_node = first_node(tree.root_node(), "string").unwrap();

        assert!(!is_top_level_pair(string_node));
    }

    #[test]
    fn enclosing_table_name_returns_none_when_outer_name_missing() {
        let src = r#"{"columns":[{"name":"email"}]}"#;
        let tree = parse_json(src);
        let pair = first_pair(&tree);

        assert!(enclosing_table_name(pair, src).is_none());
    }

    #[test]
    fn direct_child_value_returns_none_for_missing_key() {
        let src = r#"{"name":"u"}"#;
        let tree = parse_json(src);
        let object = first_node(tree.root_node(), "object").unwrap();

        assert!(direct_child_value(object, src, "ref_table").is_none());
    }

    #[test]
    fn skip_yaml_wrappers_peels_wrapper_with_parent() {
        let src = "ref_table: user\n";
        let tree = parse_yaml(src);
        let wrapper = first_node(tree.root_node(), "flow_node")
            .or_else(|| first_node(tree.root_node(), "block_node"))
            .expect("YAML scalar wrapper");

        let unwrapped = skip_yaml_wrappers(wrapper).expect("wrapper should peel to mapping pair");
        assert!(
            matches!(unwrapped.kind(), "block_mapping_pair" | "pair"),
            "got: {}",
            unwrapped.kind()
        );
    }
}
