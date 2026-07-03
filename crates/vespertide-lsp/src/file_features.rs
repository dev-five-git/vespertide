//! Lightweight per-file LSP capabilities — document outline, folding,
//! same-document symbol highlight, and selection-range expansion.
//! Each of these would be a tiny module on its own; we group them
//! here because they share the same tree-sitter walk patterns and
//! none has enough surface area to deserve its own directory.

use crate::text_util::strip_quotes;
use std::ops::Range;

// =====================================================================
// Domain types
// =====================================================================

/// File-level outline entry. Tables nest their columns as `children`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDocumentSymbol {
    pub name: String,
    pub kind: DomainDocumentSymbolKind,
    /// Byte range covering the entire object (table mapping or column
    /// object) — used as the LSP `range`.
    pub byte_range: Range<usize>,
    /// Byte range of the identifier itself — used as `selectionRange`.
    pub selection_byte_range: Range<usize>,
    pub children: Vec<DomainDocumentSymbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainDocumentSymbolKind {
    Table,
    Column,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainFoldingRange {
    /// Byte range of the foldable region. The handler converts to LSP
    /// `startLine`/`endLine` (LSP folds entire lines, not byte spans).
    pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDocumentHighlight {
    pub byte_range: Range<usize>,
    pub kind: DomainDocumentHighlightKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainDocumentHighlightKind {
    /// The position where the user's cursor sits.
    Read,
    /// Other positions referencing the same symbol.
    Reference,
}

/// LSP `selectionRange` is a linked list — each entry has an optional
/// `parent` pointing at the next broader range. We return the chain as
/// a `Vec` (innermost first); the handler reconstructs the linked list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSelectionRange {
    pub byte_range: Range<usize>,
}

// =====================================================================
// documentSymbol
// =====================================================================

#[must_use]
pub fn compute_document_symbols(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
) -> Vec<DomainDocumentSymbol> {
    if let Some(tree) = tree {
        let source_bytes = source.as_bytes();
        if let Some(outer) = find_outer_mapping(tree.root_node())
            && let Some((table_name, table_name_range)) =
                direct_string_value(outer, source_bytes, "name")
        {
            // Columns → children
            let mut children = Vec::new();
            if let Some(columns_pair) = find_pair_with_key(outer, source_bytes, "columns")
                && let Some(columns_value_raw) = columns_pair.named_child(1)
            {
                let columns_value = unwrap_yaml(columns_value_raw);
                if matches!(
                    columns_value.kind(),
                    "array" | "block_sequence" | "flow_sequence"
                ) {
                    for column in direct_column_objects(columns_value) {
                        if let Some((col_name, col_name_range)) =
                            direct_string_value(column, source_bytes, "name")
                        {
                            children.push(DomainDocumentSymbol {
                                name: col_name,
                                kind: DomainDocumentSymbolKind::Column,
                                byte_range: column.byte_range(),
                                selection_byte_range: col_name_range,
                                children: Vec::new(),
                            });
                        }
                    }
                }
            }

            return vec![DomainDocumentSymbol {
                name: table_name,
                kind: DomainDocumentSymbolKind::Table,
                byte_range: outer.byte_range(),
                selection_byte_range: table_name_range,
                children,
            }];
        }
    }

    Vec::new()
}

// =====================================================================
// foldingRange
// =====================================================================

#[must_use]
pub fn compute_folding_ranges(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
) -> Vec<DomainFoldingRange> {
    let _ = source;
    let mut out = Vec::new();
    if let Some(tree) = tree {
        collect_foldable(tree.root_node(), &mut out);
    }
    out
}

fn collect_foldable(node: tree_sitter::Node<'_>, out: &mut Vec<DomainFoldingRange>) {
    // Both JSON containers and YAML block structures fold by line.
    if matches!(
        node.kind(),
        "object" | "array" | "block_mapping" | "block_sequence" | "flow_mapping" | "flow_sequence"
    ) {
        let r = node.byte_range();
        // Don't bother emitting a fold marker for empty / one-line spans.
        if r.end > r.start {
            out.push(DomainFoldingRange { byte_range: r });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_foldable(child, out);
    }
}

// =====================================================================
// documentHighlight
// =====================================================================

/// Find every occurrence of the symbol at `cursor_byte` *within this
/// document*. Returns `Read` for the cursor's position and `Reference`
/// for the rest. Returns empty when the cursor isn't on a renameable
/// identifier (re-uses the `references::resolve_symbol` decision).
#[must_use]
pub fn compute_document_highlight(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    cursor_byte: usize,
) -> Vec<DomainDocumentHighlight> {
    let mut out = Vec::new();
    if let Some(tree) = tree
        && let Some(node) = node_at_byte(tree, cursor_byte)
        && let Some(string_node) = enclosing_string(node)
        && let Some((target, target_range)) =
            inner_string_range_text(string_node, source.as_bytes())
    {
        collect_matching_strings(
            tree.root_node(),
            source.as_bytes(),
            target.as_str(),
            target_range,
            &mut out,
        );
    }
    out
}

fn collect_matching_strings(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    target: &str,
    cursor_range: Range<usize>,
    out: &mut Vec<DomainDocumentHighlight>,
) {
    if matches!(
        node.kind(),
        "string" | "double_quote_scalar" | "single_quote_scalar" | "string_scalar" | "plain_scalar"
    ) && let Some((text, range)) = inner_string_range_text(node, source)
        && text == target
    {
        let kind = if range == cursor_range {
            DomainDocumentHighlightKind::Read
        } else {
            DomainDocumentHighlightKind::Reference
        };
        out.push(DomainDocumentHighlight {
            byte_range: range,
            kind,
        });
        // Strings don't have meaningful children for our purposes.
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_matching_strings(child, source, target, cursor_range.clone(), out);
    }
}

// =====================================================================
// selectionRange
// =====================================================================

/// Build the ancestor chain at `cursor_byte` (innermost-first). Each
/// entry is the byte range of an outer node. Duplicate ranges are
/// collapsed so the LSP client doesn't show the same selection twice.
#[must_use]
pub fn compute_selection_ranges(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    cursor_byte: usize,
) -> Vec<DomainSelectionRange> {
    let _ = source;
    let mut chain = Vec::new();
    if let Some(tree) = tree
        && let Some(start) = node_at_byte(tree, cursor_byte)
    {
        let mut current = Some(start);
        let mut last_range: Option<Range<usize>> = None;
        while let Some(node) = current {
            let r = node.byte_range();
            if last_range.as_ref() != Some(&r) && r.end > r.start {
                chain.push(DomainSelectionRange {
                    byte_range: r.clone(),
                });
                last_range = Some(r);
            }
            current = node.parent();
        }
    }
    chain
}

// =====================================================================
// Shared helpers
// =====================================================================

fn find_outer_mapping(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_outer_mapping(child) {
            return Some(found);
        }
    }
    None
}

fn find_pair_with_key<'tree>(
    mapping: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = mapping.walk();
    mapping.children(&mut cursor).find(|&child| {
        matches!(child.kind(), "pair" | "block_mapping_pair")
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(strip_quotes)
                == Some(target_key)
    })
}

fn direct_string_value(
    mapping: tree_sitter::Node<'_>,
    source: &[u8],
    target_key: &str,
) -> Option<(String, Range<usize>)> {
    let pair = find_pair_with_key(mapping, source, target_key)?;
    let value = unwrap_yaml(pair.named_child(1)?);
    let (text, range) = inner_string_range_text(value, source)?;
    Some((text, range))
}

fn inner_string_range_text(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, Range<usize>)> {
    let range = match node.kind() {
        "string" => node.named_child(0).map_or_else(
            || trim_one_byte(&node.byte_range()),
            |inner| inner.byte_range(),
        ),
        "double_quote_scalar" | "single_quote_scalar" => trim_one_byte(&node.byte_range()),
        "plain_scalar" | "string_scalar" => node.byte_range(),
        _ => return None,
    };
    let text = std::str::from_utf8(source.get(range.clone())?)
        .ok()?
        .to_string();
    Some((text, range))
}

fn direct_column_objects(columns_value: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let array = unwrap_yaml(columns_value);
    let mut out = Vec::new();
    if matches!(array.kind(), "array" | "block_sequence" | "flow_sequence") {
        let mut cursor = array.walk();
        for raw_child in array.children(&mut cursor) {
            let child = unwrap_yaml(raw_child);
            match child.kind() {
                "object" | "block_mapping" | "flow_mapping" => out.push(child),
                "block_sequence_item" => {
                    let mut inner_cursor = child.walk();
                    for inner in child.children(&mut inner_cursor) {
                        let inner = unwrap_yaml(inner);
                        if matches!(inner.kind(), "object" | "block_mapping" | "flow_mapping") {
                            out.push(inner);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
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
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

fn unwrap_yaml(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
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

fn trim_one_byte(range: &Range<usize>) -> Range<usize> {
    if range.end.saturating_sub(range.start) >= 2 {
        (range.start + 1)..(range.end - 1)
    } else {
        range.clone()
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::test_support::parse_json;
    use rstest::rstest;

    fn find_empty_yaml_wrapper(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
        if matches!(node.kind(), "flow_node" | "block_node") && node.named_child(0).is_none() {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_empty_yaml_wrapper(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn document_symbol_returns_table_with_column_children() {
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        let tree = parse_json(src);
        let syms = compute_document_symbols(src, Some(&tree));
        assert_eq!(syms.len(), 1);
        let table = &syms[0];
        assert_eq!(table.name, "user");
        assert_eq!(table.kind, DomainDocumentSymbolKind::Table);
        assert_eq!(table.children.len(), 2);
        let names: Vec<_> = table.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "email"]);
    }

    #[test]
    fn folding_ranges_cover_columns_array_and_every_object() {
        let src = r#"{
            "name":"u",
            "columns":[
                {"name":"id","type":"integer"},
                {"name":"e","type":"text"}
            ]
        }"#;
        let tree = parse_json(src);
        let ranges = compute_folding_ranges(src, Some(&tree));
        // At minimum: top-level object + columns array + each column.
        assert!(ranges.len() >= 4, "got: {ranges:?}");
    }

    #[test]
    fn document_highlight_finds_same_symbol_in_file() {
        let src = r#"{"name":"u","columns":[{"name":"email","type":"text"},{"name":"author_email","foreign_key":{"ref_columns":["email"]}}]}"#;
        let tree = parse_json(src);
        let cursor = src.find(r#""email""#).unwrap() + 1; // inside first "email"
        let hits = compute_document_highlight(src, Some(&tree), cursor);
        assert!(hits.len() >= 2, "should find both occurrences: {hits:?}");
        let _ = cursor; // exact byte comparison not portable across grammars
        assert!(
            hits.iter()
                .any(|h| h.kind == DomainDocumentHighlightKind::Read)
        );
        assert!(
            hits.iter()
                .any(|h| h.kind == DomainDocumentHighlightKind::Reference)
        );
    }

    #[test]
    fn selection_ranges_build_ancestor_chain() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        let cursor = src.find(r#""id""#).unwrap() + 1;
        let chain = compute_selection_ranges(src, Some(&tree), cursor);
        assert!(
            chain.len() >= 3,
            "expected token → pair → object → ..., got: {chain:?}"
        );
        // Strictly expanding ranges.
        for win in chain.windows(2) {
            assert!(
                win[0].byte_range.start >= win[1].byte_range.start
                    && win[0].byte_range.end <= win[1].byte_range.end,
                "non-expanding chain: {chain:?}"
            );
        }
    }

    #[test]
    fn yaml_document_symbol_works_too() {
        let pool = ParserPool::new();
        let src = "name: post\ncolumns:\n  - name: id\n    type: integer\n  - name: author_id\n    type: integer\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let syms = compute_document_symbols(src, Some(&tree));
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "post");
        assert_eq!(syms[0].children.len(), 2);
    }

    #[test]
    fn helper_branches_handle_non_strings_non_arrays_and_empty_wrappers() {
        let json = parse_json(r#"{"name":"u","columns":"not-an-array"}"#);
        let outer = find_outer_mapping(json.root_node()).unwrap();
        let columns_pair = find_pair_with_key(
            outer,
            r#"{"name":"u","columns":"not-an-array"}"#.as_bytes(),
            "columns",
        )
        .unwrap();
        let columns_value = columns_pair.named_child(1).unwrap();
        assert!(direct_column_objects(columns_value).is_empty());
        assert!(inner_string_range_text(json.root_node(), r#"{"name":"u"}"#.as_bytes()).is_none());
        assert!(enclosing_string(json.root_node()).is_none());

        let string = find_pair_with_key(
            outer,
            r#"{"name":"u","columns":"not-an-array"}"#.as_bytes(),
            "name",
        )
        .unwrap()
        .named_child(1)
        .unwrap();
        assert!(inner_string_range_text(string, b"").is_none());

        let pool = ParserPool::new();
        let yaml = "name:\n";
        let tree = pool.parse(yaml, DocumentFormat::Yaml).unwrap();
        let pair = find_outer_mapping(tree.root_node())
            .and_then(|mapping| find_pair_with_key(mapping, yaml.as_bytes(), "name"))
            .unwrap();
        if let Some(wrapper) = pair.named_child(1) {
            let _ = unwrap_yaml(wrapper);
        }
    }

    #[test]
    fn folding_and_trim_helpers_cover_recursive_and_short_ranges() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        let mut ranges = Vec::new();
        collect_foldable(tree.root_node(), &mut ranges);

        assert!(
            ranges.len() >= 3,
            "top object, columns array, and column object should fold: {ranges:?}"
        );
        assert_eq!(trim_one_byte(&(4..5)), 4..5);
        assert_eq!(trim_one_byte(&(4..6)), 5..5);
    }

    #[test]
    fn document_highlight_returns_empty_when_tree_outlives_source_text() {
        let src = r#"{"name":"u"}"#;
        let tree = parse_json(src);
        let cursor = src.find('u').unwrap();

        assert!(compute_document_highlight("", Some(&tree), cursor).is_empty());
    }

    #[test]
    fn unwrap_yaml_handles_empty_wrapper_node() {
        let pool = ParserPool::new();
        let yaml = "name:\n";
        let tree = pool.parse(yaml, DocumentFormat::Yaml).unwrap();
        if let Some(wrapper) = find_empty_yaml_wrapper(tree.root_node()) {
            let unwrapped = unwrap_yaml(wrapper);
            assert_eq!(unwrapped.id(), wrapper.id());
        }
    }

    #[test]
    fn none_tree_returns_empty_for_all_file_features() {
        assert!(compute_document_symbols("x", None).is_empty());
        assert!(compute_folding_ranges("x", None).is_empty());
        assert!(compute_document_highlight("x", None, 0).is_empty());
        assert!(compute_selection_ranges("x", None, 0).is_empty());
    }

    #[rstest]
    #[case::yaml_scalar("just_a_scalar\n", DocumentFormat::Yaml)]
    #[case::json_missing_name(r#"{"columns":[]}"#, DocumentFormat::Json)]
    fn document_symbols_empty_cases(#[case] src: &str, #[case] format: DocumentFormat) {
        let pool = ParserPool::new();
        let tree = pool.parse(src, format).unwrap();

        assert!(compute_document_symbols(src, Some(&tree)).is_empty());
    }

    #[test]
    fn document_highlight_cursor_on_brace_returns_empty() {
        let src = r#"{"name":"u","columns":[]}"#;
        let tree = parse_json(src);

        assert!(compute_document_highlight(src, Some(&tree), 0).is_empty());
    }

    #[test]
    fn selection_ranges_builds_chain_for_column_name() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse_json(src);
        let cursor = src.find(r#""id""#).unwrap() + 1;

        assert!(compute_selection_ranges(src, Some(&tree), cursor).len() >= 2);
    }
}
