//! Per-action drift dispatcher + record builders + tree-sitter range
//! anchoring helpers.
//!
//! `action_to_drift` translates a single `MigrationAction` into the
//! `(kind, byte_range, message)` triple consumed by `super::compute`.
//! Per-action helpers (`modify_column_type_drift` etc.) and rendering
//! utilities (`render_column_type` etc.) are kept private to this module
//! except where unit tests in `super::tests` reach in.

use std::ops::Range;

use tree_sitter::Tree;
use vespertide_core::{ColumnDef, ColumnType, MigrationAction, TableConstraint, TableDef};

use crate::diagnostics::{
    ErrorField, locate_column, locate_column_field, locate_constraint, locate_top_name,
};

use super::types::{DriftKind, DriftRecord};

pub(super) fn action_to_drift(
    action: &MigrationAction,
    baseline: &[TableDef],
    source: &str,
    tree: Option<&Tree>,
) -> Option<DriftRecord> {
    match action {
        MigrationAction::CreateTable { table, .. } => Some((
            DriftKind::CreateTable,
            locate_table_name(tree, source),
            format!("Table '{table}' is in the model but not in any applied migration"),
        )),
        MigrationAction::DeleteTable { table } => Some((
            DriftKind::DeleteTable,
            locate_table_name(tree, source),
            format!("Table '{table}' is in applied migrations but missing from the model"),
        )),
        MigrationAction::RenameTable { from, to } => Some((
            DriftKind::RenameTable {
                from: from.to_string(),
                to: to.to_string(),
            },
            locate_table_name(tree, source),
            format!("Table rename drift: applied '{from}' → model '{to}'"),
        )),
        MigrationAction::AddColumn { column, .. } => Some((
            DriftKind::AddColumn {
                column: column.name.to_string(),
            },
            locate_column_range(tree, source, &column.name),
            format!(
                "Column '{}' is in the model but not in any applied migration",
                column.name
            ),
        )),
        MigrationAction::DeleteColumn { column, .. } => Some((
            DriftKind::DeleteColumn {
                column: column.to_string(),
            },
            locate_table_name(tree, source),
            format!("Column '{column}' is in applied migrations but missing from the model"),
        )),
        MigrationAction::RenameColumn { from, to, .. } => Some((
            DriftKind::RenameColumn {
                from: from.to_string(),
                to: to.to_string(),
            },
            locate_column_range(tree, source, to),
            format!("Column rename drift: applied '{from}' → model '{to}'"),
        )),
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            ..
        } => Some(modify_column_type_drift(
            baseline, table, column, new_type, source, tree,
        )),
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            ..
        } => Some(modify_column_nullable_drift(
            baseline, table, column, *nullable, source, tree,
        )),
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            ..
        } => Some(modify_column_default_drift(
            baseline,
            table,
            column,
            new_default.as_ref(),
            source,
            tree,
        )),
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => Some(modify_column_comment_drift(
            baseline,
            table,
            column,
            new_comment.as_ref(),
            source,
            tree,
        )),
        MigrationAction::AddConstraint { constraint, .. } => {
            Some(add_constraint_drift(constraint, source, tree))
        }
        MigrationAction::RemoveConstraint { constraint, .. } => {
            Some(remove_constraint_drift(constraint, source, tree))
        }
        MigrationAction::ReplaceConstraint { from, to, .. } => {
            Some(replace_constraint_drift(from, to, source, tree))
        }
        MigrationAction::RawSql { .. } => Some((
            DriftKind::RawSql,
            None,
            "Raw SQL drift — typed introspection unavailable".to_string(),
        )),
        _ => None,
    }
}

/// Look up a column in the baseline schema by table and column name.
pub(super) fn lookup_baseline_column<'a>(
    baseline: &'a [TableDef],
    table_name: &str,
    column_name: &str,
) -> Option<&'a ColumnDef> {
    baseline
        .iter()
        .find(|t| t.name == table_name)
        .and_then(|table| table.columns.iter().find(|c| c.name == column_name))
}

/// Render a column type as a human-readable string.
pub(super) fn render_column_type(t: &ColumnType) -> String {
    match t {
        ColumnType::Simple(st) => format!("{st:?}"),
        ColumnType::Complex(ct) => format!("{ct:?}"),
    }
}

/// Render a default value as a human-readable string.
pub(super) fn render_default(d: Option<&str>) -> String {
    match d {
        Some(v) => format!("\"{v}\""),
        None => "<none>".to_string(),
    }
}

/// Render a nullable flag as a human-readable string.
pub(super) fn render_nullable(n: bool) -> String {
    if n {
        "nullable".to_string()
    } else {
        "not null".to_string()
    }
}

/// Render a comment as a human-readable string.
pub(super) fn render_comment(c: Option<&str>) -> String {
    match c {
        Some(v) => format!("\"{v}\""),
        None => "<none>".to_string(),
    }
}

fn modify_column_type_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    new_type: &ColumnType,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).map_or_else(
        || "<unknown>".to_string(),
        |baseline_column| render_column_type(&baseline_column.r#type),
    );
    let after = render_column_type(new_type);
    (
        DriftKind::ModifyColumnType {
            column: column.to_string(),
            before: before.clone(),
            after: after.clone(),
        },
        locate_column_field_range(tree, source, column, ErrorField::Type),
        format!("Type drift on '{column}': applied {before} → model {after}"),
    )
}

fn modify_column_nullable_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    nullable: bool,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).map_or(!nullable, |c| c.nullable);
    let before_s = render_nullable(before);
    let after_s = render_nullable(nullable);
    (
        DriftKind::ModifyColumnNullable {
            column: column.to_string(),
            before,
            after: nullable,
        },
        locate_column_field_range(tree, source, column, ErrorField::Nullable),
        format!("Nullable drift on '{column}': applied {before_s} → model {after_s}"),
    )
}

fn modify_column_default_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    new_default: Option<&String>,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).and_then(|c| {
        c.default
            .as_ref()
            .map(vespertide_core::DefaultValue::to_sql)
    });
    let after = new_default.cloned();
    let before_s = render_default(before.as_deref());
    let after_s = render_default(after.as_deref());
    (
        DriftKind::ModifyColumnDefault {
            column: column.to_string(),
            before,
            after,
        },
        locate_column_field_range(tree, source, column, ErrorField::Default),
        format!("Default drift on '{column}': applied {before_s} → model {after_s}"),
    )
}

fn modify_column_comment_drift(
    baseline: &[TableDef],
    table: &str,
    column: &str,
    new_comment: Option<&String>,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let before = lookup_baseline_column(baseline, table, column).and_then(|c| c.comment.clone());
    let after = new_comment.cloned();
    let before_s = render_comment(before.as_deref());
    let after_s = render_comment(after.as_deref());
    (
        DriftKind::ModifyColumnComment {
            column: column.to_string(),
            before,
            after,
        },
        locate_column_field_range(tree, source, column, ErrorField::Comment),
        format!("Comment drift on '{column}': applied {before_s} → model {after_s}"),
    )
}

fn add_constraint_drift(
    constraint: &TableConstraint,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let name = constraint_name(constraint).map(str::to_string);
    let label = name.as_deref().unwrap_or("<unnamed>");
    (
        DriftKind::AddConstraint { name: name.clone() },
        locate_constraint_range(tree, source, name.as_deref()),
        format!("Constraint added in model: {label}"),
    )
}

fn remove_constraint_drift(
    constraint: &TableConstraint,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let name = constraint_name(constraint).map(str::to_string);
    let label = name.as_deref().unwrap_or("<unnamed>");
    (
        DriftKind::RemoveConstraint { name: name.clone() },
        locate_constraint_range(tree, source, name.as_deref()),
        format!("Constraint in applied migrations missing from model: {label}"),
    )
}

fn replace_constraint_drift(
    from: &TableConstraint,
    to: &TableConstraint,
    source: &str,
    tree: Option<&Tree>,
) -> DriftRecord {
    let name = constraint_name(to).map(str::to_string);
    let from_label = constraint_name(from).unwrap_or("<unnamed>");
    let to_label = name.as_deref().unwrap_or("<unnamed>");
    (
        DriftKind::ReplaceConstraint { name: name.clone() },
        locate_constraint_range(tree, source, name.as_deref()),
        format!("Constraint replaced: {from_label} → {to_label}"),
    )
}

fn locate_table_name(tree: Option<&Tree>, source: &str) -> Option<Range<usize>> {
    tree.map(|tree| locate_top_name(Some(tree), source).unwrap_or(0..1))
}

fn locate_column_range(
    tree: Option<&Tree>,
    source: &str,
    column_name: &str,
) -> Option<Range<usize>> {
    tree.map(|tree| locate_column(Some(tree), source, column_name))
}

fn locate_column_field_range(
    tree: Option<&Tree>,
    source: &str,
    column_name: &str,
    field: ErrorField,
) -> Option<Range<usize>> {
    tree.map(|tree| locate_column_field(Some(tree), source, column_name, field))
}

fn locate_constraint_range(
    tree: Option<&Tree>,
    source: &str,
    name: Option<&str>,
) -> Option<Range<usize>> {
    tree.map(|tree| {
        name.map(|name| locate_constraint(Some(tree), source, name))
            .or_else(|| locate_top_name(Some(tree), source))
            .unwrap_or(0..1)
    })
}

fn constraint_name(constraint: &TableConstraint) -> Option<&str> {
    match constraint {
        TableConstraint::Unique { name, .. }
        | TableConstraint::ForeignKey { name, .. }
        | TableConstraint::Index { name, .. } => name.as_deref(),
        TableConstraint::Check { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use vespertide_core::{ColumnDef, ComplexColumnType, SimpleColumnType};

    #[test]
    fn unsupported_remap_enum_values_action_has_no_drift_record() {
        let action = MigrationAction::RemapEnumValues {
            table: "users".into(),
            column: "status".into(),
            mapping: BTreeMap::from([(1, 2)]),
        };

        assert!(action_to_drift(&action, &[], "", None).is_none());
    }

    #[test]
    fn render_column_type_covers_complex_types() {
        let rendered = render_column_type(&ColumnType::Complex(ComplexColumnType::Varchar {
            length: 32,
        }));

        assert!(rendered.contains("Varchar"));
    }

    #[test]
    fn constraint_name_covers_check_and_unnamed_constraints() {
        let check = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let pk = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };

        assert_eq!(constraint_name(&check), Some("chk_age"));
        assert_eq!(constraint_name(&pk), None);
    }

    #[test]
    fn lookup_baseline_column_finds_matching_column() {
        let table = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: Vec::new(),
        };

        assert!(lookup_baseline_column(&[table], "users", "id").is_some());
    }
}
