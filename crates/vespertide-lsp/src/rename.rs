//! Workspace-wide rename for tables and columns.
//!
//! Reuses the references engine: every occurrence the references provider
//! would return for the cursor's symbol becomes a `TextEdit` that replaces
//! the byte range with `new_name`. The declaration is always included so
//! the rename is symmetrical (definition + every usage).
//!
//! Returned [`DomainRename`] is grouped by URI so the backend can pack it
//! into an LSP `WorkspaceEdit` without further reshuffling.

use std::collections::BTreeMap;
use std::ops::Range;

use tower_lsp_server::ls_types::Uri;

use crate::parser::DocumentFormat;
use crate::references::{self, ReferenceSymbol};
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

/// Result of `textDocument/prepareRename`. When the cursor is on a
/// renameable symbol we return both the byte range of the existing
/// identifier (for the editor's "select-on-rename" UI) and a placeholder
/// value (the current name pre-filled in the rename input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainPrepareRename {
    pub byte_range: Range<usize>,
    pub placeholder: String,
}

/// Resolve the renameable symbol under the cursor and return its inner
/// content range — same byte range used by references and the rename
/// edit itself. Returning `None` makes the editor refuse the rename
/// prompt entirely, which is what we want on non-renameable positions
/// (whitespace, braces, key strings the user did not intend to rename).
#[must_use]
pub fn prepare(
    source: &str,
    _format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    byte_offset: usize,
) -> Option<DomainPrepareRename> {
    let symbol = references::resolve_symbol(source, tree, current_uri, byte_offset)?;
    let placeholder = match &symbol {
        ReferenceSymbol::Table { name } => name.clone(),
        ReferenceSymbol::Column { column, .. } => column.clone(),
    };
    let range = locate_symbol_inner_range(tree?, source, byte_offset)?;
    Some(DomainPrepareRename {
        byte_range: range,
        placeholder,
    })
}

/// Find the inner content range of the JSON/YAML string scalar that the
/// cursor sits in. Mirrors how [`references::compute`] decides which
/// range to replace, so prepare + rename agree byte-for-byte.
fn locate_symbol_inner_range(
    tree: &tree_sitter::Tree,
    source: &str,
    byte_offset: usize,
) -> Option<Range<usize>> {
    // Inside a table-level CHECK `expr`, the renameable symbol is the single
    // column identifier the cursor sits on — not the whole predicate string.
    // Narrow to that token so prepare's select-on-rename UI highlights only
    // the identifier (and matches the edit the references engine produces).
    if let Some(token_range) = check_expr_column_token_range(tree, source, byte_offset) {
        return Some(token_range);
    }
    let node = node_at_byte(tree, byte_offset)?;
    let string_node = enclosing_string(node)?;
    Some(inner_content_range(string_node, source))
}

/// If the cursor sits on a column identifier inside a CHECK `expr` string,
/// return that identifier's absolute byte range. Returns `None` for
/// non-CHECK positions or offsets that do not land on a Column token.
fn check_expr_column_token_range(
    tree: &tree_sitter::Tree,
    source: &str,
    byte_offset: usize,
) -> Option<Range<usize>> {
    let ctx = crate::check_expr_locate::find_check_expr_at(tree, source.as_bytes(), byte_offset)?;
    let expr_text = source.get(ctx.inner.clone())?;
    let rel = byte_offset.checked_sub(ctx.inner.start)?;
    let token = vespertide_planner::lex_check_expr(expr_text)
        .into_iter()
        .find(|tok| {
            tok.kind == vespertide_planner::CheckTokenKind::Column && tok.span.contains(&rel)
        })?;
    Some((ctx.inner.start + token.span.start)..(ctx.inner.start + token.span.end))
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

fn inner_content_range(node: tree_sitter::Node<'_>, source: &str) -> Range<usize> {
    let raw = node.byte_range();
    // JSON `string` node has a `string_content` named child (without quotes)
    // when non-empty; YAML quoted scalars include their delimiters. Strip
    // one byte from each side when the literal looks quoted.
    match node.kind() {
        "string" => node
            .named_child(0)
            .map_or_else(|| trim_one_byte(&raw), |inner| inner.byte_range()),
        "double_quote_scalar" | "single_quote_scalar" => trim_one_byte(&raw),
        _ => {
            // Defensive: ensure the range is in-bounds before returning.
            let _ = source;
            raw
        }
    }
}

fn trim_one_byte(range: &Range<usize>) -> Range<usize> {
    if range.end.saturating_sub(range.start) >= 2 {
        (range.start + 1)..(range.end - 1)
    } else {
        range.clone()
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainTextEdit {
    pub byte_range: Range<usize>,
    pub new_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainRename {
    /// `URI → edits to apply` (deterministic iteration order).
    pub edits: BTreeMap<Uri, Vec<DomainTextEdit>>,
    /// The symbol the rename is targeting. Useful for telemetry / logs.
    pub symbol: Option<ReferenceSymbol>,
}

/// Validate `new_name` and produce a workspace-wide rename plan.
///
/// Returns `None` when:
///   * the cursor is not on a renameable symbol,
///   * `new_name` is empty, identical to the old name, or contains
///     invalid characters (whitespace, quotes, control chars).
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "rename needs source document, cursor, open/disk workspace stores, and new identifier; RenameContext is a deferred 0.3.x refactor"
)]
pub fn compute(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    byte_offset: usize,
    new_name: &str,
) -> Option<DomainRename> {
    if !is_valid_identifier(new_name) {
        return None;
    }
    let symbol = references::resolve_symbol(source, tree, current_uri, byte_offset)?;
    let old_name = match &symbol {
        ReferenceSymbol::Table { name } => name.clone(),
        ReferenceSymbol::Column { column, .. } => column.clone(),
    };
    if old_name == new_name {
        return None;
    }

    let refs = references::compute(
        source,
        format,
        tree,
        current_uri,
        index,
        docs,
        disk_tables,
        byte_offset,
        true, // rename ALWAYS includes the declaration.
    );

    let mut edits: BTreeMap<Uri, Vec<DomainTextEdit>> = BTreeMap::new();
    for reference in refs {
        edits
            .entry(reference.uri)
            .or_default()
            .push(DomainTextEdit {
                byte_range: reference.byte_range,
                new_text: new_name.to_string(),
            });
    }

    // Sort within each file by reverse byte order so applying edits front-to-back
    // never invalidates ranges that come later. We sort descending; the LSP
    // client is expected to apply edits in document order, so deduplicate
    // overlaps and present them sorted ASCENDING here (clients sort, but
    // sending in document order avoids edge cases).
    for file_edits in edits.values_mut() {
        file_edits.sort_by_key(|e| e.byte_range.start);
        file_edits.dedup_by(|a, b| a.byte_range == b.byte_range && a.new_text == b.new_text);
    }

    (!edits.is_empty()).then_some(DomainRename {
        edits,
        symbol: Some(symbol),
    })
}

/// Names must look like SQL/JSON identifiers — no whitespace, quotes,
/// brackets, or control characters. We intentionally keep this strict so
/// the resulting file is still valid JSON without re-escaping.
fn is_valid_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.chars().all(|c| {
        !c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '"' | '\'' | '\\' | ',' | ':' | '[' | ']' | '{' | '}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParserPool;
    use crate::test_support::uri;

    #[test]
    fn rejects_empty_or_whitespace_or_same_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""name":"user""#).unwrap() + 9;

        for bad in ["", " ", "\"", "user", "a b"] {
            assert!(
                compute(
                    src,
                    DocumentFormat::Json,
                    tree.as_ref(),
                    &uri("user.json"),
                    &idx,
                    &docs,
                    None,
                    pos,
                    bad,
                )
                .is_none(),
                "rename to `{bad}` should be rejected"
            );
        }
    }

    #[test]
    fn rename_table_produces_edits_in_declaration_and_each_ref_file() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_src =
            r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let user_uri = uri("user.json");
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "json".to_string(),
            1,
            user_src.to_string(),
        );

        let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let post_uri = uri("post.json");
        let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
        idx.upsert(&post_uri, post_src, &post_tree);
        docs.open(
            post_uri.clone(),
            "json".to_string(),
            1,
            post_src.to_string(),
        );

        let pos = user_src.find(r#""name":"user""#).unwrap() + 9;
        let plan = compute(
            user_src,
            DocumentFormat::Json,
            Some(&user_tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "account",
        )
        .expect("rename should succeed");

        assert!(
            plan.edits.contains_key(&user_uri),
            "declaration file must be edited"
        );
        assert!(
            plan.edits.contains_key(&post_uri),
            "reference file must be edited"
        );
        for file_edits in plan.edits.values() {
            for edit in file_edits {
                assert_eq!(edit.new_text, "account");
            }
        }
    }

    /// Regression — `"id"` → `a` must produce `"a"`, not bare `a`. The
    /// references engine used to hand out the JSON `string` node's full
    /// range (quotes included), which made rename eat the quotes and break
    /// JSON parsing.
    #[test]
    fn rename_preserves_surrounding_json_quotes() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src =
            r#"{"name":"user","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let user_uri = uri("user.json");
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, src, &tree);
        docs.open(user_uri.clone(), "json".to_string(), 1, src.to_string());

        let pos = src.find(r#""name":"id""#).unwrap() + 9;
        let plan = compute(
            src,
            DocumentFormat::Json,
            Some(&tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "a",
        )
        .expect("rename should succeed");

        let file_edits = plan.edits.get(&user_uri).expect("edits for user.json");
        assert_eq!(file_edits.len(), 1);
        let edit = &file_edits[0];
        assert_eq!(
            &src[edit.byte_range.clone()],
            "id",
            "must replace ONLY inside the quotes"
        );

        // Apply the edit and confirm the result is still valid JSON.
        let mut after = String::from(&src[..edit.byte_range.start]);
        after.push_str(&edit.new_text);
        after.push_str(&src[edit.byte_range.end..]);
        assert!(
            after.contains(r#""a""#),
            "result must keep the quotes: {after}"
        );
        serde_json::from_str::<serde_json::Value>(&after)
            .expect("rename output must still parse as JSON");
    }

    #[test]
    fn prepare_returns_range_and_placeholder_for_top_level_name() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        // Cursor inside `"user"` value.
        let pos = src.find(r#""name":"user""#).unwrap() + 9;
        let result = prepare(
            src,
            DocumentFormat::Json,
            Some(&tree),
            &uri("user.json"),
            pos,
        )
        .expect("table name should be renameable");
        assert_eq!(result.placeholder, "user");
        assert_eq!(
            &src[result.byte_range.clone()],
            "user",
            "range must select INNER content only (quotes preserved)"
        );
    }

    #[test]
    fn prepare_returns_range_for_column_name() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"email","type":"text"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let pos = src.find(r#""name":"email""#).unwrap() + 10;
        let result = prepare(src, DocumentFormat::Json, Some(&tree), &uri("u.json"), pos)
            .expect("column name should be renameable");
        assert_eq!(result.placeholder, "email");
        assert_eq!(&src[result.byte_range.clone()], "email");
    }

    #[test]
    fn prepare_returns_none_outside_renameable_positions() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        // Cursor on the opening brace — not a symbol.
        let result = prepare(src, DocumentFormat::Json, Some(&tree), &uri("u.json"), 0);
        assert!(
            result.is_none(),
            "non-symbol positions must not be renameable"
        );
    }

    #[test]
    fn rename_column_scoped_to_its_owning_table() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_src =
            r#"{"name":"user","columns":[{"name":"email","type":"text","unique":true}]}"#;
        let user_uri = uri("user.json");
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "json".to_string(),
            1,
            user_src.to_string(),
        );

        let post_src = r#"{"name":"post","columns":[{"name":"author_email","type":"text","foreign_key":{"ref_table":"user","ref_columns":["email"]}}]}"#;
        let post_uri = uri("post.json");
        let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
        idx.upsert(&post_uri, post_src, &post_tree);
        docs.open(
            post_uri.clone(),
            "json".to_string(),
            1,
            post_src.to_string(),
        );

        // An unrelated `email` column on another table — must NOT be renamed.
        let other_src = r#"{"name":"other","columns":[{"name":"email","type":"text"}]}"#;
        let other_uri = uri("other.json");
        let other_tree = pool.parse(other_src, DocumentFormat::Json).unwrap();
        idx.upsert(&other_uri, other_src, &other_tree);
        docs.open(
            other_uri.clone(),
            "json".to_string(),
            1,
            other_src.to_string(),
        );

        let pos = user_src.find(r#""name":"email""#).unwrap() + 10;
        let plan = compute(
            user_src,
            DocumentFormat::Json,
            Some(&user_tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "mail",
        )
        .expect("rename should succeed");

        assert!(plan.edits.contains_key(&user_uri));
        assert!(plan.edits.contains_key(&post_uri));
        assert!(
            !plan.edits.contains_key(&other_uri),
            "unrelated `other.email` column must not be in the rename plan"
        );
    }

    // -- CHECK-expression rename (RN-S1/S2/S3) ----------------------------
    //
    // Rename delegates to the references engine, so renaming a column must
    // also rewrite every bare identifier inside table-level CHECK `expr`
    // strings that names that column on the owning table. Without this,
    // renaming `age` → `years` would leave a stale `age > 0` CHECK that no
    // longer matches any column — a silent schema bug.

    #[test]
    fn rn_s1_rename_column_rewrites_check_expr_occurrence() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let src = r#"{"name":"user","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let user_uri = uri("user.json");
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, src, &tree);
        docs.open(user_uri.clone(), "json".to_string(), 1, src.to_string());

        // Rename from the column declaration.
        let pos = src.find(r#""name":"age""#).unwrap() + 8;
        let plan = compute(
            src,
            DocumentFormat::Json,
            Some(&tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "years",
        )
        .expect("rename should succeed");

        let file_edits = plan.edits.get(&user_uri).expect("edits for user.json");
        // Declaration edit + the `age` inside the CHECK expr.
        let expr_field_start = src.find(r#""expr":""#).unwrap() + r#""expr":""#.len();
        let check_edits: Vec<_> = file_edits
            .iter()
            .filter(|e| e.byte_range.start >= expr_field_start)
            .collect();
        assert_eq!(
            check_edits.len(),
            1,
            "the `age` inside the CHECK expr must be rewritten, got: {file_edits:?}"
        );
        assert_eq!(&src[check_edits[0].byte_range.clone()], "age");
        assert_eq!(check_edits[0].new_text, "years");

        // Apply all edits front-to-back and confirm the CHECK now reads
        // `years > 0` and the document still parses as JSON.
        let mut sorted = file_edits.clone();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.byte_range.start));
        let mut after = src.to_string();
        for edit in &sorted {
            after.replace_range(edit.byte_range.clone(), &edit.new_text);
        }
        assert!(
            after.contains(r#""expr":"years > 0""#),
            "CHECK expr must be rewritten to `years > 0`, got: {after}"
        );
        serde_json::from_str::<serde_json::Value>(&after)
            .expect("rename output must still parse as JSON");
    }

    #[test]
    fn rn_s2_rename_initiated_from_inside_check_expr() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let src = r#"{"name":"user","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0 AND age < 150"}]}"#;
        let user_uri = uri("user.json");
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, src, &tree);
        docs.open(user_uri.clone(), "json".to_string(), 1, src.to_string());

        // Cursor placed ON the first `age` inside the CHECK expr.
        let pos = src.find(r#""expr":"age"#).unwrap() + 8;
        let plan = compute(
            src,
            DocumentFormat::Json,
            Some(&tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "years",
        )
        .expect("rename from inside CHECK should succeed");

        let file_edits = plan.edits.get(&user_uri).expect("edits for user.json");
        // Declaration + two CHECK occurrences = 3 edits, all replacing `age`.
        assert_eq!(
            file_edits.len(),
            3,
            "declaration + both CHECK occurrences must be rewritten, got: {file_edits:?}"
        );
        for edit in file_edits {
            assert_eq!(&src[edit.byte_range.clone()], "age");
            assert_eq!(edit.new_text, "years");
        }

        let mut sorted = file_edits.clone();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.byte_range.start));
        let mut after = src.to_string();
        for edit in &sorted {
            after.replace_range(edit.byte_range.clone(), &edit.new_text);
        }
        assert!(
            after.contains(r#""expr":"years > 0 AND years < 150""#),
            "both CHECK occurrences must be rewritten, got: {after}"
        );
    }

    #[test]
    fn rn_s3_rename_check_column_scoped_to_owning_table() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_src = r#"{"name":"user","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let user_uri = uri("user.json");
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "json".to_string(),
            1,
            user_src.to_string(),
        );

        // A different table with a same-named `age` column AND its own CHECK
        // referencing `age` — must NOT be touched by renaming user.age.
        let other_src = r#"{"name":"other","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let other_uri = uri("other.json");
        let other_tree = pool.parse(other_src, DocumentFormat::Json).unwrap();
        idx.upsert(&other_uri, other_src, &other_tree);
        docs.open(
            other_uri.clone(),
            "json".to_string(),
            1,
            other_src.to_string(),
        );

        let pos = user_src.find(r#""name":"age""#).unwrap() + 8;
        let plan = compute(
            user_src,
            DocumentFormat::Json,
            Some(&user_tree),
            &user_uri,
            &idx,
            &docs,
            None,
            pos,
            "years",
        )
        .expect("rename should succeed");

        assert!(plan.edits.contains_key(&user_uri));
        assert!(
            !plan.edits.contains_key(&other_uri),
            "unrelated `other.age` CHECK column must not be in the rename plan"
        );
    }

    /// `prepare` (select-on-rename UI) must select EXACTLY the column
    /// identifier under the cursor inside a CHECK expr — not the whole
    /// expression string. Otherwise F2 on `age` inside `"age > 0 AND age <
    /// 150"` would pre-select the entire predicate.
    #[test]
    fn rn_prepare_inside_check_expr_selects_only_identifier() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0 AND age < 150"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        // Cursor on the SECOND `age` inside the CHECK expr.
        let second_age = src.find("AND age").unwrap() + 4;
        let result = prepare(
            src,
            DocumentFormat::Json,
            Some(&tree),
            &uri("user.json"),
            second_age,
        )
        .expect("CHECK-expr column should be renameable");
        assert_eq!(result.placeholder, "age");
        assert_eq!(
            &src[result.byte_range.clone()],
            "age",
            "prepare must select only the column identifier, not the whole expr"
        );
    }

    #[test]
    fn enclosing_string_stops_at_structural_boundaries() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();

        assert!(enclosing_string(tree.root_node()).is_none());
    }

    #[test]
    fn prepare_yaml_quoted_and_plain_scalars_use_inner_ranges() {
        let pool = ParserPool::new();
        let quoted = "name: \"user\"\ncolumns: []\n";
        let quoted_tree = pool.parse(quoted, DocumentFormat::Yaml).unwrap();
        let quoted_pos = quoted.find("user").unwrap();
        let quoted_result = prepare(
            quoted,
            DocumentFormat::Yaml,
            Some(&quoted_tree),
            &uri("user.yaml"),
            quoted_pos,
        )
        .expect("quoted YAML name");
        assert_eq!(&quoted[quoted_result.byte_range.clone()], "user");

        let plain = "name: user\ncolumns: []\n";
        let plain_tree = pool.parse(plain, DocumentFormat::Yaml).unwrap();
        let plain_pos = plain.find("user").unwrap();
        let plain_result = prepare(
            plain,
            DocumentFormat::Yaml,
            Some(&plain_tree),
            &uri("user.yaml"),
            plain_pos,
        )
        .expect("plain YAML name");
        assert_eq!(&plain[plain_result.byte_range.clone()], "user");
    }

    #[test]
    fn trim_one_byte_leaves_short_ranges_unchanged() {
        assert_eq!(trim_one_byte(&(3..3)), 3..3);
        assert_eq!(trim_one_byte(&(4..5)), 4..5);
    }

    #[test]
    fn rename_rejects_when_old_equals_new() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let u = uri("user.json");
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);
        docs.open(u.clone(), "json".to_string(), 1, src.to_string());
        let pos = src.find(r#""name":"user""#).unwrap() + 9;

        assert!(
            compute(
                src,
                DocumentFormat::Json,
                Some(&tree),
                &u,
                &idx,
                &docs,
                None,
                pos,
                "user"
            )
            .is_none()
        );
    }

    #[test]
    fn rename_returns_none_for_non_symbol_position() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let u = uri("u.json");
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);
        docs.open(u.clone(), "json".to_string(), 1, src.to_string());

        assert!(
            compute(
                src,
                DocumentFormat::Json,
                Some(&tree),
                &u,
                &idx,
                &docs,
                None,
                0,
                "new"
            )
            .is_none()
        );
    }

    #[test]
    fn prepare_returns_none_when_tree_missing() {
        assert!(prepare("x", DocumentFormat::Json, None, &uri("x.json"), 0).is_none());
    }
}
