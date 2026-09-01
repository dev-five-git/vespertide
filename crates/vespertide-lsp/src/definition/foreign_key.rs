//! Foreign-key go-to-definition for `ref_table` and `ref_columns` values.
//!
//! Behaviour matrix:
//! | Cursor sits in                          | Resolves to                                |
//! |-----------------------------------------|--------------------------------------------|
//! | `ref_table: "<X>"`                       | top-level `name` of table X                |
//! | `ref_columns: ["<Y>", ...]` (any entry)  | column named Y inside the FK's target table|
//!
//! The target file may be open in the editor or sit only on disk — both
//! resolve. For disk-only targets we point at byte `0..0` because parsing
//! the file lives outside this module; the client opens it and the user
//! sees the document.

use std::ops::Range;

use crate::store::DocumentStore;
use crate::text_util::strip_quotes;
use crate::tree_util::{
    direct_child_value, enclosing_pair_with_key, enclosing_string, is_pair, skip_yaml_wrappers,
};
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

use super::DomainLocation;

/// Strategy that locates the precise byte range inside the target file.
enum TargetLookup<'a> {
    /// Top-level `name` value of the table.
    TableName,
    /// A specific column inside the `columns` array.
    Column(&'a str),
}

pub(super) fn try_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    if let Some(loc) = try_ref_table_definition(node, source, index, docs, disk_tables) {
        return Some(loc);
    }
    try_ref_columns_definition(node, source, index, docs, disk_tables)
}

fn try_ref_table_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    let pair = enclosing_pair_with_key(node, source, "ref_table")?;
    let value = pair.named_child(1)?;
    let target_name = strip_quotes(&source[value.byte_range()]).to_string();

    resolve_target(
        &target_name,
        &TargetLookup::TableName,
        index,
        docs,
        disk_tables,
    )
}

fn try_ref_columns_definition(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    // Locate the enclosing `ref_columns` pair. Skipping ancestry-by-kind
    // would be fragile across YAML (where inline `[x]` parses as a
    // `flow_node`, not a `flow_sequence`) — finding the pair directly is
    // grammar-agnostic.
    let string_node = enclosing_string(node)?;
    let ref_columns_pair = enclosing_pair_with_key(string_node, source, "ref_columns")?;
    let ref_columns_value = ref_columns_pair.named_child(1)?;
    // Ensure the cursor sits in the VALUE side of the pair, not the key.
    if !ref_columns_value
        .byte_range()
        .contains(&string_node.start_byte())
    {
        return None;
    }

    let column_name = strip_quotes(&source[string_node.byte_range()]).to_string();
    let fk_object_raw = ref_columns_pair.parent()?;
    let fk_object = skip_yaml_wrappers(fk_object_raw)?;
    let ref_table_value = direct_child_value(fk_object, source, "ref_table")?;
    let target_table = strip_quotes(ref_table_value).to_string();

    resolve_target(
        &target_table,
        &TargetLookup::Column(&column_name),
        index,
        docs,
        disk_tables,
    )
}

fn resolve_target(
    table_name: &str,
    lookup: &TargetLookup<'_>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainLocation> {
    // Prefer the open document (may carry unsaved edits with accurate ranges).
    if let Some(loc) = index.lookup(table_name) {
        let byte_range = docs
            .with_doc(&loc.uri, |text, tree| {
                let tree = tree?;
                match lookup {
                    TargetLookup::TableName => find_top_level_name_range(tree, text),
                    TargetLookup::Column(column) => find_column_name_range(tree, text, column),
                }
            })
            .flatten()
            .unwrap_or(0..0);
        return Some(DomainLocation {
            uri: loc.uri,
            byte_range,
        });
    }

    // Fall back to the on-disk model so closed files still navigate.
    let path = disk_tables?.model_path(table_name)?;
    let uri = crate::position::path_to_uri(&path)?;
    Some(DomainLocation {
        uri,
        byte_range: 0..0,
    })
}

fn find_column_name_range(
    tree: &tree_sitter::Tree,
    text: &str,
    column_name: &str,
) -> Option<Range<usize>> {
    let columns_value = find_columns_array(tree.root_node(), text.as_bytes())?;
    walk_for_named_column(columns_value, text.as_bytes(), column_name)
}

fn find_columns_array<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_is(child, source, "columns") {
            return child.named_child(1);
        }
        if let Some(found) = find_columns_array(child, source) {
            return Some(found);
        }
    }
    None
}

fn walk_for_named_column(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    column_name: &str,
) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_mapping(child)
            && let Some(name_pair) = direct_named_child_pair(child, source, "name")
            && let Some(name_value) = name_pair.named_child(1)
            && source
                .get(name_value.byte_range())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .is_some_and(|raw| strip_quotes(raw) == column_name)
        {
            // Highlight the column's `name` value range — that's where
            // the user expects the cursor to land.
            return Some(name_value.byte_range());
        }
        if let Some(range) = walk_for_named_column(child, source, column_name) {
            return Some(range);
        }
    }
    None
}

fn direct_named_child_pair<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object
        .children(&mut cursor)
        .find(|&child| is_pair(child) && key_is(child, source, target_key))
}

fn find_top_level_name_range(tree: &tree_sitter::Tree, text: &str) -> Option<Range<usize>> {
    let root = tree.root_node();
    let mapping = first_mapping(root)?;
    find_direct_name_range(mapping, text.as_bytes())
        .or_else(|| walk_for_name(root, text.as_bytes()))
}

fn first_mapping(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if is_mapping(node) {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_mapping(child) {
            return Some(found);
        }
    }
    None
}

fn find_direct_name_range(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_is(child, source, "name") {
            let value = child.named_child(1)?;
            return Some(value.byte_range());
        }
    }
    None
}

fn walk_for_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child) && key_is(child, source, "name") {
            let value = child.named_child(1)?;
            return Some(value.byte_range());
        }
        if let Some(found) = walk_for_name(child, source) {
            return Some(found);
        }
    }
    None
}

fn key_is(node: tree_sitter::Node<'_>, source: &[u8], expected: &str) -> bool {
    is_pair(node)
        && node
            .named_child(0)
            .and_then(|key| source.get(key.byte_range()))
            .and_then(|text| std::str::from_utf8(text).ok())
            .is_some_and(|key_str| strip_quotes(key_str) == expected)
}

fn is_mapping(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "object" | "block_mapping")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::DocumentFormat;
    use crate::test_support::parse;
    use std::str::FromStr;
    use tower_lsp_server::ls_types::Uri;

    fn node_at<'tree>(
        tree: &'tree tree_sitter::Tree,
        source: &str,
        needle: &str,
        advance: usize,
    ) -> tree_sitter::Node<'tree> {
        let byte = source.find(needle).unwrap() + advance;
        tree.root_node()
            .descendant_for_byte_range(byte, byte)
            .unwrap()
    }

    fn first_node<'tree>(
        node: tree_sitter::Node<'tree>,
        predicate: impl Fn(tree_sitter::Node<'tree>) -> bool + Copy,
    ) -> Option<tree_sitter::Node<'tree>> {
        if predicate(node) {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = first_node(child, predicate) {
                return Some(found);
            }
        }
        None
    }

    fn uri(text: &str) -> Uri {
        Uri::from_str(text).unwrap()
    }

    #[test]
    fn ref_columns_key_side_and_missing_ref_table_return_none() {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let key_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let key_tree = parse(key_src, DocumentFormat::Json);
        let key_node = node_at(&key_tree, key_src, r#""ref_columns""#, 2);
        assert!(try_definition(key_node, key_src, &idx, &docs, None).is_none());
        let missing_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_columns":["id"]}}]}"#;
        let missing_tree = parse(missing_src, DocumentFormat::Json);
        let missing_node = node_at(&missing_tree, missing_src, r#"["id"]"#, 3);
        assert!(try_definition(missing_node, missing_src, &idx, &docs, None).is_none());
    }

    #[test]
    fn yaml_ref_columns_resolves_open_target_column() {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let user_uri = uri("file:///workspace/user.yaml");
        let user_src = "name: user\ncolumns:\n  - name: id\n    type: integer\n";
        let user_tree = parse(user_src, DocumentFormat::Yaml);
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "yaml".to_string(),
            1,
            user_src.to_string(),
        );
        let post_src = "name: post\ncolumns:\n  - name: author_id\n    type: integer\n    foreign_key:\n      ref_table: user\n      ref_columns:\n        - id\n";
        let post_tree = parse(post_src, DocumentFormat::Yaml);
        let node = node_at(&post_tree, post_src, "- id", 3);
        let loc = try_definition(node, post_src, &idx, &docs, None)
            .expect("YAML ref_columns should resolve");
        assert_eq!(loc.uri, user_uri);
        assert_eq!(&user_src[loc.byte_range], "id");
    }

    #[test]
    fn private_range_helpers_cover_absent_and_nested_names() {
        let array_src = "[]";
        let array_tree = parse(array_src, DocumentFormat::Json);
        assert!(find_top_level_name_range(&array_tree, array_src).is_none());
        let no_name_src = r#"{"columns":[]}"#;
        let no_name_tree = parse(no_name_src, DocumentFormat::Json);
        assert!(find_top_level_name_range(&no_name_tree, no_name_src).is_none());
        let nested_src = r#"{"wrapper":{"name":"inner"}}"#;
        let nested_tree = parse(nested_src, DocumentFormat::Json);
        let range = find_top_level_name_range(&nested_tree, nested_src)
            .expect("walk_for_name fallback should find nested name");
        assert_eq!(&nested_src[range], r#""inner""#);
        assert!(enclosing_string(nested_tree.root_node()).is_none());
        assert!(!key_is(
            nested_tree.root_node(),
            nested_src.as_bytes(),
            "name"
        ));
    }

    #[test]
    fn skip_yaml_wrappers_climbs_to_parent_mapping() {
        let src = "name: p\ncolumns:\n  - {name: a, type: integer}\n";
        let tree = parse(src, DocumentFormat::Yaml);
        let wrapper = first_node(tree.root_node(), |node| {
            matches!(node.kind(), "flow_node" | "block_node")
        })
        .expect("YAML wrapper node");

        let skipped = skip_yaml_wrappers(wrapper).expect("wrapper parent");

        assert_ne!(skipped.id(), wrapper.id());
    }

    #[test]
    fn ref_table_in_yaml_with_no_target_returns_none() {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = "name: post\ncolumns:\n  - name: a\n    type: integer\n    foreign_key:\n      ref_table: nonexistent\n      ref_columns: [id]\n";
        let tree = parse(src, DocumentFormat::Yaml);
        let node = node_at(&tree, src, "ref_table: nonexistent", 12);

        assert!(try_definition(node, src, &idx, &docs, None).is_none());
    }

    #[test]
    fn cursor_outside_foreign_key_returns_none() {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse(src, DocumentFormat::Json);
        let node = node_at(&tree, src, "{", 0);

        assert!(try_definition(node, src, &idx, &docs, None).is_none());
    }
}
