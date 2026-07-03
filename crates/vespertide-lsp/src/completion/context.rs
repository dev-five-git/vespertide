//! Completion context detection via tree-sitter node ancestry.

use std::ops::Range;

use vespertide_planner::{CheckToken, CheckTokenKind, lex_check_expr};

use crate::check_expr_range::expr_inner_range;
use crate::text_util::strip_quotes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Context {
    // ------------------- VALUE positions -------------------
    /// Cursor is inside the string literal of a `type` value. Simple types
    /// insert as-is; complex types overwrite the entire `string_byte_range`
    /// (quotes included) with an object literal.
    ColumnTypeInString {
        string_byte_range: std::ops::Range<usize>,
    },
    /// Cursor is at the bare value slot of `type`: both simple strings and
    /// complex object snippets are valid.
    ColumnTypeValue,
    Nullable,
    PrimaryKey,
    Unique,
    OnDeleteAction,
    OnUpdateAction,
    RefTable,
    RefColumns {
        ref_table: String,
    },
    /// Cursor is on the value of `kind` inside a complex `type` object
    /// (`varchar` / `char` / `numeric` / `enum` / `custom`). When the
    /// cursor sits inside a `"..."` literal, the suggested label replaces
    /// the whole literal so partial typing is cleaned up.
    TypeKind {
        string_byte_range: Option<std::ops::Range<usize>>,
    },
    /// Cursor is at a column's `default` value. The candidate set depends on
    /// the sibling `type`: enum gets its `values` quoted, timestamp gets
    /// `now()`/`CURRENT_TIMESTAMP`, uuid gets `gen_random_uuid()`, etc.
    DefaultValue {
        /// Either a simple type name (`"integer"`, `"timestamp"`, ...) or the
        /// `kind` of a complex `type` object (`"varchar"`, `"enum"`, ...).
        type_kind: Option<String>,
        /// String enum members or stringified integer enum names. Empty
        /// unless the sibling `type.kind == "enum"`.
        enum_values: Vec<String>,
        /// When the cursor sits inside a `"..."` literal, this is the byte
        /// range of that string (quotes included). Completions use it as
        /// the `TextEdit` range so accepting a suggestion wipes the
        /// existing literal instead of appending to it.
        string_byte_range: Option<Range<usize>>,
    },
    /// Cursor is inside a table-level CHECK expression string
    /// (`constraints[*].expr`). The position decides whether we suggest
    /// operands (columns), operators/SQL keywords, or a partial-column edit.
    CheckExpr {
        table_columns: Vec<String>,
        position: CheckExprPos,
        replace_range_bytes: Option<Range<usize>>,
    },

    // ------------------- KEY positions ---------------------
    /// New key inside the top-level table object.
    TableTopLevelKey,
    /// New key inside a column object (`columns[N]`).
    ColumnObjectKey,
    /// New key inside a `foreign_key` object.
    ForeignKeyObjectKey,
    /// New key inside a complex `type` object (varchar/numeric/enum/...).
    TypeObjectKey,

    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CheckExprPos {
    Operand,
    Operator,
    PartialColumn { prefix: String },
}

pub(super) fn detect(tree: &tree_sitter::Tree, source: &str, byte_offset: usize) -> Context {
    let node = node_at_byte(tree, byte_offset);
    // KEY position completions take priority over VALUE position logic so
    // that typing `"` at an object boundary offers the right key set.
    if let Some(ctx) = classify_key_context(node, source) {
        return ctx;
    }

    let path = collect_key_path(node, source);
    classify_path(&path, node, source, byte_offset)
}

fn classify_path(
    path: &[String],
    cursor_node: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Context {
    let last = path.last().map(String::as_str);
    let has = |key: &str| path.iter().any(|part| part == key);

    match last {
        Some("type") if has("columns") => {
            if let Some(range) = enclosing_string_range(cursor_node) {
                Context::ColumnTypeInString {
                    string_byte_range: range,
                }
            } else {
                Context::ColumnTypeValue
            }
        }
        Some("nullable") if has("columns") => Context::Nullable,
        Some("primary_key") if has("columns") => Context::PrimaryKey,
        Some("unique") if has("columns") => Context::Unique,
        // `kind` is only meaningful inside `columns[*].type` — guard on
        // both keys so an arbitrary nested `kind` (e.g. inside someone's
        // custom JSON) does not accidentally match.
        Some("kind") if has("columns") && has("type") => Context::TypeKind {
            string_byte_range: enclosing_string_range(cursor_node),
        },
        Some("on_delete") => Context::OnDeleteAction,
        Some("on_update") => Context::OnUpdateAction,
        Some("ref_table") => Context::RefTable,
        Some("ref_columns") => Context::RefColumns {
            ref_table: sibling_ref_table(cursor_node, source).unwrap_or_default(),
        },
        Some("default") if has("columns") => {
            let (type_kind, enum_values) = analyze_sibling_type(cursor_node, source);
            Context::DefaultValue {
                type_kind,
                enum_values,
                string_byte_range: enclosing_string_range(cursor_node),
            }
        }
        Some("expr") if has("constraints") => {
            check_expr_context(cursor_node, source, byte_offset).unwrap_or(Context::None)
        }
        _ => Context::None,
    }
}

fn check_expr_context(
    cursor: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Option<Context> {
    let expr_pair = enclosing_pair_with_key(cursor, source, "expr")?;
    if !is_inside_constraints(expr_pair, source) {
        return None;
    }

    let expr_value = expr_pair.named_child(1).map(unwrap_flow_node)?;
    let inner = expr_inner_range(expr_value)?;
    if byte_offset < inner.start || byte_offset > inner.end {
        return None;
    }

    let expr_text = source.get(inner.clone())?;
    let cursor_rel = clamp_to_char_boundary(
        expr_text,
        byte_offset.saturating_sub(inner.start).min(expr_text.len()),
    );
    let table_columns = current_table_columns(cursor, source);
    let (position, replace_range_bytes) =
        classify_check_expr_position(expr_text, inner.start, cursor_rel, &table_columns);

    Some(Context::CheckExpr {
        table_columns,
        position,
        replace_range_bytes,
    })
}

fn classify_check_expr_position(
    expr_text: &str,
    inner_start: usize,
    cursor_rel: usize,
    table_columns: &[String],
) -> (CheckExprPos, Option<Range<usize>>) {
    let prefix = &expr_text[..cursor_rel];
    if prefix.trim().is_empty() {
        return (CheckExprPos::Operand, None);
    }

    let tokens = lex_check_expr(prefix);
    if let Some(last) = tokens.last() {
        if let Some((typed_prefix, replace_range)) =
            partial_column_at_cursor(prefix, last, cursor_rel, inner_start, table_columns)
        {
            return (
                CheckExprPos::PartialColumn {
                    prefix: typed_prefix,
                },
                Some(replace_range),
            );
        }

        if token_expects_operand(last, prefix) {
            (CheckExprPos::Operand, None)
        } else {
            (CheckExprPos::Operator, None)
        }
    } else {
        (CheckExprPos::Operand, None)
    }
}

fn partial_column_at_cursor(
    prefix: &str,
    token: &CheckToken,
    cursor_rel: usize,
    inner_start: usize,
    table_columns: &[String],
) -> Option<(String, Range<usize>)> {
    if token.kind != CheckTokenKind::Column || token.span.end != cursor_rel {
        return None;
    }

    let typed_prefix = prefix.get(token.span.clone())?;
    if typed_prefix.is_empty() || table_columns.iter().any(|column| column == typed_prefix) {
        return None;
    }

    let replace_range = (inner_start + token.span.start)..(inner_start + token.span.end);
    Some((typed_prefix.to_string(), replace_range))
}

fn token_expects_operand(token: &CheckToken, expr_prefix: &str) -> bool {
    let text = expr_prefix.get(token.span.clone()).unwrap_or_default();
    match token.kind {
        CheckTokenKind::Operator => true,
        CheckTokenKind::Punctuation => matches!(text, "(" | ","),
        CheckTokenKind::Keyword => keyword_expects_operand(text),
        CheckTokenKind::Column | CheckTokenKind::Number | CheckTokenKind::String => false,
    }
}

fn keyword_expects_operand(keyword: &str) -> bool {
    ["AND", "OR", "NOT", "IN", "BETWEEN"]
        .iter()
        .any(|expected| keyword.eq_ignore_ascii_case(expected))
}

fn is_inside_constraints(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_pair(candidate) && key_text(candidate, source) == Some("constraints") {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn current_table_columns(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let root = document_value(node);
    if let Some(columns_value_raw) = find_pair_with_key(root, source, "columns")
        .and_then(|columns_pair| columns_pair.named_child(1))
    {
        collect_column_names(unwrap_flow_node(columns_value_raw), source)
    } else {
        Vec::new()
    }
}

fn document_value(mut node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }

    node.named_child(0).map_or(node, unwrap_flow_node)
}

fn collect_column_names(columns_value: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    if matches!(
        columns_value.kind(),
        "array" | "block_sequence" | "flow_sequence"
    ) {
        let mut cursor = columns_value.walk();
        for raw_child in columns_value.children(&mut cursor) {
            let child = unwrap_flow_node(raw_child);
            if let Some(column_object) = column_object_from_sequence_child(child)
                && let Some(name) = string_value_for_key(column_object, source, "name")
                && !name.is_empty()
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn column_object_from_sequence_child(
    child: tree_sitter::Node<'_>,
) -> Option<tree_sitter::Node<'_>> {
    match child.kind() {
        "object" | "block_mapping" | "flow_mapping" => Some(child),
        "block_sequence_item" => {
            let mut cursor = child.walk();
            child.children(&mut cursor).find_map(|raw_inner| {
                let inner = unwrap_flow_node(raw_inner);
                matches!(inner.kind(), "object" | "block_mapping" | "flow_mapping").then_some(inner)
            })
        }
        _ => None,
    }
}

fn string_value_for_key<'source>(
    object: tree_sitter::Node<'_>,
    source: &'source str,
    key: &str,
) -> Option<&'source str> {
    let pair = find_pair_with_key(object, source, key)?;
    let value = pair.named_child(1).map(unwrap_flow_node)?;
    source.get(value.byte_range()).map(strip_quotes)
}

fn clamp_to_char_boundary(text: &str, mut byte_offset: usize) -> usize {
    while byte_offset > 0 && !text.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    byte_offset
}

/// Walk to the enclosing column object and inspect its `type` sibling.
/// Returns `(type_kind, enum_values)` where `type_kind` is either the simple
/// type name (`"integer"`) or the complex object's `kind` (`"varchar"` /
/// `"enum"`), and `enum_values` is the value list when `kind == "enum"`.
fn analyze_sibling_type(
    cursor: tree_sitter::Node<'_>,
    source: &str,
) -> (Option<String>, Vec<String>) {
    if let Some(type_value) = enclosing_column_object(cursor)
        .and_then(|column_object| find_pair_with_key(column_object, source, "type"))
        .and_then(|type_pair| type_pair.named_child(1))
    {
        let effective = unwrap_flow_node(type_value);
        match effective.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                let raw = source.get(effective.byte_range()).unwrap_or("");
                (Some(strip_quotes(raw).to_string()), Vec::new())
            }
            "object" | "block_mapping" | "flow_mapping" => {
                let kind = find_pair_with_key(effective, source, "kind")
                    .and_then(|pair| pair.named_child(1))
                    .map(unwrap_flow_node)
                    .and_then(|node| source.get(node.byte_range()))
                    .map(|raw| strip_quotes(raw).to_string());

                let enum_values = if kind.as_deref() == Some("enum") {
                    collect_enum_values(effective, source)
                } else {
                    Vec::new()
                };
                (kind, enum_values)
            }
            _ => (None, Vec::new()),
        }
    } else {
        (None, Vec::new())
    }
}

/// tree-sitter-yaml wraps scalars in `flow_node` (inline values) and
/// multi-line mappings/sequences in `block_node`. Both are pure wrappers
/// over their first named child — peel them so downstream `match`es see
/// the real kind. We loop to handle the (rare) double-wrapping case.
/// JSON's grammar has no such wrapper, so this is a no-op there.
fn unwrap_flow_node(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    // Fused while-let so the empty-wrapper case shares the same exit as the
    // kind-mismatch case — no defensive `return current` branch dependent on
    // tree-sitter-yaml producing empty wrappers.
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node")
        && let Some(inner) = current
            .named_child(0)
            .filter(|inner| inner.id() != current.id())
    {
        current = inner;
    }
    current
}

/// Walk up to the smallest enclosing object that lives inside a `columns`
/// array — i.e. the column object the cursor belongs to.
fn enclosing_column_object(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn find_pair_with_key<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &str,
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object
        .children(&mut cursor)
        .find(|&child| is_pair(child) && key_text(child, source) == Some(target_key))
}

fn collect_enum_values(type_object: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(values_array_raw) = find_pair_with_key(type_object, source, "values")
        .and_then(|values_pair| values_pair.named_child(1))
    {
        let values_array = unwrap_flow_node(values_array_raw);
        if matches!(
            values_array.kind(),
            "array" | "block_sequence" | "flow_sequence"
        ) {
            let mut cursor = values_array.walk();
            for raw_child in values_array.children(&mut cursor) {
                let child = unwrap_flow_node(raw_child);
                match child.kind() {
                    "string"
                    | "double_quote_scalar"
                    | "single_quote_scalar"
                    | "string_scalar"
                    | "plain_scalar" => {
                        if let Some(raw) = source.get(child.byte_range()) {
                            out.push(strip_quotes(raw).to_string());
                        }
                    }
                    // Integer-enum members are objects of the form `{name: "...", value: N}`.
                    "object" | "block_mapping" | "flow_mapping" => {
                        if let Some(name_pair) = find_pair_with_key(child, source, "name")
                            && let Some(name_value_raw) = name_pair.named_child(1)
                        {
                            let name_value = unwrap_flow_node(name_value_raw);
                            if let Some(raw) = source.get(name_value.byte_range()) {
                                out.push(strip_quotes(raw).to_string());
                            }
                        }
                    }
                    // YAML block sequence items show up as `block_sequence_item` →
                    // `flow_node` or `block_mapping`; recurse one level so they are
                    // not silently skipped.
                    "block_sequence_item" => {
                        let mut inner_cursor = child.walk();
                        for inner in child.children(&mut inner_cursor) {
                            let inner = unwrap_flow_node(inner);
                            if let Some(raw) = source.get(inner.byte_range())
                                && matches!(
                                    inner.kind(),
                                    "string"
                                        | "double_quote_scalar"
                                        | "single_quote_scalar"
                                        | "string_scalar"
                                        | "plain_scalar"
                                )
                            {
                                out.push(strip_quotes(raw).to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Decide whether the cursor sits at a place where a new object key would
/// be typed, and if so which set of keys is appropriate.
fn classify_key_context(cursor: tree_sitter::Node<'_>, source: &str) -> Option<Context> {
    if !is_at_pair_key_position(cursor) {
        return None;
    }

    let path = enclosing_object_parent_path(cursor, source);
    Some(match path.last().map(String::as_str) {
        Some("foreign_key") => Context::ForeignKeyObjectKey,
        Some("type") => Context::TypeObjectKey,
        Some("columns") => Context::ColumnObjectKey,
        None => Context::TableTopLevelKey,
        // Unknown nested object (e.g. inside enum values, table-level
        // constraints) — fall through to value-based classification.
        _ => return None,
    })
}

/// True when the cursor sits inside a pair's KEY string, or directly inside
/// an object body between pairs (where a new key would be typed).
fn is_at_pair_key_position(cursor: tree_sitter::Node<'_>) -> bool {
    let cursor_start = cursor.start_byte();
    let cursor_end = cursor.end_byte();

    let mut current = Some(cursor);
    while let Some(candidate) = current {
        if is_pair(candidate) {
            return candidate.named_child(0).is_some_and(|key| {
                let range = key.byte_range();
                cursor_start >= range.start && cursor_end <= range.end
            });
        }
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// Walk upward from the cursor, find the smallest enclosing object, and
/// return the ancestor pair-key path ABOVE that object (excluding any pair
/// that the cursor itself is the key of).
fn enclosing_object_parent_path(cursor: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let object = {
        let mut current = Some(cursor);
        loop {
            match current {
                Some(node)
                    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") =>
                {
                    break Some(node);
                }
                Some(node) => current = node.parent(),
                None => break None,
            }
        }
    };
    let mut path = Vec::new();
    if let Some(object) = object {
        let mut current = object.parent();
        while let Some(candidate) = current {
            if is_pair(candidate)
                && let Some(key) = key_text(candidate, source)
            {
                path.push(key.to_string());
            }
            current = candidate.parent();
        }
    }
    path.reverse();
    path
}

/// Walk upward from the cursor and return the byte range of the enclosing
/// JSON/YAML string literal (quotes included for quoted variants), or `None`
/// if the cursor is not inside a string. Stops at the first container
/// boundary so nested object values never count as "in string".
fn enclosing_string_range(node: tree_sitter::Node<'_>) -> Option<std::ops::Range<usize>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            // `string_content` is the inner span without quotes — climb one
            // more level so we capture the surrounding quotes too.
            "string_content" => {
                return Some(
                    candidate
                        .parent()
                        .filter(|parent| parent.kind() == "string")
                        .unwrap_or(candidate)
                        .byte_range(),
                );
            }
            // All other scalar variants are returned as-is. Quoted JSON/YAML
            // scalars (`string`, `double_quote_scalar`, `single_quote_scalar`)
            // already include their delimiters; unquoted YAML scalars
            // (`string_scalar`, `plain_scalar`) have no quotes to begin with
            // but should still be replaced wholesale when expanding into an
            // object literal.
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                return Some(candidate.byte_range());
            }
            "pair" | "block_mapping_pair" | "object" | "array" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => {
                return None;
            }
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn collect_key_path(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = Some(node);

    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = key_text(candidate, source)
        {
            path.push(key.to_string());
        }
        current = candidate.parent();
    }

    path.reverse();
    path
}

fn sibling_ref_table(node: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    let ref_columns_pair = enclosing_pair_with_key(node, source, "ref_columns")?;
    let parent = ref_columns_pair.parent()?;
    direct_child_value(parent, source, "ref_table").map(ToString::to_string)
}

fn enclosing_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = Some(node);

    while let Some(candidate) = current {
        if is_pair(candidate) && key_text(candidate, source) == Some(expected) {
            return Some(candidate);
        }
        current = candidate.parent();
    }

    None
}

fn direct_child_value<'source>(
    node: tree_sitter::Node<'_>,
    source: &'source str,
    expected_key: &str,
) -> Option<&'source str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_text(child, source) == Some(expected_key) {
            return value_text(child, source);
        }
    }

    None
}

fn key_text<'source>(
    pair_node: tree_sitter::Node<'_>,
    source: &'source str,
) -> Option<&'source str> {
    let key = pair_node.named_child(0)?;
    source.get(key.byte_range()).map(strip_quotes)
}

fn value_text<'source>(
    pair_node: tree_sitter::Node<'_>,
    source: &'source str,
) -> Option<&'source str> {
    let value = pair_node.named_child(1)?;
    source.get(value.byte_range()).map(strip_quotes)
}

fn node_at_byte(tree: &tree_sitter::Tree, byte_offset: usize) -> tree_sitter::Node<'_> {
    let root = tree.root_node();
    if root.end_byte() == 0 {
        return root;
    }

    let start = byte_offset
        .saturating_sub(1)
        .min(root.end_byte().saturating_sub(1));
    let end = byte_offset.min(root.end_byte());
    root.descendant_for_byte_range(start, end).unwrap_or(root)
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::DocumentFormat;
    use crate::store::DocumentStore;
    use crate::test_support::*;
    use crate::workspace_index::WorkspaceIndex;
    use rstest::rstest;

    fn detect_json(src: &str, needle: &str, advance: usize) -> Context {
        let tree = parse(src, DocumentFormat::Json);
        let pos = src.find(needle).unwrap() + advance;
        detect(&tree, src, pos)
    }

    fn run_completion(
        src: &str,
        format: DocumentFormat,
        pos: usize,
    ) -> Vec<super::super::DomainCompletion> {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let tree = parse(src, format);
        super::super::compute(src, format, Some(&tree), &idx, &docs, pos)
    }

    fn completion_labels(items: &[super::super::DomainCompletion]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    fn assert_label_expectations(
        labels: &[&str],
        expected_present: &[&str],
        expected_absent: &[&str],
        expected_any: &[&str],
    ) {
        for &expected in expected_present {
            assert!(
                labels.contains(&expected),
                "expected `{expected}` in labels: {labels:?}"
            );
        }
        for &unexpected in expected_absent {
            assert!(
                !labels.contains(&unexpected),
                "unexpected `{unexpected}` in labels: {labels:?}"
            );
        }
        if !expected_any.is_empty() {
            assert!(
                expected_any
                    .iter()
                    .any(|expected| labels.contains(expected)),
                "expected one of {expected_any:?} in labels: {labels:?}"
            );
        }
    }

    #[test]
    fn classify_path_column_type_uses_ancestor_key_predicate() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse(src, DocumentFormat::Json);
        let pos = src.find("integer").unwrap();
        let node = node_at_byte(&tree, pos);
        let path = ["columns".to_string(), "type".to_string()];

        let ctx = classify_path(&path, node, src, pos);

        assert!(
            matches!(ctx, Context::ColumnTypeInString { .. }),
            "got: {ctx:?}"
        );
    }

    fn find_pair_recursive<'tree>(
        node: tree_sitter::Node<'tree>,
        source: &str,
        key: &str,
    ) -> Option<tree_sitter::Node<'tree>> {
        if is_pair(node) && key_text(node, source) == Some(key) {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_pair_recursive(child, source, key) {
                return Some(found);
            }
        }
        None
    }

    #[rstest]
    #[case::partial_column_no_match(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"xy"}]}"#, r#""expr":"xy""#, 9, &[], &["id", "age"], &[])]
    #[case::after_open_paren_punctuation(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"(id > 0) AND ("}]}"#, "AND (", 5, &[], &[], &["id", "age"])]
    #[case::after_keyword_not(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"NOT "}]}"#, r#""expr":"NOT ""#, 12, &["id"], &[], &[])]
    #[case::after_column(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"id "}]}"#, r#""expr":"id ""#, 11, &["=", "AND", "BETWEEN"], &[], &[])]
    #[case::after_number(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"id > 5 "}]}"#, r#""expr":"id > 5 ""#, 15, &["AND", "OR"], &[], &[])]
    #[case::outside_expr_inner(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"id > 0"}]}"#, r#""expr":"id > 0""#, r#""expr":"id > 0""#.len(), &[], &["AND"], &[])]
    #[case::yaml_block_scalar("name: u\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\nconstraints:\n  - type: check\n    name: c\n    expr: |\n      id > \n", "id > ", "id > ".len(), &[], &[], &["NOT", "("])]
    fn check_expr_completion_cases(
        #[case] src: &str,
        #[case] needle: &str,
        #[case] advance: usize,
        #[case] expected_present: &[&str],
        #[case] expected_absent: &[&str],
        #[case] expected_any: &[&str],
    ) {
        let format = if src.starts_with('{') {
            DocumentFormat::Json
        } else {
            DocumentFormat::Yaml
        };
        let pos = src.find(needle).unwrap() + advance;
        let items = run_completion(src, format, pos);
        let labels = completion_labels(&items);

        assert_label_expectations(&labels, expected_present, expected_absent, expected_any);
    }

    #[test]
    fn empty_document_yields_no_completion() {
        let items = run_completion("", DocumentFormat::Json, 0);

        assert!(items.is_empty());
    }

    #[rstest]
    #[case::unknown_top_level_value(
        r#"{"unknown_key": ""}"#,
        r#""unknown_key": """#,
        16,
        "integer"
    )]
    #[case::unknown_parent_object_key(r#"{"name":"u","columns":[{"name":"s","type":{"kind":"enum","name":"x","values":[{"":""}]}}]}"#, r#"[{"":""#, 3, "primary_key")]
    fn unhandled_contexts_do_not_offer_standard_completion(
        #[case] src: &str,
        #[case] needle: &str,
        #[case] advance: usize,
        #[case] unexpected: &str,
    ) {
        let pos = src.find(needle).unwrap() + advance;
        let items = run_completion(src, DocumentFormat::Json, pos);

        assert!(
            items.iter().all(|item| item.label != unexpected),
            "unexpected `{unexpected}` completion, got: {:?}",
            items.iter().map(|item| &item.label).collect::<Vec<_>>()
        );
    }

    #[rstest]
    #[case::without_ref_table(false, r#"{"name":"p","columns":[{"name":"x","type":"integer","foreign_key":{"ref_columns":[""]}}]}"#, &[])]
    #[case::open_doc_ref_table(true, r#"{"name":"post","columns":[{"name":"x","type":"integer","foreign_key":{"ref_table":"user","ref_columns":[""]}}]}"#, &["id"])]
    fn ref_columns_completion_cases(
        #[case] open_user_doc: bool,
        #[case] post_src: &str,
        #[case] expected_labels: &[&str],
    ) {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        if open_user_doc {
            let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
            let user_uri = uri("user.json");
            let user_tree = parse(user_src, DocumentFormat::Json);
            idx.upsert(&user_uri, user_src, &user_tree);
            docs.open(user_uri, "json".to_string(), 1, user_src.to_string());
        }

        let post_tree = parse(post_src, DocumentFormat::Json);
        let pos = post_src.find(r#"[""#).unwrap() + 2;
        let items = super::super::compute(
            post_src,
            DocumentFormat::Json,
            Some(&post_tree),
            &idx,
            &docs,
            pos,
        );
        let labels = completion_labels(&items);

        if expected_labels.is_empty() {
            assert!(
                items.is_empty(),
                "expected no ref_columns completions, got: {:?}",
                items.iter().map(|item| &item.label).collect::<Vec<_>>()
            );
        } else {
            for &expected in expected_labels {
                assert!(
                    labels.contains(&expected),
                    "expected `{expected}` ref column, got: {labels:?}"
                );
            }
        }
    }

    #[test]
    fn expr_key_outside_constraints_is_not_check_context() {
        let src = r#"{"name":"u","expr":"id > 0","columns":[{"name":"id","type":"integer"}]}"#;
        assert_eq!(detect_json(src, "id > 0", 2), Context::None);
    }

    #[test]
    fn check_expr_position_handles_unlexable_and_exact_column_prefixes() {
        let (position, replace) = classify_check_expr_position("@", 10, 1, &["id".to_string()]);
        assert_eq!(position, CheckExprPos::Operand);
        assert!(replace.is_none());
        let (position, replace) = classify_check_expr_position("id", 20, 2, &["id".to_string()]);
        assert_eq!(position, CheckExprPos::Operator);
        assert!(replace.is_none());
        assert_eq!(clamp_to_char_boundary("é", 1), 0);
    }

    #[test]
    fn malformed_columns_shapes_return_empty_column_lists() {
        let missing_value =
            r#"{"name":"u","columns":,"constraints":[{"type":"check","name":"c","expr":""}]}"#;
        let tree = parse(missing_value, DocumentFormat::Json);
        let pos = missing_value.find(r#""expr":""#).unwrap() + 8;
        let ctx = detect(&tree, missing_value, pos);
        assert!(
            matches!(ctx, Context::CheckExpr { table_columns, .. } if table_columns.is_empty())
        );
        let not_array =
            r#"{"name":"u","columns":"bad","constraints":[{"type":"check","name":"c","expr":""}]}"#;
        let tree = parse(not_array, DocumentFormat::Json);
        let pos = not_array.find(r#""expr":""#).unwrap() + 8;
        let ctx = detect(&tree, not_array, pos);
        assert!(
            matches!(ctx, Context::CheckExpr { table_columns, .. } if table_columns.is_empty())
        );
    }

    #[test]
    fn yaml_block_sequence_columns_and_enum_values_are_collected() {
        let src = "name: u\ncolumns:\n  - name: status\n    type:\n      kind: enum\n      name: status_kind\n      values:\n        - pending\n        - active\n    default: \"\"\n";
        let tree = parse(src, DocumentFormat::Yaml);
        let pos = src.find("default: \"\"").unwrap() + "default: \"".len();
        let ctx = detect(&tree, src, pos);
        assert!(
            matches!(ctx, Context::DefaultValue { type_kind: Some(kind), enum_values, .. } if kind == "enum" && enum_values == vec!["pending".to_string(), "active".to_string()])
        );
    }

    #[test]
    fn default_context_without_column_or_type_information_is_generic() {
        let top_default = r#"{"name":"u","default":""}"#;
        assert_eq!(
            detect_json(top_default, r#""default":"""#, 11),
            Context::None
        );
        let missing_type_value = r#"{"name":"u","columns":[{"name":"x","type":,"default":""}]}"#;
        let ctx = detect_json(missing_type_value, r#""default":"""#, 11);
        assert!(
            matches!(ctx, Context::DefaultValue { type_kind: None, enum_values, .. } if enum_values.is_empty())
        );
        let array_type = r#"{"name":"u","columns":[{"name":"x","type":[],"default":""}]}"#;
        let ctx = detect_json(array_type, r#""default":"""#, 11);
        assert!(
            matches!(ctx, Context::DefaultValue { type_kind: None, enum_values, .. } if enum_values.is_empty())
        );
    }

    #[test]
    fn enum_values_missing_or_not_sequence_are_empty() {
        let no_values = r#"{"kind":"enum","name":"s"}"#;
        let tree = parse(no_values, DocumentFormat::Json);
        let object = tree
            .root_node()
            .descendant_for_byte_range(0, no_values.len())
            .unwrap();
        assert!(collect_enum_values(document_value(object), no_values).is_empty());
        let scalar_values = r#"{"kind":"enum","name":"s","values":"pending"}"#;
        let tree = parse(scalar_values, DocumentFormat::Json);
        let object = document_value(tree.root_node());
        assert!(collect_enum_values(object, scalar_values).is_empty());
        let missing_value = r#"{"kind":"enum","name":"s","values":}"#;
        let tree = parse(missing_value, DocumentFormat::Json);
        let object = document_value(tree.root_node());
        assert!(collect_enum_values(object, missing_value).is_empty());
    }

    #[test]
    fn path_and_string_helpers_handle_no_container_cases() {
        let src = r#"{"name":"u"}"#;
        let tree = parse(src, DocumentFormat::Json);
        assert!(enclosing_object_parent_path(tree.root_node(), src).is_empty());
        assert!(enclosing_string_range(tree.root_node()).is_none());
        assert!(enclosing_pair_with_key(tree.root_node(), src, "missing").is_none());
    }

    #[test]
    fn direct_check_expr_context_rejects_expr_outside_constraints() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}],"expr":"id > 0"}"#;
        let tree = parse(src, DocumentFormat::Json);
        let pos = src.find("id > 0").unwrap() + 2;
        let node = node_at_byte(&tree, pos);

        assert_eq!(check_expr_context(node, src, pos), None);
    }

    #[test]
    fn yaml_check_expr_collects_block_sequence_column_names() {
        let src = "name: u\ncolumns:\n  - name: amount\n    type: integer\nconstraints:\n  - type: check\n    expr: amount > 0\n";
        let tree = parse(src, DocumentFormat::Yaml);
        let columns_pair = find_pair_recursive(tree.root_node(), src, "columns").unwrap();
        let columns_value = unwrap_flow_node(columns_pair.named_child(1).unwrap());
        let names = collect_column_names(columns_value, src);

        assert_eq!(names, vec!["amount".to_string()]);
    }

    #[test]
    fn malformed_yaml_value_shapes_cover_context_defensive_returns() {
        let missing_columns =
            "name: u\ncolumns:\nconstraints:\n  - type: check\n    expr: age > 0\n";
        let tree = parse(missing_columns, DocumentFormat::Yaml);
        let expr_pair = find_pair_recursive(tree.root_node(), missing_columns, "expr").unwrap();
        assert!(current_table_columns(expr_pair, missing_columns).is_empty());
        assert_eq!(
            analyze_sibling_type(tree.root_node(), missing_columns),
            (None, Vec::new())
        );

        let missing_type = "name: u\ncolumns:\n  - name: x\n    type:\n    default: \"\"\n";
        let tree = parse(missing_type, DocumentFormat::Yaml);
        let default_pair = find_pair_recursive(tree.root_node(), missing_type, "default").unwrap();
        assert_eq!(
            analyze_sibling_type(default_pair, missing_type),
            (None, Vec::new())
        );

        let enum_missing_values = "kind: enum\nvalues:\n";
        let tree = parse(enum_missing_values, DocumentFormat::Yaml);
        let object = document_value(tree.root_node());
        assert!(collect_enum_values(object, enum_missing_values).is_empty());
    }
}
