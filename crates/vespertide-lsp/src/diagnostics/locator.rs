//! Locate planner validation errors in source text.

use std::ops::Range;

use vespertide_planner::PlannerError;

/// Specific column field a diagnostic should attach to. The locator narrows
/// the highlighted range from the whole column object down to this child
/// pair so the squiggle lands on the *responsible* line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorField {
    /// The column's `type` value (string or object).
    Type,
    /// The column's `default` value.
    Default,
    /// `foreign_key.ref_table`.
    ForeignKeyRefTable,
    /// `foreign_key.ref_columns`.
    ForeignKeyRefColumns,
    /// The column's `nullable` value.
    Nullable,
    /// The column's `comment` value.
    Comment,
}

/// Structured location extracted from a planner error.
pub(crate) struct ErrorLocation {
    /// Table responsible for the diagnostic.
    pub table: String,
    /// Column responsible for the diagnostic, when the planner provides one.
    pub column: Option<String>,
    /// Constraint or index responsible for the diagnostic, when available.
    pub constraint: Option<String>,
    /// More precise location inside the column object.
    pub field: Option<ErrorField>,
}

impl ErrorLocation {
    /// Extract the table/column/constraint tuple carried by a planner error.
    pub fn from_planner_error(err: &PlannerError) -> Option<Self> {
        use PlannerError::{
            AddColumnWithFkRequiresNullable, BetweenBoundaryReversed, CheckSelfContradiction,
            ColumnExists, ColumnNotFound, ConstraintColumnNotFound, ConstraintTypeChanged,
            DanglingForeignKeyAfterDrop, DefaultViolatesCheck, DuplicateEnumValue,
            DuplicateEnumVariantName, DuplicateTableName, EmptyConstraintColumns,
            ForeignKeyColumnNotFound, ForeignKeyTableNotFound, IndexColumnNotFound, IndexNotFound,
            InvalidAutoIncrement, InvalidEnumDefault, MissingFillWith, MissingPrimaryKey, Multiple,
            PrimaryKeyColumnNullable, PrimaryKeyRemovedWithoutReplacement, TableExists,
            TableNotFound, TableValidation,
        };

        match err {
            // Batched validation errors carry several independent violations.
            // Locator returns *one* location per call, so we recurse into the
            // first nested error to pick a sensible anchor. Per-violation
            // diagnostics belong to the publisher (see TODO in
            // `diagnostics::validation`): when it iterates `find_*_violations`
            // each inner error is presented directly and this arm is bypassed.
            Multiple(batch) => batch.0.first().and_then(Self::from_planner_error),
            // F9 dangling FK after drop: anchor on the *dropped* target so the
            // squiggle lands on the column/table the user removed. When only
            // the table name is known (`dropped_column = None`), fall through
            // to table-level location.
            DanglingForeignKeyAfterDrop {
                dropped_table,
                dropped_column,
                ..
            } => Some(match dropped_column {
                Some(col) => Self::column(dropped_table, col),
                None => Self::table(dropped_table),
            }),
            // Table-anchored errors. F12 PK↔UQ swap / PK removal without
            // replacement are listed alongside the existing table-name
            // family because column-level anchoring on F12 would require
            // parsing the comma-joined `columns` string, which is not
            // worth the per-character precision for an LSP squiggle.
            ConstraintTypeChanged { table, .. }
            | PrimaryKeyRemovedWithoutReplacement { table, .. }
            | TableExists(table)
            | TableNotFound(table)
            | DuplicateTableName(table)
            | MissingPrimaryKey(table) => Some(Self::table(table)),
            TableValidation(_) => None,
            // Column-anchored errors. F12 Scenario C
            // (`PrimaryKeyColumnNullable`) is a struct variant rather than
            // a tuple, so its arm is listed separately even though the
            // location resolution is identical — clippy's `match_same_arms`
            // is silenced because merging would require restructuring the
            // enum (tuple vs struct), not a meaningful code change.
            ColumnExists(table, column)
            | ColumnNotFound(table, column)
            | MissingFillWith(table, column)
            | DuplicateEnumVariantName(_, table, column, _)
            | DuplicateEnumValue(_, table, column, _) => Some(Self::column(table, column)),
            // Struct-variant column-anchor group. Both
            // `PrimaryKeyColumnNullable` (F12 Scenario C) and
            // `AddColumnWithFkRequiresNullable` (F3 Edge #1) resolve to a
            // column location identically; merged so the arm is shared
            // (clippy's `match_same_arms` would otherwise fire).
            PrimaryKeyColumnNullable { table, column }
            | AddColumnWithFkRequiresNullable { table, column }
            | BetweenBoundaryReversed { table, column, .. }
            | CheckSelfContradiction { table, column, .. } => Some(Self::column(table, column)),
            InvalidAutoIncrement(table, column, _) => {
                Some(Self::column_field(table, column, ErrorField::Type))
            }
            ForeignKeyTableNotFound(table, column, _) => Some(Self::column_field(
                table,
                column,
                ErrorField::ForeignKeyRefTable,
            )),
            ForeignKeyColumnNotFound(table, column, _, _) => Some(Self::column_field(
                table,
                column,
                ErrorField::ForeignKeyRefColumns,
            )),
            IndexNotFound(table, index) | EmptyConstraintColumns(table, index) => {
                Some(Self::constraint(table, index))
            }
            IndexColumnNotFound(table, index, column)
            | ConstraintColumnNotFound(table, index, column) => Some(Self {
                table: table.clone(),
                column: Some(column.clone()),
                constraint: Some(index.clone()),
                field: None,
            }),
            InvalidEnumDefault(err) => Some(Self::column_field(
                &err.table_name,
                &err.column_name,
                ErrorField::Default,
            )),
            DefaultViolatesCheck { table, column, .. } => {
                // Squiggle goes on the `default` value — that's the field
                // the user has to change to make the CHECK satisfiable. The
                // CHECK constraint itself is also a candidate, but pointing
                // at the column default is the more actionable target.
                Some(Self::column_field(table, column, ErrorField::Default))
            }
        }
    }

    fn table(table: &str) -> Self {
        Self {
            table: table.to_string(),
            column: None,
            constraint: None,
            field: None,
        }
    }

    fn column(table: &str, column: &str) -> Self {
        Self {
            table: table.to_string(),
            column: Some(column.to_string()),
            constraint: None,
            field: None,
        }
    }

    fn column_field(table: &str, column: &str, field: ErrorField) -> Self {
        Self {
            table: table.to_string(),
            column: Some(column.to_string()),
            constraint: None,
            field: Some(field),
        }
    }

    fn constraint(table: &str, constraint: &str) -> Self {
        Self {
            table: table.to_string(),
            column: None,
            constraint: Some(constraint.to_string()),
            field: None,
        }
    }
}

/// Find the source range for a named column object.
///
/// Falls back to the table's top-level `name` value, then `0..1`.
pub(crate) fn locate_column(
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    column_name: &str,
) -> Range<usize> {
    let Some(tree) = tree else {
        return 0..1;
    };

    locate_named_object(tree, source, "columns", column_name)
        .or_else(|| locate_top_name(Some(tree), source))
        .unwrap_or(0..1)
}

/// Find the source range for a specific FIELD of a named column. Falls back
/// to the column object, then the table's top-level `name`, then `0..1`.
pub(crate) fn locate_column_field(
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    column_name: &str,
    field: ErrorField,
) -> Range<usize> {
    let Some(tree) = tree else {
        return 0..1;
    };

    locate_field_in_column(tree, source, column_name, field)
        .or_else(|| locate_named_object(tree, source, "columns", column_name))
        .or_else(|| locate_top_name(Some(tree), source))
        .unwrap_or(0..1)
}

fn locate_field_in_column(
    tree: &tree_sitter::Tree,
    source: &str,
    column_name: &str,
    field: ErrorField,
) -> Option<Range<usize>> {
    let column = find_named_mapping(tree.root_node(), source.as_bytes(), "columns", column_name)?;

    match field {
        ErrorField::Type => {
            find_child_pair(column, source.as_bytes(), "type").map(|pair| pair.byte_range())
        }
        ErrorField::Default => {
            find_child_pair(column, source.as_bytes(), "default").map(|pair| pair.byte_range())
        }
        ErrorField::Nullable => {
            find_child_pair(column, source.as_bytes(), "nullable").map(|pair| pair.byte_range())
        }
        ErrorField::Comment => {
            find_child_pair(column, source.as_bytes(), "comment").map(|pair| pair.byte_range())
        }
        ErrorField::ForeignKeyRefTable | ErrorField::ForeignKeyRefColumns => {
            let fk_pair = find_child_pair(column, source.as_bytes(), "foreign_key")?;
            let fk_value = fk_pair.named_child(1)?;
            let sub_key = match field {
                ErrorField::ForeignKeyRefTable => "ref_table",
                ErrorField::ForeignKeyRefColumns => "ref_columns",
                _ => unreachable!(),
            };
            find_child_pair(fk_value, source.as_bytes(), sub_key)
                .map(|pair| pair.byte_range())
                // If the sub-field is missing, fall back to the whole fk pair.
                .or(Some(fk_pair.byte_range()))
        }
    }
}

fn find_named_mapping<'tree>(
    root: tree_sitter::Node<'tree>,
    source: &[u8],
    collection_key: &str,
    target_name: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let collection = find_value_for_key(root, source, collection_key)?;
    find_named_mapping_in(collection, source, target_name)
}

fn find_named_mapping_in<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_name: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_mapping(child) && mapping_has_name(child, source, target_name) {
            return Some(child);
        }
        if let Some(found) = find_named_mapping_in(child, source, target_name) {
            return Some(found);
        }
    }
    None
}

fn find_child_pair<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|&child| is_pair(child) && pair_key_matches(child, source, target_key))
}

/// Find the source range for a named constraint object.
///
/// Falls back to the table's top-level `name` value, then `0..1`.
pub(crate) fn locate_constraint(
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    constraint_name: &str,
) -> Range<usize> {
    let Some(tree) = tree else {
        return 0..1;
    };

    locate_named_object(tree, source, "constraints", constraint_name)
        .or_else(|| locate_top_name(Some(tree), source))
        .unwrap_or(0..1)
}

/// Find the source range for the TABLE-LEVEL `name` value.
///
/// Locates the outermost mapping in the document and returns the byte range
/// of its direct `name` pair value. Crucially, this does NOT recurse into
/// nested objects — when JSON puts `columns` before `name`, a naive walk
/// would land on the first column's `name` field, producing wildly wrong
/// diagnostic positions like the "duplicate table name" warning showing up
/// on a column's name.
pub(crate) fn locate_top_name(
    tree: Option<&tree_sitter::Tree>,
    source: &str,
) -> Option<Range<usize>> {
    let tree = tree?;
    let source_bytes = source.as_bytes();
    let mapping = find_outer_mapping(tree.root_node())?;
    direct_name_value_range(mapping, source_bytes)
}

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

fn direct_name_value_range(mapping: tree_sitter::Node<'_>, source: &[u8]) -> Option<Range<usize>> {
    let mut cursor = mapping.walk();
    for child in mapping.children(&mut cursor) {
        if is_pair(child)
            && pair_key_matches(child, source, "name")
            && let Some(value) = child.named_child(1)
        {
            return Some(value.byte_range());
        }
    }
    None
}

fn locate_named_object(
    tree: &tree_sitter::Tree,
    source: &str,
    collection_key: &str,
    target_name: &str,
) -> Option<Range<usize>> {
    let collection = find_value_for_key(tree.root_node(), source.as_bytes(), collection_key)?;
    walk_for_named_mapping(collection, source.as_bytes(), target_name)
}

fn find_value_for_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child)
            && pair_key_matches(child, source, target_key)
            && let Some(value) = child.named_child(1)
        {
            return Some(value);
        }
        if let Some(value) = find_value_for_key(child, source, target_key) {
            return Some(value);
        }
    }
    None
}

fn walk_for_named_mapping(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    target_name: &str,
) -> Option<Range<usize>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_mapping(child) && mapping_has_name(child, source, target_name) {
            return Some(child.byte_range());
        }
        if let Some(range) = walk_for_named_mapping(child, source, target_name) {
            return Some(range);
        }
    }
    None
}

fn mapping_has_name(node: tree_sitter::Node<'_>, source: &[u8], target_name: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_pair(child)
            && pair_key_matches(child, source, "name")
            && let Some(value) = child.named_child(1)
            && node_text(value, source).is_some_and(|text| strip_quotes(text) == target_name)
        {
            return true;
        }
    }
    false
}

fn is_mapping(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "object" | "block_mapping")
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn pair_key_matches(node: tree_sitter::Node<'_>, source: &[u8], expected: &str) -> bool {
    // Fused chain so a key-less pair (no `named_child(0)`) folds into the
    // same `false` result without a separate defensive `return` line.
    node.named_child(0)
        .and_then(|key| node_text(key, source))
        .is_some_and(|text| strip_quotes(text) == expected)
}

fn node_text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    std::str::from_utf8(&source[node.byte_range()]).ok()
}

fn strip_quotes(s: &str) -> &str {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|without_prefix| without_prefix.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|without_prefix| without_prefix.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use rstest::rstest;
    use vespertide_planner::error::InvalidEnumDefaultError;
    use vespertide_planner::{MultipleErrors, PlannerError};

    #[test]
    fn locate_column_finds_target_byte_range() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_column(Some(&tree), src, "email");
        let snippet = &src[range];

        assert!(snippet.contains(r#""email""#), "got: {snippet}");
    }

    #[test]
    fn locate_column_fallback_to_top_name() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_column(Some(&tree), src, "nonexistent");

        assert!(src[range].contains("user"));
    }

    #[test]
    fn locate_constraint_fallback_to_top_name_when_constraint_missing() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","constraints":[]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_constraint(Some(&tree), src, "missing_constraint");

        assert!(src[range].contains("user"));
    }

    #[test]
    fn locate_column_field_nullable_finds_target() {
        let pool = ParserPool::new();
        let src =
            r#"{"name":"user","columns":[{"name":"the_col","type":"integer","nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_column_field(Some(&tree), src, "the_col", ErrorField::Nullable);
        let snippet = &src[range];

        assert!(snippet.contains("nullable"), "got: {snippet}");
        assert!(snippet.contains("false"), "got: {snippet}");
    }

    #[test]
    fn locate_column_field_comment_finds_target() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[{"name":"the_col","type":"text","comment":"user email"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locate_column_field(Some(&tree), src, "the_col", ErrorField::Comment);
        let snippet = &src[range];

        assert!(snippet.contains("comment"), "got: {snippet}");
        assert!(snippet.contains("user email"), "got: {snippet}");
    }

    #[rstest]
    #[case::multiple(PlannerError::Multiple(Box::new(MultipleErrors(vec![PlannerError::ColumnExists("users".into(), "email".into())]))), Some("email"), None, None)]
    #[case::dangling_column(PlannerError::DanglingForeignKeyAfterDrop { dropped_table: "users".into(), dropped_column: Some("id".into()), referencing_table: "posts".into(), referencing_constraint: None }, Some("id"), None, None)]
    #[case::dangling_table(PlannerError::DanglingForeignKeyAfterDrop { dropped_table: "users".into(), dropped_column: None, referencing_table: "posts".into(), referencing_constraint: None }, None, None, None)]
    #[case::column_tuple(PlannerError::ColumnNotFound("users".into(), "missing".into()), Some("missing"), None, None)]
    #[case::column_struct(PlannerError::PrimaryKeyColumnNullable { table: "users".into(), column: "id".into() }, Some("id"), None, None)]
    #[case::auto_increment_type(PlannerError::InvalidAutoIncrement("users".into(), "slug".into(), "text".into()), Some("slug"), None, Some(ErrorField::Type))]
    fn planner_error_location_remaining_column_and_table_arms(
        #[case] err: PlannerError,
        #[case] column: Option<&str>,
        #[case] constraint: Option<&str>,
        #[case] field: Option<ErrorField>,
    ) {
        let loc = ErrorLocation::from_planner_error(&err).expect("located planner error");

        assert_eq!(loc.table, "users");
        assert_eq!(loc.column.as_deref(), column);
        assert_eq!(loc.constraint.as_deref(), constraint);
        assert_eq!(loc.field, field);
    }

    #[test]
    fn planner_error_location_table_validation_has_no_anchor() {
        let err = PlannerError::TableValidation("bad table".to_string());
        assert!(ErrorLocation::from_planner_error(&err).is_none());
    }

    #[test]
    fn planner_error_location_invalid_enum_default_anchors_default_field() {
        let err = PlannerError::InvalidEnumDefault(Box::new(InvalidEnumDefaultError {
            enum_name: "status".into(),
            table_name: "users".into(),
            column_name: "status".into(),
            value_type: "default".into(),
            value: "'bad'".into(),
            allowed: "active".into(),
        }));
        let loc = ErrorLocation::from_planner_error(&err).expect("invalid enum default location");

        assert_eq!(loc.table, "users");
        assert_eq!(loc.column.as_deref(), Some("status"));
        assert_eq!(loc.field, Some(ErrorField::Default));
    }

    #[test]
    fn locators_without_tree_use_default_range() {
        assert_eq!(locate_column(None, "", "id"), 0..1);
        assert_eq!(locate_column_field(None, "", "id", ErrorField::Type), 0..1);
        assert_eq!(locate_constraint(None, "", "pk_id"), 0..1);
    }

    #[test]
    fn locate_top_name_returns_none_when_document_has_no_mapping() {
        let pool = ParserPool::new();
        let src = "just_a_scalar\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();

        assert!(locate_top_name(Some(&tree), src).is_none());
    }

    #[test]
    fn malformed_named_mapping_without_value_is_skipped() {
        let pool = ParserPool::new();
        let src = r#"{"name":"users","columns":[{"name":}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();

        let range = locate_column(Some(&tree), src, "missing");

        assert!(src[range].contains("users"));
    }
}
