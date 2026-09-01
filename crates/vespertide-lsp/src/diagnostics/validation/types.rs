/// Simple column type names recognized as string literals. Mirrors
/// `vespertide_core::SimpleColumnType`. Kept here so we can flag unknown
/// strings BEFORE serde fails — serde's error position is unreliable inside
/// untagged enums and tends to point at the wrong byte (often the column's
/// closing brace).
pub(super) const KNOWN_SIMPLE_TYPES: &[&str] = &[
    "small_int",
    "integer",
    "big_int",
    "real",
    "double_precision",
    "text",
    "boolean",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "bytea",
    "uuid",
    "json",
    "inet",
    "cidr",
    "macaddr",
    "xml",
];

pub(super) struct EnumValueDescriptor {
    pub(super) name: String,
    pub(super) byte_range: std::ops::Range<usize>,
    /// Optional explicit integer value (for integer enums).
    pub(super) integer_value: Option<String>,
    pub(super) integer_value_range: std::ops::Range<usize>,
}

pub(super) use crate::tree_util::unwrap_yaml_node;

use crate::tree_util::is_pair;

pub(super) fn collect_enum_value_descriptors(
    array: tree_sitter::Node<'_>,
    source: &[u8],
) -> Vec<EnumValueDescriptor> {
    let mut out = Vec::new();
    let mut cursor = array.walk();
    for raw_child in array.children(&mut cursor) {
        let child = unwrap_yaml_node(raw_child);
        match child.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => {
                if let Some(name) = scalar_string(child, source) {
                    out.push(EnumValueDescriptor {
                        name,
                        byte_range: child.byte_range(),
                        integer_value: None,
                        integer_value_range: 0..0,
                    });
                }
            }
            "object" | "block_mapping" | "flow_mapping" => {
                let name_pair = find_pair_with_key(child, source, "name");
                let value_pair = find_pair_with_key(child, source, "value");
                if let Some(name_pair) = name_pair
                    && let Some(name_value_raw) = name_pair.named_child(1)
                    && let Some(name) = scalar_string(unwrap_yaml_node(name_value_raw), source)
                {
                    let value_node =
                        value_pair.and_then(|pair| pair.named_child(1).map(unwrap_yaml_node));
                    let (integer_value, integer_range) = value_node.map_or((None, 0..0), |node| {
                        (scalar_string(node, source), node.byte_range())
                    });
                    out.push(EnumValueDescriptor {
                        name,
                        byte_range: child.byte_range(),
                        integer_value,
                        integer_value_range: integer_range,
                    });
                }
            }
            // YAML block_sequence_item wraps the actual element.
            "block_sequence_item" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let inner = unwrap_yaml_node(inner);
                    match inner.kind() {
                        "string"
                        | "double_quote_scalar"
                        | "single_quote_scalar"
                        | "string_scalar"
                        | "plain_scalar" => {
                            if let Some(name) = scalar_string(inner, source) {
                                out.push(EnumValueDescriptor {
                                    name,
                                    byte_range: inner.byte_range(),
                                    integer_value: None,
                                    integer_value_range: 0..0,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub(super) fn scalar_text<'a>(pair: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let value_raw = pair.named_child(1)?;
    let value = unwrap_yaml_node(value_raw);
    let text = source
        .get(value.byte_range())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())?;
    Some(strip_quotes_str(text))
}

pub(super) fn scalar_string(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let text = source
        .get(node.byte_range())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())?;
    Some(strip_quotes_str(text).to_string())
}

#[cfg(test)]
pub(super) fn find_value_for_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child)
            && pair_key_text(child, source).is_some_and(|k| k == target_key)
            && let Some(value) = child.named_child(1)
        {
            return Some(value);
        }
        if let Some(found) = find_value_for_key(child, source, target_key) {
            return Some(found);
        }
    }
    None
}

pub(super) fn find_pair_with_key<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object.children(&mut cursor).find(|&child| {
        is_pair(child) && pair_key_text(child, source).is_some_and(|k| k == target_key)
    })
}

pub(super) fn pair_key_text<'a>(pair: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let key = pair.named_child(0)?;
    let text = source
        .get(key.byte_range())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())?;
    Some(strip_quotes_str(text))
}

pub(super) fn strip_quotes_str(s: &str) -> &str {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|w| w.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|w| w.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::DocumentFormat;
    use crate::test_support::parse;

    fn values_node<'tree>(
        tree: &'tree tree_sitter::Tree,
        source: &[u8],
    ) -> tree_sitter::Node<'tree> {
        unwrap_yaml_node(
            find_value_for_key(tree.root_node(), source, "values").expect("values node"),
        )
    }

    fn find_kind<'tree>(
        node: tree_sitter::Node<'tree>,
        kind: &str,
        no_named_child: bool,
    ) -> Option<tree_sitter::Node<'tree>> {
        if node.kind() == kind && (!no_named_child || node.named_child(0).is_none()) {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_kind(child, kind, no_named_child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn unwrap_yaml_node_stops_on_empty_wrapper() {
        let src = "values:\n  -\n";
        let tree = parse(src, DocumentFormat::Yaml);
        if let Some(wrapper) = find_kind(tree.root_node(), "block_node", true) {
            assert_eq!(unwrap_yaml_node(wrapper).id(), wrapper.id());
        }
    }

    #[test]
    fn known_simple_types_include_network_scalars() {
        assert!(KNOWN_SIMPLE_TYPES.contains(&"inet"));
        assert!(KNOWN_SIMPLE_TYPES.contains(&"cidr"));
        assert!(KNOWN_SIMPLE_TYPES.contains(&"macaddr"));
    }

    #[test]
    fn enum_descriptor_skips_objects_missing_name_or_value_fields() {
        let src = r#"{"values":[{"value":1},{"name":"low"},{"name":"high","value":2}]}"#;
        let tree = parse(src, DocumentFormat::Json);
        let descriptors =
            collect_enum_value_descriptors(values_node(&tree, src.as_bytes()), src.as_bytes());

        assert_eq!(
            descriptors
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "high"]
        );
        assert_eq!(descriptors[0].integer_value, None);
        assert_eq!(descriptors[1].integer_value.as_deref(), Some("2"));
    }

    #[test]
    fn enum_descriptor_skips_object_name_when_value_is_missing_or_not_utf8() {
        let malformed = r#"{"values":[{"name":},{"name":"ok"}]}"#;
        let tree = parse(malformed, DocumentFormat::Json);
        let _ = collect_enum_value_descriptors(
            values_node(&tree, malformed.as_bytes()),
            malformed.as_bytes(),
        );

        let valid = r#"{"values":[{"name":"bad"}]}"#;
        let tree = parse(valid, DocumentFormat::Json);
        let mut bytes = valid.as_bytes().to_vec();
        let start = valid.find("bad").unwrap();
        bytes[start] = 0xff;
        let descriptors = collect_enum_value_descriptors(values_node(&tree, &bytes), &bytes);

        assert!(descriptors.is_empty());
    }

    #[test]
    fn enum_descriptor_handles_yaml_block_sequence_items() {
        let src = "values:\n  - active\n  - name: low\n    value: 1\n";
        let tree = parse(src, DocumentFormat::Yaml);
        let descriptors =
            collect_enum_value_descriptors(values_node(&tree, src.as_bytes()), src.as_bytes());

        assert!(
            descriptors.iter().any(|d| d.name == "active"),
            "got: {:?}",
            descriptors
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn json_integer_enum_value_members_capture_name_and_integer_value() {
        let src = r#"{"values":[{"name":"low","value":0},{"name":"low","value":10}]}"#;
        let tree = parse(src, DocumentFormat::Json);
        let descriptors =
            collect_enum_value_descriptors(values_node(&tree, src.as_bytes()), src.as_bytes());

        assert_eq!(
            descriptors
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "low"]
        );
        assert_eq!(
            descriptors
                .iter()
                .map(|d| d.integer_value.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("0"), Some("10")]
        );
    }
}
