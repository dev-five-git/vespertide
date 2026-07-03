//! Completion provider — pure domain layer (no LSP protocol types).

mod context;
mod values;

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DomainCompletion {
    pub label: String,
    pub kind: CompletionItemKind,
    /// Markdown documentation, if any.
    pub detail: Option<String>,
    /// Text to insert; may differ from the label for snippets.
    pub insert_text: Option<String>,
    /// Sort priority (smaller = higher). Mirrors the sqls pattern.
    pub sort_priority: u8,
    /// Optional UTF-8 byte range to replace. When set, the LSP layer
    /// converts this to a `TextEdit` that overwrites the range (including
    /// surrounding quotes) with `insert_text`. Used to expand `"varchar"`
    /// inside a string into a full `{kind: "varchar", ...}` object.
    pub replace_range_bytes: Option<std::ops::Range<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompletionItemKind {
    /// Enum value, boolean literal, or other scalar value.
    #[default]
    Value,
    /// Object key.
    Property,
    /// Workspace reference, such as a table or column.
    Reference,
    /// Multi-field template.
    Snippet,
}

/// Compute completions at a byte offset. Returns an empty list when no context matches.
#[must_use]
pub fn compute(
    text: &str,
    _format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    byte_offset: usize,
) -> Vec<DomainCompletion> {
    compute_inner(text, tree, index, docs, None, byte_offset)
}

/// Compute completions with disk-discovered workspace tables included.
#[must_use]
pub fn compute_with_workspace_tables(
    text: &str,
    _format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: &WorkspaceTables,
    byte_offset: usize,
) -> Vec<DomainCompletion> {
    compute_inner(text, tree, index, docs, Some(disk_tables), byte_offset)
}

fn compute_inner(
    text: &str,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    byte_offset: usize,
) -> Vec<DomainCompletion> {
    let Some(tree) = tree else {
        tracing::debug!(
            target: "vespertide_lsp::completion",
            byte_offset,
            "no tree — returning empty completion list"
        );
        return Vec::new();
    };

    let ctx = context::detect(tree, text, byte_offset);
    tracing::debug!(
        target: "vespertide_lsp::completion",
        byte_offset,
        context = ?ctx,
        "completion context detected"
    );
    match ctx {
        context::Context::ColumnTypeInString { string_byte_range } => {
            values::column_types_in_string(string_byte_range)
        }
        context::Context::ColumnTypeValue => values::column_types_full(),
        context::Context::OnDeleteAction | context::Context::OnUpdateAction => {
            values::reference_actions()
        }
        context::Context::Nullable | context::Context::PrimaryKey | context::Context::Unique => {
            values::booleans()
        }
        context::Context::RefTable => values::tables_in_workspace(index, disk_tables),
        context::Context::RefColumns { ref_table } => {
            values::columns_of(ref_table.as_str(), index, docs, disk_tables)
        }
        context::Context::TypeKind { string_byte_range } => {
            values::type_kind_values(string_byte_range.as_ref())
        }
        context::Context::DefaultValue {
            type_kind,
            enum_values,
            string_byte_range,
        } => values::default_values(
            type_kind.as_deref(),
            &enum_values,
            string_byte_range.as_ref(),
        ),
        context::Context::CheckExpr {
            table_columns,
            position,
            replace_range_bytes,
        } => match position {
            context::CheckExprPos::Operand => values::check_expr_operands(&table_columns),
            context::CheckExprPos::Operator => values::check_expr_operators(),
            context::CheckExprPos::PartialColumn { prefix } => values::check_expr_partial_columns(
                &table_columns,
                &prefix,
                replace_range_bytes.as_ref(),
            ),
        },
        context::Context::TableTopLevelKey => values::table_top_level_keys(),
        context::Context::ColumnObjectKey => values::column_object_keys(),
        context::Context::ForeignKeyObjectKey => values::foreign_key_object_keys(),
        context::Context::TypeObjectKey => values::type_object_keys(),
        context::Context::None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::parser::ParserPool;
    use crate::test_support::*;
    use tempfile::tempdir;

    fn compute_items(src: &str, format: DocumentFormat, pos: usize) -> Vec<DomainCompletion> {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let tree = parse(src, format);
        compute(src, format, Some(&tree), &idx, &docs, pos)
    }

    fn labels(items: &[DomainCompletion]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    #[test]
    fn compute_with_no_tree_returns_empty_list() {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let items = compute("nothing", DocumentFormat::Json, None, &idx, &docs, 0);

        assert!(items.is_empty(), "no tree → empty completion list");
    }

    #[test]
    fn compute_with_workspace_tables_propagates_to_inner() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("widget.json"), r#"{"name":"widget","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let disk = WorkspaceTables::new();
        assert!(disk.refresh(tmp.path()));

        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let post_src = r#"{"name":"post","columns":[{"name":"x","type":"integer","foreign_key":{"ref_table":""}}]}"#;
        let post_tree = parse(post_src, DocumentFormat::Json);
        let pos = post_src.find(r#""ref_table":"""#).unwrap() + 14;
        let items = compute_with_workspace_tables(
            post_src,
            DocumentFormat::Json,
            Some(&post_tree),
            &idx,
            &docs,
            &disk,
            pos,
        );

        assert!(
            items.iter().any(|item| item.label == "widget"),
            "disk-discovered table must surface, got: {:?}",
            items.iter().map(|item| &item.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completion_inside_column_type_string_offers_simple_plus_replacing_snippets() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"","nullable":false}]}"#;
        let pos = src.find(r#""type":"""#).unwrap() + 8;
        let items = compute_items(src, DocumentFormat::Json, pos);

        let integer = items.iter().find(|item| item.label == "integer").unwrap();
        assert!(integer.replace_range_bytes.is_none());

        let string_start = src.rfind(r#""""#).unwrap();
        let string_end = string_start + 2;
        for label in ["varchar(N)", "char(N)", "numeric(P,S)", "enum"] {
            let snippet = items
                .iter()
                .find(|item| item.label == label)
                .unwrap_or_else(|| panic!("snippet `{label}` should be offered"));
            let range = snippet
                .replace_range_bytes
                .as_ref()
                .unwrap_or_else(|| panic!("`{label}` must carry replace_range_bytes"));
            assert_eq!(range.start, string_start, "{label} start");
            assert_eq!(range.end, string_end, "{label} end");
        }
    }

    #[test]
    fn completion_at_bare_column_type_value_offers_object_snippets() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":,"nullable":false}]}"#;
        let pos = src.find(r#""type":"#).unwrap() + 7;
        let items = compute_items(src, DocumentFormat::Json, pos);

        assert!(items.iter().any(|item| item.label == "varchar(N)"));
        assert!(items.iter().any(|item| item.label == "integer"));
    }

    #[test]
    fn completion_for_on_delete_returns_actions() {
        let src = r#"{"name":"p","columns":[{"name":"x","type":"integer","nullable":false,"foreign_key":{"ref_table":"u","ref_columns":["id"],"on_delete":""}}]}"#;
        let pos = src.find(r#""on_delete":"""#).unwrap() + 14;
        let items = compute_items(src, DocumentFormat::Json, pos);

        assert!(items.iter().any(|item| item.label == "cascade"));
        assert!(items.iter().any(|item| item.label == "set_null"));
    }

    #[test]
    fn yaml_column_type_in_string_offers_simple_types() {
        let src = "name: u\ncolumns:\n  - name: id\n    type: \"\"\n    nullable: false\n";
        let pos = src.find(r#"type: """#).unwrap() + 7;
        let items = compute_items(src, DocumentFormat::Yaml, pos);
        let labels = labels(&items);

        assert!(
            labels.contains(&"integer"),
            "YAML should offer `integer` for type, got: {labels:?}"
        );
        assert!(
            labels.contains(&"uuid"),
            "YAML should offer `uuid` for type, got: {labels:?}"
        );
    }

    #[test]
    fn yaml_default_for_timestamp_offers_now() {
        let src =
            "name: u\ncolumns:\n  - name: created_at\n    type: timestamp\n    default: \"\"\n";
        let pos = src.find(r#"default: """#).unwrap() + 10;
        let items = compute_items(src, DocumentFormat::Yaml, pos);
        let labels = labels(&items);

        assert!(
            labels.contains(&"now()"),
            "YAML default for timestamp should offer now(), got: {labels:?}"
        );
        assert!(
            labels.contains(&"CURRENT_TIMESTAMP"),
            "YAML default should offer CURRENT_TIMESTAMP, got: {labels:?}"
        );
    }

    #[test]
    fn yaml_default_for_string_enum_offers_only_its_values() {
        let src = "name: u\ncolumns:\n  - name: status\n    type:\n      kind: enum\n      name: s\n      values: [active, banned]\n    default: \"\"\n";
        let pos = src.rfind(r#"default: """#).unwrap() + 10;
        let items = compute_items(src, DocumentFormat::Yaml, pos);
        let labels = labels(&items);

        assert!(
            labels.contains(&"'active'"),
            "YAML enum default must surface 'active', got: {labels:?}"
        );
        assert!(
            labels.contains(&"'banned'"),
            "YAML enum default must surface 'banned', got: {labels:?}"
        );
        assert!(
            !labels.contains(&"now()"),
            "enum column must not leak timestamp defaults, got: {labels:?}"
        );
    }

    #[test]
    fn cmp_s1_check_expr_start_suggests_columns() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false},{"name":"name","type":"text","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let cursor_pos = src.find(r#""expr":"""#).unwrap() + 8;
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in ["id", "age", "name"] {
            assert!(
                labels.contains(&expected),
                "CHECK expr start should suggest column `{expected}`, got: {labels:?}"
            );
        }
    }

    #[test]
    fn cmp_s2_after_column_suggests_operators_keywords() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false},{"name":"name","type":"text","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"age "}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let literal_start = src.find(r#""age ""#).unwrap();
        let cursor_pos = literal_start + 1 + "age ".len();
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in [">", "<", "=", "AND", "BETWEEN", "IS NULL", "IN"] {
            assert!(
                labels.contains(&expected),
                "after CHECK column should suggest `{expected}`, got: {labels:?}"
            );
        }
    }

    #[test]
    fn cmp_s3_partial_column_replace_range() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false},{"name":"name","type":"text","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"ag"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let literal_start = src.find(r#""ag""#).unwrap();
        let cursor_pos = literal_start + 1 + "ag".len();
        let expected_range = (literal_start + 1)..(literal_start + 1 + "ag".len());
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        let age = items
            .iter()
            .find(|i| i.label == "age")
            .expect("partial `ag` should suggest age");
        assert_eq!(age.insert_text.as_deref(), Some("age"));
        assert_eq!(age.replace_range_bytes, Some(expected_range.clone()));
        assert_eq!(
            &src[expected_range], "ag",
            "cmp_s3 replace range must cover only the partial SQL token"
        );
    }

    #[test]
    fn cmp_s4_regression_other_contexts() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"i","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"id > 0"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let cursor_pos = src.find(r#""type":"i""#).unwrap() + 9;
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        assert!(items.iter().any(|item| item.label == "integer"));
        assert!(items.iter().any(|item| item.label == "varchar(N)"));
    }

    #[test]
    fn completion_in_column_type_string_offers_simple_and_replacing_object_snippets() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"i","nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let cursor_pos = src.find(r#""type":"i""#).unwrap() + 9;
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        // Simple scalar types still insert in-place (no replacement metadata).
        let integer = items
            .iter()
            .find(|i| i.label == "integer")
            .expect("integer suggestion");
        assert!(
            integer.replace_range_bytes.is_none(),
            "simple types insert at cursor, no range replacement"
        );

        // Object snippets ARE offered, but each carries a `replace_range_bytes`
        // covering the entire enclosing `"i"` literal (quotes included), so
        // accepting the suggestion collapses the quotes into a `{...}` object.
        let string_start = src.find(r#""i""#).unwrap();
        let string_end = string_start + r#""i""#.len();
        for snippet_label in ["varchar(N)", "char(N)", "numeric(P,S)", "enum"] {
            let snippet = items
                .iter()
                .find(|i| i.label == snippet_label)
                .unwrap_or_else(|| panic!("missing snippet `{snippet_label}`"));
            let range = snippet
                .replace_range_bytes
                .as_ref()
                .unwrap_or_else(|| panic!("`{snippet_label}` should carry replace range"));
            assert_eq!(
                range.start, string_start,
                "{snippet_label} replace_range.start must align with opening quote"
            );
            assert_eq!(
                range.end, string_end,
                "{snippet_label} replace_range.end must align with closing quote"
            );
            assert!(
                snippet
                    .insert_text
                    .as_ref()
                    .is_some_and(|t| t.starts_with('{')),
                "{snippet_label} insert_text must be an object literal"
            );
        }
    }

    #[test]
    fn completion_in_bare_column_type_offers_object_snippets() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // Cursor sits at the bare value slot (no surrounding quotes).
        let src = r#"{"name":"u","columns":[{"name":"id","type":,"nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""type":"#).unwrap() + 7;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        assert!(items.iter().any(|item| item.label == "varchar(N)"));
        assert!(items.iter().any(|item| item.label == "integer"));
    }

    #[test]
    fn completion_in_column_object_key_offers_column_keys() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // Cursor sits inside an empty key string in a column object.
        let src = r#"{"name":"u","columns":[{"name":"id","type":"uuid",""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        // The empty key is the second `""` (between the comma and `}`).
        let pos = src.rfind(r#""""#).unwrap() + 1;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in [
            "nullable",
            "primary_key",
            "unique",
            "index",
            "default",
            "foreign_key",
            "comment",
        ] {
            assert!(
                labels.contains(&expected),
                "should suggest `{expected}` key, got: {labels:?}"
            );
        }
    }

    #[test]
    fn completion_in_table_top_level_key_offers_top_keys() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u",""}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.rfind(r#""""#).unwrap() + 1;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in ["columns", "constraints", "$schema"] {
            assert!(
                labels.contains(&expected),
                "should suggest `{expected}` key, got: {labels:?}"
            );
        }
    }

    #[test]
    fn completion_in_foreign_key_object_offers_fk_keys() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"p","columns":[{"name":"a","type":"integer","foreign_key":{""}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.rfind(r#""""#).unwrap() + 1;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in ["ref_table", "ref_columns", "on_delete", "on_update"] {
            assert!(
                labels.contains(&expected),
                "should suggest `{expected}` key, got: {labels:?}"
            );
        }
    }

    #[test]
    fn completion_in_type_object_offers_type_keys() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"c","type":{""}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.rfind(r#""""#).unwrap() + 1;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in ["kind", "length", "precision", "scale", "values"] {
            assert!(
                labels.contains(&expected),
                "should suggest `{expected}` key, got: {labels:?}"
            );
        }
    }

    #[test]
    fn completion_for_kind_value_in_string_offers_complex_kinds() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // Cursor inside `"kind": "<here>"` of a complex type object.
        let src = r#"{"name":"u","columns":[{"name":"x","type":{"kind":""}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""kind":"""#).unwrap() + 8;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in ["varchar", "char", "numeric", "enum", "custom"] {
            assert!(
                labels.contains(&expected),
                "kind completion missing `{expected}`, got: {labels:?}"
            );
        }
        // Replace range must cover the INNER content only (no quotes) so
        // VS Code's prefix-filter does not reject candidates whose label
        // starts with `v` against a `"` prefix. Insert text is the raw
        // identifier — quotes are already in place from the user's typing.
        let varchar = items.iter().find(|i| i.label == "varchar").unwrap();
        let range = varchar
            .replace_range_bytes
            .as_ref()
            .expect("kind completion must carry an inner-content replace range");
        let snippet = &src[range.clone()];
        assert!(
            !snippet.contains('"'),
            "replace range MUST NOT include surrounding quotes, got: {snippet:?}"
        );
        assert_eq!(
            varchar.insert_text.as_deref(),
            Some("varchar"),
            "in-string insert text must be the bare identifier"
        );
    }

    /// Regression — VS Code rejects completion items whose `range` starts
    /// at `"` because the implicit prefix becomes `"` and the label
    /// `varchar` does not match `"` as a prefix. Make sure partial
    /// typing (`"v"`) accepts cleanly with inner-only replacement.
    #[test]
    fn completion_for_kind_value_inner_range_covers_partial_typing() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"x","type":{"kind":"v"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""kind":"v""#).unwrap() + 9; // inside, after `v`
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let varchar = items.iter().find(|i| i.label == "varchar").unwrap();
        let range = varchar.replace_range_bytes.as_ref().unwrap();
        assert_eq!(
            &src[range.clone()],
            "v",
            "must replace ONLY the partial `v`, leaving quotes intact"
        );

        // Apply the edit and confirm the result is valid JSON.
        let mut after = String::from(&src[..range.start]);
        after.push_str(varchar.insert_text.as_deref().unwrap());
        after.push_str(&src[range.end..]);
        assert!(after.contains(r#""kind":"varchar""#), "got: {after}");
        serde_json::from_str::<serde_json::Value>(&after).expect("must parse as JSON");
    }

    /// At a bare value slot (`"kind":` followed immediately by EOF/value)
    /// we still emit JSON-quoted insert text and no replacement range.
    #[test]
    fn completion_for_kind_value_bare_slot_emits_quoted_insert() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"x","type":{"kind":}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""kind":"#).unwrap() + 7;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let varchar = items.iter().find(|i| i.label == "varchar").unwrap();
        assert!(varchar.replace_range_bytes.is_none());
        assert_eq!(varchar.insert_text.as_deref(), Some("\"varchar\""));
    }

    #[test]
    fn completion_for_kind_outside_a_columns_array_returns_no_kind_suggestions() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // `kind` appears at the top level (NOT inside columns/type) — must
        // not surface the complex-type kinds.
        let src = r#"{"name":"u","kind":"","columns":[]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""kind":"""#).unwrap() + 8;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        assert!(
            items.iter().all(|i| i.label != "varchar"),
            "top-level `kind` must not get type-kind completions"
        );
    }

    #[test]
    fn default_completion_falls_back_when_type_cannot_be_resolved() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // No `type` field on the column at all — analyze_sibling_type fails.
        let src = r#"{"name":"u","columns":[{"name":"x","default":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"""#).unwrap() + 11;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"null"));
        // Generic fallback always includes a handful of common literals so
        // the client never sees an empty / single-item list (which some
        // editors silently auto-accept).
        assert!(
            labels.len() > 1,
            "should offer >1 fallback item, got: {labels:?}"
        );
        assert!(labels.contains(&"0"));
        assert!(labels.contains(&"true"));
    }

    #[test]
    fn default_completion_for_timestamp_offers_now() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"c","type":"timestamp","default":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"""#).unwrap() + 11;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        for expected in ["now()", "CURRENT_TIMESTAMP"] {
            assert!(
                labels.contains(&expected),
                "should suggest `{expected}` for timestamp default, got: {labels:?}"
            );
        }
    }

    #[test]
    fn default_completion_for_uuid_offers_gen_random_uuid() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"uuid","default":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"""#).unwrap() + 11;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        assert!(
            items.iter().any(|i| i.label == "gen_random_uuid()"),
            "should suggest gen_random_uuid() for uuid default"
        );
    }

    #[test]
    fn default_completion_for_boolean_offers_true_false() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"active","type":"boolean","default":}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"#).unwrap() + 10;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"true"));
        assert!(labels.contains(&"false"));
        assert!(labels.contains(&"null"));
    }

    #[test]
    fn default_completion_inside_existing_string_replaces_inner_content() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // Cursor at start of `"old_value"`. We replace the INNER content
        // only — the surrounding quotes stay in place. This is what
        // lets VS Code's prefix-filter accept items whose label doesn't
        // begin with a quote (e.g. `now()`).
        let src = r#"{"name":"u","columns":[{"name":"created_at","type":"timestamp","default":"old_value"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let literal_start = src.find(r#""old_value""#).unwrap();
        let cursor_pos = literal_start + 1; // just past the opening quote
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        let now = items
            .iter()
            .find(|i| i.label == "now()")
            .expect("now() should be offered");
        let range = now
            .replace_range_bytes
            .as_ref()
            .expect("must replace, not insert");
        // Replace exactly `old_value` (quotes preserved).
        assert_eq!(&src[range.clone()], "old_value");
        assert_eq!(
            now.insert_text.as_deref(),
            Some("now()"),
            "in-string insert text must be the raw expression — outer quotes already exist"
        );

        // Apply the edit and confirm the result is `"now()"` in valid JSON.
        let mut after = String::from(&src[..range.start]);
        after.push_str(now.insert_text.as_deref().unwrap());
        after.push_str(&src[range.end..]);
        assert!(after.contains(r#""default":"now()""#), "got: {after}");
        serde_json::from_str::<serde_json::Value>(&after).expect("must parse as JSON");
    }

    #[test]
    fn default_completion_for_json_literal_replaces_inner_content() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // Boolean default. Cursor inside `"true"` literal. We now replace
        // the INNER content (so VS Code's filter passes) — `vespertide-core`
        // accepts `"default": "false"` and `"default": false` identically
        // because `DefaultValue::String("false").to_sql()` returns `false`.
        let src = r#"{"name":"u","columns":[{"name":"active","type":"boolean","default":"true"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let cursor_pos = src.find(r#""true""#).unwrap() + 1;
        let items = compute(
            src,
            DocumentFormat::Json,
            tree.as_ref(),
            &idx,
            &docs,
            cursor_pos,
        );

        let false_item = items.iter().find(|i| i.label == "false").unwrap();
        assert_eq!(
            false_item.insert_text.as_deref(),
            Some("false"),
            "in-string insert text is the raw literal"
        );
        let range = false_item.replace_range_bytes.as_ref().unwrap();
        assert_eq!(
            &src[range.clone()],
            "true",
            "replace only the inner content `true`, leaving quotes in place"
        );

        // Applying the edit should produce `"default":"false"` — still valid
        // JSON, still a valid Vespertide boolean default.
        let mut after = String::from(&src[..range.start]);
        after.push_str(false_item.insert_text.as_deref().unwrap());
        after.push_str(&src[range.end..]);
        assert!(after.contains(r#""default":"false""#), "got: {after}");
        serde_json::from_str::<serde_json::Value>(&after).expect("must parse as JSON");
    }

    #[test]
    fn default_completion_for_enum_value_uses_inner_range_for_vscode_filter() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // Regression — enum default completions used to use an outer range
        // (covering the surrounding quotes). VS Code's prefix filter then
        // rejected items because labels like `'active'` do not start with
        // `"`. We now emit an inner-content range so the user sees the
        // candidates and accepting one cleanly fills `"'active'"`.
        let src = r#"{"name":"u","columns":[{"name":"status","type":{"kind":"enum","name":"st","values":["active","banned"]},"default":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"""#).unwrap() + 11;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let active = items
            .iter()
            .find(|i| i.label == "'active'")
            .expect("enum value must be offered for enum default");
        let range = active
            .replace_range_bytes
            .as_ref()
            .expect("must carry a replace range");
        let snippet = &src[range.clone()];
        assert!(
            !snippet.contains('"'),
            "replace range MUST NOT span the surrounding JSON quotes, got: {snippet:?}"
        );
        assert_eq!(
            active.insert_text.as_deref(),
            Some("'active'"),
            "in-string insert is the raw SQL literal"
        );
    }

    #[test]
    fn default_completion_for_string_enum_offers_only_its_values() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"status","type":{"kind":"enum","name":"user_status","values":["active","banned"]},"default":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"""#).unwrap() + 11;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"'active'"),
            "must surface enum value 'active', got {labels:?}"
        );
        assert!(
            labels.contains(&"'banned'"),
            "must surface enum value 'banned', got {labels:?}"
        );
        // Should NOT leak timestamp/uuid defaults for an enum column.
        assert!(!labels.contains(&"now()"), "no timestamp helpers");
        assert!(!labels.contains(&"gen_random_uuid()"), "no uuid helpers");
    }

    #[test]
    fn default_completion_for_int_enum_offers_member_names() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"priority","type":{"kind":"enum","name":"priority_level","values":[{"name":"low","value":0},{"name":"high","value":10}]},"default":""}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""default":"""#).unwrap() + 11;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"'low'"),
            "int-enum names too, got {labels:?}"
        );
        assert!(labels.contains(&"'high'"));
    }

    #[test]
    fn completion_for_nullable_returns_booleans() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find("nullable\":").unwrap() + 10;
        let items = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        assert!(items.iter().any(|item| item.label == "true"));
        assert!(items.iter().any(|item| item.label == "false"));
    }
}
