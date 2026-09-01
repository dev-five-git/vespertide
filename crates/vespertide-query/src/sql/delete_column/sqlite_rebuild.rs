use vespertide_core::{ColumnType, TableConstraint, TableDef};

use crate::sql::helpers::{build_drop_enum_type_sql, build_sqlite_table_rebuild};
use crate::sql::types::{BuiltQuery, DatabaseBackend};

/// `SQLite` temp table approach for deleting a column that has FK, PK, CHECK, or
/// enum-backed constraints.
///
/// Steps:
/// 1. Create temp table without the column (and without constraints referencing it)
/// 2. Copy data (excluding the deleted column)
/// 3. Drop original table
/// 4. Rename temp table to original name
/// 5. Recreate indexes that don't reference the deleted column
/// 6. If the column type was an enum, drop the enum type (PostgreSQL only;
///    included for completeness)
pub(super) fn build_delete_column_sqlite_temp_table(
    table: &str,
    column: &str,
    table_def: &TableDef,
    column_type: Option<&ColumnType>,
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    // Build new columns list without the deleted column.
    let new_columns: Vec<_> = table_def
        .columns
        .iter()
        .filter(|c| c.name != column)
        .cloned()
        .collect();

    // Build new constraints list without constraints referencing the deleted column.
    // perf: pre-quote the column identifier once instead of re-allocating the
    // same `"col"` literal inside the closure on every CHECK constraint visited.
    let quoted_col = format!("\"{column}\"");
    let new_constraints: Vec<_> = table_def
        .constraints
        .iter()
        .filter(|c| {
            // For CHECK constraints, check if expression references the column.
            if let TableConstraint::Check { expr, .. } = c {
                return !expr.contains(&quoted_col) && !expr.contains(column);
            }
            !c.columns().iter().any(|col| col == column)
        })
        .cloned()
        .collect();

    let mut stmts = build_sqlite_table_rebuild(
        DatabaseBackend::Sqlite,
        table,
        &new_columns,
        &new_constraints,
        &new_columns,
        &new_constraints,
        pending_constraints,
    );

    // If column type is an enum, drop the type after (PostgreSQL only,
    // but include for completeness).
    if let Some(col_type) = column_type
        && let Some(drop_type_sql) = build_drop_enum_type_sql(table, col_type)
    {
        stmts.push(BuiltQuery::Raw(drop_type_sql));
    }

    stmts
}
