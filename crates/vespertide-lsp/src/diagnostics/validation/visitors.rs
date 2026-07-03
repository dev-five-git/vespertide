use crate::diagnostics::{DomainDiagnostic, Severity};

#[cfg(test)]
use super::types::find_value_for_key;
use super::types::{
    EnumValueDescriptor, KNOWN_SIMPLE_TYPES, collect_enum_value_descriptors, find_pair_with_key,
    is_pair_node, pair_key_text, scalar_text, strip_quotes_str, unwrap_yaml_node,
};

pub(in crate::diagnostics) fn collect_all(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let mut collector = FusedCollector::new(source_bytes, tree.root_node().has_error());
    collector.walk(tree.root_node(), false, collector.syntax_active);

    out.append(&mut collector.syntax);
    out.append(&mut collector.unknown_types);
    out.append(&mut collector.complex_types);

    if let Some(columns_raw) = collector.columns {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for column in direct_column_objects(columns_raw) {
            inspect_column_name(column, source_bytes, &mut seen, out);
        }
    }
}

struct FusedCollector<'source, 'tree> {
    source: &'source [u8],
    columns: Option<tree_sitter::Node<'tree>>,
    syntax: Vec<DomainDiagnostic>,
    unknown_types: Vec<DomainDiagnostic>,
    complex_types: Vec<DomainDiagnostic>,
    syntax_active: bool,
}

impl<'source, 'tree> FusedCollector<'source, 'tree> {
    fn new(source: &'source [u8], syntax_active: bool) -> Self {
        Self {
            source,
            columns: None,
            syntax: Vec::new(),
            unknown_types: Vec::new(),
            complex_types: Vec::new(),
            syntax_active,
        }
    }

    fn walk(&mut self, node: tree_sitter::Node<'tree>, in_columns: bool, syntax_active: bool) {
        let child_syntax_active = self.inspect_syntax(node, syntax_active);
        if in_columns && matches!(node.kind(), "object" | "block_mapping") {
            inspect_column_type(node, self.source, &mut self.unknown_types);
            inspect_complex_type(node, self.source, &mut self.complex_types);
        }

        let columns_value = self.first_columns_value(node);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let child_in_columns =
                in_columns || columns_value.is_some_and(|value| value.id() == child.id());
            self.walk(child, child_in_columns, child_syntax_active);
        }
    }

    fn inspect_syntax(&mut self, node: tree_sitter::Node<'tree>, syntax_active: bool) -> bool {
        if !syntax_active {
            return false;
        }

        if node.is_error() || node.is_missing() {
            self.syntax.push(DomainDiagnostic {
                byte_range: node.byte_range(),
                severity: Severity::Error,
                message: if node.is_missing() {
                    format!("Missing {}", node.kind())
                } else {
                    "Syntax error".to_string()
                },
                code: "syntax-error".to_string(),
            });
            return false;
        }

        true
    }

    fn first_columns_value(
        &mut self,
        node: tree_sitter::Node<'tree>,
    ) -> Option<tree_sitter::Node<'tree>> {
        if self.columns.is_some()
            || !is_pair_node(node)
            || pair_key_text(node, self.source).is_none_or(|key| key != "columns")
        {
            return None;
        }

        let value = node.named_child(1)?;
        self.columns = Some(value);
        Some(value)
    }
}

#[cfg(test)]
pub(in crate::diagnostics) fn collect_syntax_errors(
    tree: &tree_sitter::Tree,
    out: &mut Vec<DomainDiagnostic>,
) {
    let root = tree.root_node();
    if root.has_error() {
        walk_for_errors(root, out);
    }
}

/// Tree-sitter-based pre-pass that flags unknown column types with a
/// precise byte range pointing at the offending `type` value. Runs before
/// serde so users see the squiggle on the right line even when serde's
/// untagged-enum error reports a misleading position.
#[cfg(test)]
pub(in crate::diagnostics) fn collect_unknown_column_types(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let Some(columns) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return;
    };

    walk_column_objects(columns, source_bytes, out);
}

#[cfg(test)]
fn walk_column_objects(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "object" | "block_mapping") {
            inspect_column_type(child, source, out);
        }
        walk_column_objects(child, source, out);
    }
}

fn inspect_column_type(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    if let Some(type_pair) = find_pair_with_key(column, source, "type")
        && let Some(type_value_raw) = type_pair.named_child(1)
    {
        // tree-sitter-yaml wraps every value in a `flow_node` / `block_node`.
        // Peel those wrappers so we see the real scalar or mapping underneath.
        let type_value = unwrap_yaml_node(type_value_raw);

        // Object form (`{kind: ...}`) is checked by serde + planner — skip here.
        if !matches!(
            type_value.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) && let Some(text) = source
            .get(type_value.byte_range())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            let stripped = strip_quotes_str(text);

            // Skip empty placeholder while the user is typing.
            if !stripped.is_empty() && !KNOWN_SIMPLE_TYPES.contains(&stripped) {
                out.push(DomainDiagnostic {
                    byte_range: type_pair.byte_range(),
                    severity: Severity::Error,
                    message: format!(
                        "Unknown column type `{stripped}`. Expected one of: {} \
                         — or a complex type object such as {{\"kind\":\"varchar\",\"length\":255}}",
                        KNOWN_SIMPLE_TYPES.join(", ")
                    ),
                    code: "unknown-type".to_string(),
                });
            }
        }
    }
}

/// Tree-sitter-based pre-pass that flags two columns sharing a `name`.
/// Pinpoints the SECOND (and later) occurrence so the user sees the
/// squiggle on the offending column, not on the table.
///
/// Critically, we visit ONLY the direct elements of the `columns` array.
/// A naive recursive walk would dive into nested objects (e.g. integer
/// enum members like `{"name":"low","value":0}` inside `type.values`) and
/// mistakenly compare their `name` against the column names.
#[cfg(test)]
pub(in crate::diagnostics) fn collect_duplicate_column_names(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let Some(columns_raw) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return;
    };

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for column in direct_column_objects(columns_raw) {
        inspect_column_name(column, source_bytes, &mut seen, out);
    }
}

/// Resolve `columns: [...]` value to the direct list of column mapping
/// nodes — peeling through tree-sitter-yaml's wrappers and skipping
/// punctuation / comments.
fn direct_column_objects(columns_value: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let array = unwrap_yaml_node(columns_value);
    if !matches!(array.kind(), "array" | "block_sequence" | "flow_sequence") {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = array.walk();
    for raw_child in array.children(&mut cursor) {
        let child = unwrap_yaml_node(raw_child);
        match child.kind() {
            "object" | "block_mapping" | "flow_mapping" => out.push(child),
            // YAML block sequence items wrap each element in
            // `block_sequence_item` → mapping. Recurse exactly one level.
            "block_sequence_item" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let inner = unwrap_yaml_node(inner);
                    if matches!(inner.kind(), "object" | "block_mapping" | "flow_mapping") {
                        out.push(inner);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn inspect_column_name(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    seen: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<DomainDiagnostic>,
) {
    if let Some(name_pair) = find_pair_with_key(column, source, "name")
        && let Some(name_value_raw) = name_pair.named_child(1)
    {
        let name_value = unwrap_yaml_node(name_value_raw);
        if let Some(text) = source
            .get(name_value.byte_range())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            let name = strip_quotes_str(text).to_string();
            if !name.is_empty() && !seen.insert(name.clone()) {
                out.push(DomainDiagnostic {
                    byte_range: name_value.byte_range(),
                    severity: Severity::Error,
                    message: format!("Duplicate column name `{name}` in this table"),
                    code: "duplicate-column".to_string(),
                });
            }
        }
    }
}

/// Tree-sitter-based pre-pass for COMPLEX (object-form) column types.
///
/// Catches things serde either silently allows or reports at a misleading
/// byte position:
///   * `kind` is missing / empty / unknown.
///   * `varchar` / `char` without `length`.
///   * `numeric` without `precision` or `scale`.
///   * `enum` without `name`, without `values`, with an empty `values`, or
///     with duplicate string variants / duplicate integer variant names.
///   * `custom` without `custom_type`.
///
/// Each diagnostic gets a precise byte range covering the offending pair so
/// the squiggle lands on the right line.
#[cfg(test)]
pub(in crate::diagnostics) fn collect_complex_type_violations(
    tree: &tree_sitter::Tree,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let source_bytes = source.as_bytes();
    let Some(columns) = find_value_for_key(tree.root_node(), source_bytes, "columns") else {
        return;
    };
    walk_columns_for_complex_type(columns, source_bytes, out);
}

#[cfg(test)]
fn walk_columns_for_complex_type(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "object" | "block_mapping") {
            inspect_complex_type(child, source, out);
        }
        walk_columns_for_complex_type(child, source, out);
    }
}

fn inspect_complex_type(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let Some(type_pair) = find_pair_with_key(column, source, "type") else {
        return;
    };
    if let Some(type_value_raw) = type_pair.named_child(1) {
        let type_value = unwrap_yaml_node(type_value_raw);
        if !matches!(
            type_value.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return;
        }

        // `kind` is mandatory.
        let Some(kind_pair) = find_pair_with_key(type_value, source, "kind") else {
            push_complex(
                out,
                type_pair.byte_range(),
                "Type object requires a `kind` field (varchar, char, numeric, enum, custom)",
            );
            return;
        };
        let kind = match scalar_text(kind_pair, source) {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => {
                push_complex(
                    out,
                    kind_pair.byte_range(),
                    "`kind` must be a non-empty string",
                );
                return;
            }
        };

        match kind.as_str() {
            "varchar" | "char" => check_length_required(type_value, type_pair, &kind, source, out),
            "numeric" => check_numeric_precision_scale(type_value, type_pair, source, out),
            "enum" => check_enum_shape(type_value, type_pair, source, out),
            "custom" => check_custom_type(type_value, type_pair, source, out),
            other => {
                push_complex(
                    out,
                    kind_pair.byte_range(),
                    &format!(
                        "Unknown type kind `{other}`. Expected: varchar, char, numeric, enum, custom"
                    ),
                );
            }
        }
    }
}

fn check_length_required(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    kind: &str,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    if find_pair_with_key(type_value, source, "length").is_none() {
        push_complex(
            out,
            type_pair.byte_range(),
            &format!("`{kind}` type requires a `length` field"),
        );
    }
}

fn check_numeric_precision_scale(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut missing = Vec::new();
    if find_pair_with_key(type_value, source, "precision").is_none() {
        missing.push("precision");
    }
    if find_pair_with_key(type_value, source, "scale").is_none() {
        missing.push("scale");
    }
    if !missing.is_empty() {
        push_complex(
            out,
            type_pair.byte_range(),
            &format!(
                "`numeric` type requires {} field{}",
                missing.join(" and "),
                if missing.len() > 1 { "s" } else { "" }
            ),
        );
    }
}

fn check_enum_shape(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    let name_pair = find_pair_with_key(type_value, source, "name");
    let values_pair = find_pair_with_key(type_value, source, "values");

    let mut missing = Vec::new();
    if name_pair.is_none() {
        missing.push("name");
    }
    if values_pair.is_none() {
        missing.push("values");
    }
    if !missing.is_empty() {
        push_complex(
            out,
            type_pair.byte_range(),
            &format!(
                "`enum` type requires field{}: {}",
                if missing.len() > 1 { "s" } else { "" },
                missing.join(", ")
            ),
        );
        return;
    }

    let values_pair = values_pair.unwrap();
    if let Some(values_value_raw) = values_pair.named_child(1) {
        let values_value = unwrap_yaml_node(values_value_raw);
        if !matches!(
            values_value.kind(),
            "array" | "block_sequence" | "flow_sequence"
        ) {
            push_complex(
                out,
                values_pair.byte_range(),
                "`values` must be a non-empty array",
            );
            return;
        }

        let elements = collect_enum_value_descriptors(values_value, source);
        if elements.is_empty() {
            push_complex(
                out,
                values_pair.byte_range(),
                "`enum` requires a non-empty `values` array",
            );
            return;
        }

        check_duplicate_enum_values(&elements, out);
    }
}

fn check_custom_type(
    type_value: tree_sitter::Node<'_>,
    type_pair: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<DomainDiagnostic>,
) {
    if find_pair_with_key(type_value, source, "custom_type").is_none() {
        push_complex(
            out,
            type_pair.byte_range(),
            "`custom` type requires a `custom_type` SQL string",
        );
    }
}

fn check_duplicate_enum_values(
    descriptors: &[EnumValueDescriptor],
    out: &mut Vec<DomainDiagnostic>,
) {
    let mut seen_names: std::collections::BTreeMap<&str, std::ops::Range<usize>> =
        std::collections::BTreeMap::new();
    for descriptor in descriptors {
        if let Some(_prev) =
            seen_names.insert(descriptor.name.as_str(), descriptor.byte_range.clone())
        {
            push_complex(
                out,
                descriptor.byte_range.clone(),
                &format!("Duplicate enum value `{}`", descriptor.name),
            );
        }
    }

    // Integer enums: also catch duplicate numeric values.
    let mut seen_values: std::collections::BTreeMap<String, std::ops::Range<usize>> =
        std::collections::BTreeMap::new();
    for descriptor in descriptors {
        let Some(value) = &descriptor.integer_value else {
            continue;
        };
        if seen_values
            .insert(value.clone(), descriptor.integer_value_range.clone())
            .is_some()
        {
            push_complex(
                out,
                descriptor.integer_value_range.clone(),
                &format!("Duplicate enum numeric value `{value}`"),
            );
        }
    }
}

fn push_complex(
    out: &mut Vec<DomainDiagnostic>,
    byte_range: std::ops::Range<usize>,
    message: &str,
) {
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Error,
        message: message.to_string(),
        code: "complex-type".to_string(),
    });
}

#[cfg(test)]
fn walk_for_errors(node: tree_sitter::Node<'_>, out: &mut Vec<DomainDiagnostic>) {
    if node.is_error() || node.is_missing() {
        out.push(DomainDiagnostic {
            byte_range: node.byte_range(),
            severity: Severity::Error,
            message: if node.is_missing() {
                format!("Missing {}", node.kind())
            } else {
                "Syntax error".to_string()
            },
            code: "syntax-error".to_string(),
        });
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_errors(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::parse_json as parse;

    fn first_column<'tree>(
        tree: &'tree tree_sitter::Tree,
        source: &[u8],
    ) -> tree_sitter::Node<'tree> {
        let columns =
            find_value_for_key(tree.root_node(), source, "columns").expect("columns value");
        direct_column_objects(columns)
            .into_iter()
            .next()
            .expect("column object")
    }

    #[test]
    fn collectors_return_when_columns_are_missing() {
        let tree = parse(r#"{"name":"u"}"#);
        let mut out = Vec::new();

        collect_unknown_column_types(&tree, r#"{"name":"u"}"#, &mut out);
        collect_duplicate_column_names(&tree, r#"{"name":"u"}"#, &mut out);
        collect_complex_type_violations(&tree, r#"{"name":"u"}"#, &mut out);

        assert!(out.is_empty());
    }

    #[test]
    fn recursive_column_walks_visit_nested_objects_without_panicking() {
        let src = r#"{"name":"u","columns":[[{"type":"bogus"}]]}"#;
        let tree = parse(src);
        let mut out = Vec::new();

        collect_unknown_column_types(&tree, src, &mut out);
        collect_complex_type_violations(&tree, src, &mut out);

        assert!(
            out.iter().any(|diag| diag.code == "unknown-type"),
            "got: {out:?}"
        );
    }

    #[test]
    fn inspect_column_type_handles_missing_value_empty_value_and_bad_source() {
        let malformed = r#"{"columns":[{"type":}]}"#;
        let tree = parse(malformed);
        let mut out = Vec::new();
        collect_unknown_column_types(&tree, malformed, &mut out);

        let empty = r#"{"columns":[{"type":""}]}"#;
        let tree = parse(empty);
        collect_unknown_column_types(&tree, empty, &mut out);

        let valid = r#"{"columns":[{"type":"bogus"}]}"#;
        let tree = parse(valid);
        let column = first_column(&tree, valid.as_bytes());
        let type_value_start = find_pair_with_key(column, valid.as_bytes(), "type")
            .unwrap()
            .named_child(1)
            .unwrap()
            .start_byte();
        inspect_column_type(column, &valid.as_bytes()[..type_value_start], &mut out);
        let mut bad = valid.as_bytes().to_vec();
        let idx = valid.find("bogus").unwrap();
        bad[idx] = 0xff;
        inspect_column_type(column, &bad, &mut out);
        inspect_column_type(column, valid.as_bytes(), &mut out);

        assert!(out.iter().any(|diag| diag.code == "unknown-type"));
    }

    #[test]
    fn direct_column_objects_returns_empty_for_non_array_columns() {
        let src = r#"{"name":"u","columns":"oops"}"#;
        let tree = parse(src);
        let mut out = Vec::new();

        collect_all(&tree, src, &mut out);

        assert!(out.iter().all(|diag| diag.code != "duplicate-column"));
    }

    #[test]
    fn inspect_column_name_handles_missing_empty_and_bad_source() {
        let missing = r#"{"columns":[{"type":"integer"}]}"#;
        let tree = parse(missing);
        let column = first_column(&tree, missing.as_bytes());
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        inspect_column_name(column, missing.as_bytes(), &mut seen, &mut out);

        let malformed = r#"{"columns":[{"name":}]}"#;
        let tree = parse(malformed);
        let column = first_column(&tree, malformed.as_bytes());
        inspect_column_name(column, malformed.as_bytes(), &mut seen, &mut out);

        let empty = r#"{"columns":[{"name":""}]}"#;
        let tree = parse(empty);
        let column = first_column(&tree, empty.as_bytes());
        inspect_column_name(column, empty.as_bytes(), &mut seen, &mut out);

        let valid = r#"{"columns":[{"name":"id"}]}"#;
        let tree = parse(valid);
        let column = first_column(&tree, valid.as_bytes());
        let name_value_start = find_pair_with_key(column, valid.as_bytes(), "name")
            .unwrap()
            .named_child(1)
            .unwrap()
            .start_byte();
        inspect_column_name(
            column,
            &valid.as_bytes()[..name_value_start],
            &mut seen,
            &mut out,
        );
        let mut bad = valid.as_bytes().to_vec();
        let idx = valid.find("id").unwrap();
        bad[idx] = 0xff;
        inspect_column_name(column, &bad, &mut seen, &mut out);

        assert!(out.is_empty());
    }

    #[test]
    fn complex_type_shape_branches_emit_expected_diagnostics() {
        let src = r#"{"columns":[{"type":{"kind":""}},{"type":{"kind":"enum","name":"status","values":"bad"}},{"type":{"kind":"custom"}}]}"#;
        let tree = parse(src);
        let mut out = Vec::new();

        collect_complex_type_violations(&tree, src, &mut out);

        assert!(
            out.iter().any(|diag| diag.message.contains("non-empty")),
            "got: {out:?}"
        );
        assert!(
            out.iter().any(|diag| diag.message.contains("values` must")),
            "got: {out:?}"
        );
        assert!(
            out.iter().any(|diag| diag.message.contains("custom_type")),
            "got: {out:?}"
        );
    }

    #[test]
    fn complex_type_handles_missing_type_value_and_missing_values_value() {
        let src = r#"{"columns":[{"type":},{"type":{"kind":"enum","name":"status","values":}}]}"#;
        let tree = parse(src);
        let mut out = Vec::new();

        collect_complex_type_violations(&tree, src, &mut out);

        let _ = out;
    }

    #[test]
    fn complex_enum_missing_name_reports_field_specific_diagnostic() {
        let src = r#"{"columns":[{"type":{"kind":"enum","values":["active"]}}]}"#;
        let tree = parse(src);
        let mut out = Vec::new();

        collect_complex_type_violations(&tree, src, &mut out);

        assert!(
            out.iter()
                .any(|diag| diag.message.contains("requires field: name")),
            "got: {out:?}"
        );
    }

    #[test]
    fn collect_syntax_errors_recurses_into_children() {
        let src = r#"{"columns":[{"name":"id",}]}"#;
        let tree = parse(src);
        let mut out = Vec::new();

        collect_syntax_errors(&tree, &mut out);

        assert!(!out.is_empty());
    }
}
