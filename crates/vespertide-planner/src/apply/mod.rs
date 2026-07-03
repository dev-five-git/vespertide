mod column_ops;
mod constraint_ops;
mod raw_sql;
mod table_ops;

#[cfg(test)]
mod tests;

use vespertide_core::{MigrationAction, TableDef};

use crate::error::PlannerError;

/// Apply a single migration action to an in-memory schema snapshot.
pub fn apply_action(
    schema: &mut Vec<TableDef>,
    action: &MigrationAction,
) -> Result<(), PlannerError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => table_ops::create_table(schema, table, columns, constraints),
        MigrationAction::DeleteTable { table } => table_ops::delete_table(schema, table),
        MigrationAction::RenameTable { from, to } => table_ops::rename_table(schema, from, to),
        MigrationAction::AddColumn {
            table,
            column,
            fill_with: _,
        } => column_ops::add_column(schema, table, column),
        MigrationAction::DeleteColumn { table, column } => {
            column_ops::delete_column(schema, table, column)
        }
        MigrationAction::RenameColumn { table, from, to } => {
            column_ops::rename_column(schema, table, from, to)
        }
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            ..
        } => column_ops::modify_column_type(schema, table, column, new_type),
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with: _,
            delete_null_rows: _,
        } => column_ops::modify_column_nullable(schema, table, column, *nullable),
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            ..
        } => column_ops::modify_column_default(schema, table, column, new_default.as_deref()),
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => column_ops::modify_column_comment(schema, table, column, new_comment.as_ref()),
        MigrationAction::AddConstraint { table, constraint } => {
            constraint_ops::add_constraint(schema, table, constraint)
        }
        MigrationAction::RemoveConstraint { table, constraint } => {
            constraint_ops::remove_constraint(schema, table, constraint)
        }
        MigrationAction::ReplaceConstraint { table, from, to } => {
            constraint_ops::replace_constraint(schema, table, from, to)
        }
        MigrationAction::RemapEnumValues {
            table,
            column,
            mapping,
        } => column_ops::remap_enum_values(schema, table, column, mapping),
        MigrationAction::RawSql { .. } | _ => {
            raw_sql::apply_raw_sql();
            Ok(())
        }
    }
}
