pub mod add_column;
pub mod add_constraint;
pub mod create_table;
pub mod data_migration;
pub mod delete_column;
pub mod delete_table;
pub(crate) mod fill_with;
pub mod helpers;
pub mod modify_column_comment;
pub mod modify_column_default;
pub mod modify_column_nullable;
pub mod modify_column_type;
pub mod raw_sql;
pub mod remap_enum_values;
pub mod remove_constraint;
pub mod rename_column;
pub mod rename_table;
pub mod replace_constraint;
pub mod types;

pub use helpers::{quote_ident, quote_idents};
pub use types::{BuiltQuery, DatabaseBackend, RawSql};

use crate::error::QueryError;
use vespertide_core::{MigrationAction, TableConstraint, TableDef};

use self::{
    add_column::build_add_column, add_constraint::build_add_constraint,
    create_table::build_create_table, data_migration::build_data_migration,
    delete_column::build_delete_column, delete_table::build_delete_table,
    modify_column_comment::build_modify_column_comment,
    modify_column_default::build_modify_column_default,
    modify_column_nullable::build_modify_column_nullable,
    remap_enum_values::build_remap_enum_values, remove_constraint::build_remove_constraint,
    rename_column::build_rename_column, rename_table::build_rename_table,
    replace_constraint::build_replace_constraint,
};

/// Build SQL for a single migration action against a known schema.
///
/// For multi-action plans use [`crate::build_plan_queries`] which handles
/// schema evolution between actions.
pub fn build_action_queries(
    backend: DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    build_action_queries_with_pending(backend, action, current_schema, &[])
}

/// Build SQL queries for a migration action, with awareness of pending constraints.
///
/// `pending_constraints` are constraints that exist in the logical schema but haven't been
/// physically created as database indexes yet. This is used by `SQLite` temp table rebuilds
/// to avoid recreating indexes that will be created by future `AddConstraint` actions.
#[expect(
    clippy::too_many_lines,
    reason = "flat 16-variant MigrationAction dispatcher kept inline so the variant→builder mapping stays auditable; extracting individual arms scatters the routing logic"
)]
pub fn build_action_queries_with_pending(
    backend: DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => build_create_table(backend, table, columns, constraints),

        MigrationAction::DeleteTable { table } => Ok(vec![build_delete_table(table)]),

        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => build_add_column(
            backend,
            table,
            column,
            fill_with.as_deref(),
            current_schema,
            pending_constraints,
        ),

        MigrationAction::RenameColumn { table, from, to } => {
            Ok(vec![build_rename_column(table, from, to)])
        }

        MigrationAction::DeleteColumn { table, column } => {
            // Find the column type from current schema for enum DROP TYPE support
            let column_type =
                helpers::find_column_in_schema(current_schema, table, column).map(|c| &c.r#type);
            Ok(build_delete_column(
                backend,
                table,
                column,
                column_type,
                current_schema,
                pending_constraints,
            ))
        }

        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            fill_with,
            narrowing_strategy,
            timezone,
        } => modify_column_type::build_with_narrowing_preprocess(
            backend,
            table.as_str(),
            column.as_str(),
            new_type,
            fill_with.as_ref(),
            narrowing_strategy.as_ref(),
            timezone.as_deref(),
            current_schema,
            pending_constraints,
        ),

        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with,
            delete_null_rows,
        } => build_modify_column_nullable(
            backend,
            table,
            column,
            *nullable,
            fill_with.as_deref(),
            delete_null_rows.unwrap_or(false),
            current_schema,
            pending_constraints,
        ),

        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            backfill,
        } => build_modify_column_default(
            backend,
            table,
            column,
            new_default.as_deref(),
            backfill.as_deref(),
            current_schema,
            pending_constraints,
        ),

        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => build_modify_column_comment(
            backend,
            table,
            column,
            new_comment.as_deref(),
            current_schema,
        ),

        MigrationAction::RenameTable { from, to } => Ok(vec![build_rename_table(from, to)]),

        MigrationAction::RawSql { sql } => Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql.clone()))]),

        MigrationAction::DataMigration { sql, .. } => Ok(build_data_migration(sql)),

        MigrationAction::AddConstraint { .. }
        | MigrationAction::RemoveConstraint { .. }
        | MigrationAction::ReplaceConstraint { .. } => {
            build_constraint_action_queries(backend, action, current_schema, pending_constraints)
        }

        MigrationAction::RemapEnumValues {
            table,
            column,
            mapping,
        } => build_remap_enum_values(backend, table.as_str(), column.as_str(), mapping),

        _ => unreachable!("MigrationAction is #[non_exhaustive]; all variants are matched above"),
    }
}

fn build_constraint_action_queries(
    backend: DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match action {
        MigrationAction::AddConstraint { table, constraint } => build_add_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        ),
        MigrationAction::RemoveConstraint { table, constraint } => build_remove_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        ),
        MigrationAction::ReplaceConstraint { table, from, to } => build_replace_constraint(
            backend,
            table,
            from,
            to,
            current_schema,
            pending_constraints,
        ),
        _ => unreachable!("only constraint actions are dispatched here"),
    }
}

#[cfg(test)]
mod tests;
