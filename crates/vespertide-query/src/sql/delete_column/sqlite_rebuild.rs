use sea_query::{Alias, Query, Table};

use vespertide_core::{ColumnType, TableConstraint, TableDef};

use crate::sql::helpers::{
    build_drop_enum_type_sql, build_sqlite_temp_table_create, recreate_indexes_after_rebuild,
};
use crate::sql::rename_table::build_rename_table;
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
pub(super) fn build_delete_column_sqlite_temp_table(
    table: &str,
    column: &str,
    table_def: &TableDef,
    column_type: Option<&ColumnType>,
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    // perf: SQLite rebuild emits four fixed statements plus recreated indexes.
    let mut stmts = Vec::with_capacity(4 + table_def.constraints.len());
    let temp_table = format!("{table}_temp");

    // Build new columns list without the deleted column.
    let new_columns: Vec<_> = table_def
        .columns
        .iter()
        .filter(|c| c.name != column)
        .cloned()
        .collect();

    // Build new constraints list without constraints referencing the deleted column.
    let new_constraints: Vec<_> = table_def
        .constraints
        .iter()
        .filter(|c| {
            // For CHECK constraints, check if expression references the column.
            if let TableConstraint::Check { expr, .. } = c {
                return !expr.contains(&format!("\"{column}\"")) && !expr.contains(column);
            }
            !c.columns().iter().any(|col| col == column)
        })
        .cloned()
        .collect();

    // 1. Create temp table without the column + CHECK constraints.
    let create_query = build_sqlite_temp_table_create(
        DatabaseBackend::Sqlite,
        &temp_table,
        table,
        &new_columns,
        &new_constraints,
    );
    stmts.push(create_query);

    // 2. Copy data (excluding the deleted column).
    let column_aliases: Vec<Alias> = new_columns.iter().map(|c| Alias::new(&c.name)).collect();
    let mut select_query = Query::select();
    for col_alias in &column_aliases {
        select_query.column(col_alias.clone());
    }
    select_query.from(Alias::new(table));

    let insert_stmt = Query::insert()
        .into_table(Alias::new(&temp_table))
        .columns(column_aliases.clone())
        .select_from(select_query)
        .unwrap()
        .to_owned();
    stmts.push(BuiltQuery::Insert(Box::new(insert_stmt)));

    // 3. Drop original table.
    let drop_table = Table::drop().table(Alias::new(table)).to_owned();
    stmts.push(BuiltQuery::DropTable(Box::new(drop_table)));

    // 4. Rename temp table to original name.
    stmts.push(build_rename_table(&temp_table, table));

    // 5. Recreate indexes (both regular and UNIQUE) that don't reference the deleted column.
    stmts.extend(recreate_indexes_after_rebuild(
        table,
        &new_constraints,
        pending_constraints,
    ));

    // If column type is an enum, drop the type after (PostgreSQL only, but include for completeness).
    if let Some(col_type) = column_type
        && let Some(drop_type_sql) = build_drop_enum_type_sql(table, col_type)
    {
        stmts.push(BuiltQuery::Raw(drop_type_sql));
    }

    stmts
}
