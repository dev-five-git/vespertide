//! Scan open documents (and on-disk models) for references to a symbol.

use crate::text_util::strip_quotes;
use std::path::PathBuf;

use tower_lsp_server::ls_types::Uri;

use crate::store::DocumentStore;
use crate::workspace_tables::WorkspaceTables;

use super::{DomainReference, ReferenceSymbol};

pub(super) fn find_all(
    symbol: &ReferenceSymbol,
    current_uri: &Uri,
    current_source: &str,
    current_tree: Option<&tree_sitter::Tree>,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    include_declaration: bool,
) -> Vec<DomainReference> {
    let mut out = Vec::new();

    // Always scan the document the cursor is in (it might contain self-refs).
    if let Some(tree) = current_tree {
        collect_in_document(
            symbol,
            current_uri,
            current_source,
            tree,
            include_declaration,
            &mut out,
        );
    }

    // Snapshot the open URIs once and reuse it for both the open-document
    // scan and the disk-file dedup set below (was queried twice).
    let open_uris = docs.open_uris();

    // Every OTHER open document.
    for uri in open_uris.iter().filter(|uri| *uri != current_uri) {
        docs.with_doc(uri, |text, tree| {
            if let Some(tree) = tree {
                collect_in_document(symbol, uri, text, tree, include_declaration, &mut out);
            }
        });
    }

    // Disk-only models that the editor has not opened.
    if let Some(disk) = disk_tables {
        let open_paths: std::collections::BTreeSet<PathBuf> = open_uris
            .iter()
            .filter_map(crate::position::uri_to_path)
            .collect();
        // One parser pool for the whole disk sweep instead of constructing a
        // fresh tree-sitter parser per file inside `scan_disk_file`.
        let pool = crate::parser::ParserPool::new();
        for name in disk.names() {
            if let Some(path) = disk.model_path(&name) {
                // Already scanned via the open document above.
                if !open_paths.contains(&path) {
                    scan_disk_file(symbol, &path, &pool, include_declaration, &mut out);
                }
            }
        }
    }

    // Deterministic ordering — uri first, then byte range.
    out.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.byte_range.start.cmp(&b.byte_range.start))
    });
    out.dedup();

    out
}

fn scan_disk_file(
    symbol: &ReferenceSymbol,
    path: &std::path::Path,
    pool: &crate::parser::ParserPool,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    if let Ok(text) = std::fs::read_to_string(path) {
        let format = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Some(crate::parser::DocumentFormat::Json),
            Some("yaml" | "yml") => Some(crate::parser::DocumentFormat::Yaml),
            _ => None,
        };
        if let Some(format) = format
            && let Some(tree) = pool.parse(&text, format)
        {
            // Fallback to the synthetic empty `file:///` URI keeps
            // `scan_disk_file` infallible even on the (practically
            // unreachable) path that `path_to_uri` rejects — better to
            // attribute references to an obviously-synthetic URI than to
            // silently drop them.
            let uri = crate::position::path_to_uri(path)
                .unwrap_or_else(|| <Uri as std::str::FromStr>::from_str("file:///").unwrap());
            collect_in_document(symbol, &uri, &text, &tree, include_declaration, out);
        }
    }
}

fn collect_in_document(
    symbol: &ReferenceSymbol,
    uri: &Uri,
    source: &str,
    tree: &tree_sitter::Tree,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    let source_bytes = source.as_bytes();
    let root = tree.root_node();
    walk_for_symbol(symbol, uri, source_bytes, root, include_declaration, out);
}

fn walk_for_symbol(
    symbol: &ReferenceSymbol,
    uri: &Uri,
    source: &[u8],
    node: tree_sitter::Node<'_>,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair") {
            inspect_pair(symbol, uri, source, child, include_declaration, out);
        }
        walk_for_symbol(symbol, uri, source, child, include_declaration, out);
    }
}

fn inspect_pair(
    symbol: &ReferenceSymbol,
    uri: &Uri,
    source: &[u8],
    pair: tree_sitter::Node<'_>,
    include_declaration: bool,
    out: &mut Vec<DomainReference>,
) {
    if let Some(key) = pair.named_child(0)
        && let Some(key_text) = source
            .get(key.byte_range())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        && let Some(value) = pair.named_child(1)
    {
        let key_text = strip_quotes(key_text);
        match (symbol, key_text) {
            (ReferenceSymbol::Table { name }, "ref_table")
                if value_matches(source, value, name) =>
            {
                out.push(DomainReference {
                    uri: uri.clone(),
                    byte_range: scalar_range(value),
                });
            }
            // Emit the top-level declaration only when explicitly asked.
            (ReferenceSymbol::Table { name }, "name")
                if include_declaration
                    && value_matches(source, value, name)
                    && is_top_level(pair) =>
            {
                out.push(DomainReference {
                    uri: uri.clone(),
                    byte_range: scalar_range(value),
                });
            }
            // ref_columns is an array — push every matching element, scoped to
            // the FK whose sibling `ref_table` equals `table`.
            (ReferenceSymbol::Column { table, column }, "ref_columns")
                if sibling_ref_table_matches(source, pair, table) =>
            {
                push_array_matches(value, source, column, uri, out);
            }
            // Column declaration inside its owning table.
            (ReferenceSymbol::Column { table, column }, "name")
                if include_declaration
                    && value_matches(source, value, column)
                    && is_column_pair(pair, source, table) =>
            {
                out.push(DomainReference {
                    uri: uri.clone(),
                    byte_range: scalar_range(value),
                });
            }
            // Column reference inside a table-level CHECK `expr` string. Each
            // bare identifier in the expression that names this column (scoped
            // to the CHECK's owning table) is a reference.
            (ReferenceSymbol::Column { table, column }, "expr")
                if is_check_constraint_pair(source, pair)
                    && check_owning_table_matches(source, pair, table) =>
            {
                push_check_expr_matches(value, source, column, uri, out);
            }
            _ => {}
        }
    }
}

/// True when this `expr` pair sits next to a sibling `type: "check"` pair
/// inside a constraint object.
fn is_check_constraint_pair(source: &[u8], expr_pair: tree_sitter::Node<'_>) -> bool {
    sibling_value(source, expr_pair, "type").is_some_and(|v| v == "check")
}

/// True when the CHECK constraint's owning table (the document's outermost
/// `name`) equals `expected_table`.
fn check_owning_table_matches(
    source: &[u8],
    expr_pair: tree_sitter::Node<'_>,
    expected_table: &str,
) -> bool {
    outer_table_name(source, expr_pair).is_some_and(|name| name == expected_table)
}

/// Scan a mapping node's DIRECT child pairs (non-recursive) for the pair
/// whose key equals `key`, then return its value's scalar text with the
/// `flow_node`/`block_node` wrapper peeled and surrounding quotes stripped.
///
/// Shared by `sibling_value`, `outer_table_name`, `sibling_ref_table_matches`,
/// and `is_column_pair`, which each open-coded this exact walk before. The
/// crate's `tree_util::find_pair_with_key` is the *recursive* variant; this is
/// the *direct-child* one.
fn direct_child_scalar<'a>(
    object: tree_sitter::Node<'_>,
    source: &'a [u8],
    key: &str,
) -> Option<&'a str> {
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if matches!(child.kind(), "pair" | "block_mapping_pair")
            && let Some(key_node) = child.named_child(0)
            && let Some(key_text) = source
                .get(key_node.byte_range())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            && strip_quotes(key_text) == key
        {
            let value = child.named_child(1)?;
            let actual = match value.kind() {
                "flow_node" | "block_node" => value.named_child(0).unwrap_or(value),
                _ => value,
            };
            let text = source
                .get(actual.byte_range())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())?;
            return Some(strip_quotes(text));
        }
    }
    None
}

/// Look up a sibling pair's scalar value within the same constraint object.
fn sibling_value<'a>(
    source: &'a [u8],
    pair: tree_sitter::Node<'_>,
    target_key: &str,
) -> Option<&'a str> {
    let object_raw = pair.parent()?;
    let object = match object_raw.kind() {
        "flow_node" | "block_node" => object_raw.named_child(0)?,
        _ => object_raw,
    };
    direct_child_scalar(object, source, target_key)
}

/// Walk up to the document's outermost mapping and return its `name` value.
fn outer_table_name<'a>(source: &'a [u8], node: tree_sitter::Node<'_>) -> Option<&'a str> {
    let outer = crate::tree_util::outermost_ancestor_mapping(node)?;
    direct_child_scalar(outer, source, "name")
}

/// Lex the CHECK expression in `value` and push a reference for every bare
/// identifier matching `column`, with byte ranges absolute to the document.
fn push_check_expr_matches(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    column: &str,
    uri: &Uri,
    out: &mut Vec<DomainReference>,
) {
    if let Some(inner) = crate::check_expr_range::expr_inner_range(value)
        && let Some(expr_text) = source
            .get(inner.clone())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    {
        for token in vespertide_planner::lex_check_expr(expr_text) {
            if token.kind == vespertide_planner::CheckTokenKind::Column
                && let Some(ident) = expr_text.get(token.span.clone())
                && ident == column
            {
                out.push(DomainReference {
                    uri: uri.clone(),
                    byte_range: (inner.start + token.span.start)..(inner.start + token.span.end),
                });
            }
        }
    }
}

fn sibling_ref_table_matches(
    source: &[u8],
    ref_columns_pair: tree_sitter::Node<'_>,
    table_name: &str,
) -> bool {
    let fk_object = ref_columns_pair.parent().and_then(|raw| match raw.kind() {
        "flow_node" | "block_node" => raw.named_child(0),
        _ => Some(raw),
    });
    fk_object
        .and_then(|obj| direct_child_scalar(obj, source, "ref_table"))
        .is_some_and(|ref_table| ref_table == table_name)
}

fn push_array_matches(
    array_node: tree_sitter::Node<'_>,
    source: &[u8],
    column: &str,
    uri: &Uri,
    out: &mut Vec<DomainReference>,
) {
    // For both `array` (JSON) and any YAML wrapper, walk descendants and
    // check every scalar.
    let mut cursor = array_node.walk();
    for child in array_node.children(&mut cursor) {
        // Skip punctuation, comments, etc. — only scalar values matter.
        if is_scalar_kind(child.kind()) && value_matches(source, child, column) {
            out.push(DomainReference {
                uri: uri.clone(),
                byte_range: scalar_range(child),
            });
        } else {
            // YAML wraps each element in `flow_node`; recurse one level.
            push_array_matches(child, source, column, uri, out);
        }
    }
}

fn is_scalar_kind(kind: &str) -> bool {
    matches!(
        kind,
        "string" | "double_quote_scalar" | "single_quote_scalar" | "string_scalar" | "plain_scalar"
    )
}

fn value_matches(source: &[u8], value: tree_sitter::Node<'_>, expected: &str) -> bool {
    // YAML wraps scalars in flow_node — peel.
    let actual = if matches!(value.kind(), "flow_node" | "block_node") {
        value.named_child(0).unwrap_or(value)
    } else {
        value
    };
    source
        .get(actual.byte_range())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .is_some_and(|text| strip_quotes(text) == expected)
}

fn scalar_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
    let actual = match node.kind() {
        "flow_node" | "block_node" => node.named_child(0).unwrap_or(node),
        _ => node,
    };
    inner_content_range(actual)
}

/// Byte range of the scalar's TEXT CONTENT, with surrounding quotes
/// excluded when present. This is what we want for highlighting and for
/// rename — `"id"` → `a` should leave the quotes intact and replace only
/// the two-byte interior, not blow them away.
fn inner_content_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
    match node.kind() {
        // tree-sitter-json: `string` is `"…"`. Its first named child is
        // `string_content` (absent when the literal is empty).
        "string" => node.named_child(0).map_or_else(
            || crate::tree_util::trim_one_byte_each_side(&node.byte_range()),
            |inner| inner.byte_range(),
        ),
        // tree-sitter-yaml quoted scalars include their delimiters; trim
        // one byte on each side.
        "double_quote_scalar" | "single_quote_scalar" => {
            crate::tree_util::trim_one_byte_each_side(&node.byte_range())
        }
        // Unquoted scalars (YAML plain / string_scalar, or anything else)
        // have no delimiters — the full range is the identifier.
        _ => node.byte_range(),
    }
}

fn is_top_level(pair: tree_sitter::Node<'_>) -> bool {
    crate::tree_util::is_top_level_pair(pair)
}

/// Check that this `name` pair lives directly inside a column object whose
/// owning table is `expected_table`.
fn is_column_pair(name_pair: tree_sitter::Node<'_>, source: &[u8], expected_table: &str) -> bool {
    // The pair's grandparent (mapping) is the column object; we walk above
    // the column object to the outer mapping and check its `name`.
    if let Some(column_object) = name_pair.parent()
        && matches!(
            column_object.kind(),
            "object" | "block_mapping" | "flow_mapping"
        )
    {
        // The column object is not allowed to be the outermost mapping — that's
        // the table itself.
        if let Some(outer) = crate::tree_util::outermost_ancestor_mapping(column_object)
            && outer.id() != column_object.id()
        {
            return direct_child_scalar(outer, source, "name")
                .is_some_and(|name| name == expected_table);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::DocumentFormat;
    use crate::test_support::{parse, parse_json, uri};
    use tempfile::tempdir;

    fn find_pair_with_key<'tree>(
        node: tree_sitter::Node<'tree>,
        source: &[u8],
        key: &str,
    ) -> Option<tree_sitter::Node<'tree>> {
        if matches!(node.kind(), "pair" | "block_mapping_pair")
            && node
                .named_child(0)
                .and_then(|key_node| source.get(key_node.byte_range()))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(strip_quotes)
                == Some(key)
        {
            return Some(node);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_pair_with_key(child, source, key) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn scan_disk_file_accepts_yaml_yml_and_skips_unknown_extensions() {
        let tmp = tempdir().unwrap();
        let yaml = tmp.path().join("user.yaml");
        let yml = tmp.path().join("account.yml");
        let txt = tmp.path().join("ignored.txt");
        std::fs::write(&yaml, "name: user\ncolumns: []\n").unwrap();
        std::fs::write(&yml, "name: account\ncolumns: []\n").unwrap();
        std::fs::write(&txt, "name: user\ncolumns: []\n").unwrap();

        let mut out = Vec::new();
        let pool = crate::parser::ParserPool::new();
        scan_disk_file(
            &ReferenceSymbol::Table {
                name: "user".to_string(),
            },
            &yaml,
            &pool,
            true,
            &mut out,
        );
        scan_disk_file(
            &ReferenceSymbol::Table {
                name: "account".to_string(),
            },
            &yml,
            &pool,
            true,
            &mut out,
        );
        let before_unknown = out.len();
        scan_disk_file(
            &ReferenceSymbol::Table {
                name: "user".to_string(),
            },
            &txt,
            &pool,
            true,
            &mut out,
        );

        assert_eq!(before_unknown, 2);
        assert_eq!(
            out.len(),
            before_unknown,
            "unsupported extensions must be ignored"
        );
    }

    #[test]
    fn find_all_sorts_references_by_uri_then_byte_range() {
        let docs = DocumentStore::new();
        let current_uri = uri("z_post.json");
        let other_uri = uri("a_post.json");
        let src = r#"{"name":"post","columns":[{"name":"user_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let current_tree = parse_json(src);
        docs.open(other_uri.clone(), "json".to_string(), 1, src.to_string());

        let refs = find_all(
            &ReferenceSymbol::Table {
                name: "user".to_string(),
            },
            &current_uri,
            src,
            Some(&current_tree),
            &docs,
            None,
            false,
        );

        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].uri, other_uri);
        assert_eq!(refs[1].uri, current_uri);
    }

    #[test]
    fn helper_false_paths_handle_missing_siblings_and_non_mapping_nodes() {
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","foreign_key":{"ref_columns":["id"]}}],"constraints":[{"name":"c","expr":"id > 0"}]}"#;
        let tree = parse_json(src);
        let expr_pair =
            find_pair_with_key(tree.root_node(), src.as_bytes(), "expr").expect("expr pair");
        assert_eq!(sibling_value(src.as_bytes(), expr_pair, "type"), None);

        let ref_columns_pair = find_pair_with_key(tree.root_node(), src.as_bytes(), "ref_columns")
            .expect("ref_columns pair");
        assert!(!sibling_ref_table_matches(
            src.as_bytes(),
            ref_columns_pair,
            "user"
        ));

        let array_src = "[]";
        let array_tree = parse(array_src, DocumentFormat::Json);
        assert_eq!(
            outer_table_name(array_src.as_bytes(), array_tree.root_node()),
            None
        );

        assert!(!value_matches(
            array_src.as_bytes(),
            array_tree.root_node(),
            "user"
        ));
    }

    #[test]
    fn range_and_top_level_helpers_cover_defensive_branches() {
        assert_eq!(crate::tree_util::trim_one_byte_each_side(&(4..5)), 4..5);

        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        let top_name = find_pair_with_key(tree.root_node(), src.as_bytes(), "name")
            .expect("top-level name pair");
        assert!(is_top_level(top_name));

        let column_src = r#"{"name":"user","columns":[{"type":"integer","name":"id"}]}"#;
        let column_tree = parse_json(column_src);
        let column_name =
            find_pair_with_key(column_tree.root_node(), column_src.as_bytes(), "name")
                .expect("outer name pair");
        let mut cursor = column_tree.root_node().walk();
        let nested_name = column_tree
            .root_node()
            .children(&mut cursor)
            .find_map(|child| find_pair_with_key(child, column_src.as_bytes(), "columns"))
            .and_then(|columns| find_pair_with_key(columns, column_src.as_bytes(), "name"))
            .expect("nested column name pair");

        assert!(!is_top_level(nested_name));
        assert!(!is_top_level(
            column_name
                .named_child(0)
                .expect("key node has pair parent")
        ));
        assert!(!is_top_level(column_tree.root_node()));
        assert!(!is_column_pair(
            nested_name,
            column_src.as_bytes(),
            "other_table"
        ));
    }

    /// L197 — `outer_table_name` falls through to `None` when an outer
    /// mapping is found but no `"name"` key sits inside it. Construct a
    /// document with `{"columns":[…]}` (root mapping lacks `name`) and call
    /// the helper from a node *inside* the mapping so the parent-walk lands
    /// on the root, but the for-loop scan finds no `name` pair.
    #[test]
    fn outer_table_name_returns_none_when_outer_mapping_lacks_name_key() {
        let src = r#"{"columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        // Find the inner `columns` pair so the parent-walk inside
        // `outer_table_name` reaches the root mapping which lacks "name".
        let columns_pair =
            find_pair_with_key(tree.root_node(), src.as_bytes(), "columns").expect("columns pair");
        assert_eq!(outer_table_name(src.as_bytes(), columns_pair), None);
    }

    /// L345 — `is_column_pair` falls through to `false` when the outer
    /// mapping is reachable AND distinct from the column object, but the
    /// outer mapping has no `"name"` pair at all. Use a root object that
    /// declares only `"columns"`.
    #[test]
    fn is_column_pair_returns_false_when_outer_lacks_name_key() {
        let src = r#"{"columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        // Locate the column-object's `name` pair (nested inside `columns`).
        let columns_pair =
            find_pair_with_key(tree.root_node(), src.as_bytes(), "columns").expect("columns pair");
        let nested_name =
            find_pair_with_key(columns_pair, src.as_bytes(), "name").expect("nested name pair");
        // No "name" sibling in the root mapping → falls through to the
        // L345 sentinel `false` return.
        assert!(!is_column_pair(nested_name, src.as_bytes(), "user"));
    }

    /// The table's own `name` pair sits directly in the outermost mapping, so
    /// `outer.id() != column_object.id()` is false and the function reaches its
    /// trailing `false` — the only path that skips both inner `if let`s. Without
    /// this, a table-level `name` could be mistaken for a column declaration.
    #[test]
    fn is_column_pair_returns_false_for_the_tables_own_name_pair() {
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        let root_name =
            find_pair_with_key(tree.root_node(), src.as_bytes(), "name").expect("root name pair");

        assert!(
            !is_column_pair(root_name, src.as_bytes(), "user"),
            "the table's own `name` is not a column declaration"
        );
    }

    #[test]
    fn collect_in_document_keeps_table_and_check_references_distinct() {
        let src = r#"{"name":"user","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"c","expr":"age > 0"}]}"#;
        let tree = parse_json(src);
        let mut out = Vec::new();

        collect_in_document(
            &ReferenceSymbol::Column {
                table: "user".to_string(),
                column: "age".to_string(),
            },
            &uri("user.json"),
            src,
            &tree,
            true,
            &mut out,
        );

        assert!(
            out.iter()
                .any(|reference| &src[reference.byte_range.clone()] == "age")
        );
        assert!(
            out.len() >= 2,
            "declaration and CHECK expression should both be found: {out:?}"
        );
    }

    /// The test above only ever lexes a CHECK expression whose single column
    /// identifier matches the symbol, so `push_check_expr_matches` never took
    /// the `ident == column` false path. A predicate naming two columns forces
    /// both: `age` is pushed, `score` is skipped.
    #[test]
    fn check_expr_matches_skip_identifiers_for_other_columns() {
        let src = r#"{"name":"user","columns":[{"name":"age","type":"integer"},{"name":"score","type":"integer"}],"constraints":[{"type":"check","name":"c","expr":"age > 0 AND score > age"}]}"#;
        let tree = parse_json(src);
        let mut out = Vec::new();

        collect_in_document(
            &ReferenceSymbol::Column {
                table: "user".to_string(),
                column: "age".to_string(),
            },
            &uri("user.json"),
            src,
            &tree,
            false,
            &mut out,
        );

        let hits: Vec<&str> = out
            .iter()
            .map(|reference| &src[reference.byte_range.clone()])
            .collect();
        assert!(
            hits.iter().all(|hit| *hit == "age"),
            "only `age` identifiers may be reported, got: {hits:?}"
        );
        assert_eq!(
            hits.len(),
            2,
            "both `age` occurrences in the CHECK predicate are references: {hits:?}"
        );
    }
}
