use std::collections::BTreeMap;

use sea_query::{Alias, Query, Table};

use vespertide_core::{ColumnType, TableConstraint, TableDef};

use crate::error::QueryError;
use crate::sql::helpers::{build_sqlite_temp_table_create, recreate_indexes_after_rebuild};
use crate::sql::rename_table::build_rename_table;
use crate::sql::types::{BuiltQuery, DatabaseBackend};

/// Build the canonical `SQLite` temp-table rebuild sequence for column type changes.
pub(super) fn build_modify_column_type_sqlite_temp_table(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // Current schema information is required
    let table_def = current_schema.iter().find(|t| t.name == table).ok_or_else(|| QueryError::SchemaError(format!("Table '{table}' not found in current schema. SQLite requires current schema information to modify column types.")))?;

    // Create new column definitions with the modified column
    let mut new_columns = table_def.columns.clone();
    let col_index = new_columns
        .iter()
        .position(|c| c.name == column)
        .ok_or_else(|| {
            QueryError::SchemaError(format!("Column '{column}' not found in table '{table}'"))
        })?;

    new_columns[col_index].r#type = new_type.clone();

    // Generate temporary table name
    let temp_table = format!("{table}_temp");

    // 1. Create temporary table with new column types + CHECK constraints
    let create_query = build_sqlite_temp_table_create(
        backend,
        &temp_table,
        table,
        &new_columns,
        &table_def.constraints,
    );

    // 2. Copy data (all columns) - Use INSERT INTO ... SELECT
    let column_aliases: Vec<Alias> = new_columns.iter().map(|c| Alias::new(&c.name)).collect();

    // Build SELECT query
    let mut select_query = Query::select();
    for col_alias in &column_aliases {
        select_query.column(col_alias.clone());
    }
    select_query.from(Alias::new(table));

    // Build INSERT query
    let insert_stmt = Query::insert()
        .into_table(Alias::new(&temp_table))
        .columns(column_aliases.clone())
        .select_from(select_query)
        .unwrap()
        .to_owned();

    let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

    // 3. Drop original table
    let drop_table = Table::drop().table(Alias::new(table)).to_owned();
    let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

    // 4. Rename temporary table to original name
    let rename_query = build_rename_table(&temp_table, table);

    // 5. Recreate indexes (both regular and UNIQUE)
    let index_queries =
        recreate_indexes_after_rebuild(table, &table_def.constraints, pending_constraints);

    let mut queries = Vec::new();

    // Insert fill_with UPDATE statements before table recreation
    if let Some(fw) = fill_with {
        queries.extend(super::build_fill_with_updates(table, column, fw));
    }

    queries.extend([create_query, insert_query, drop_query, rename_query]);
    queries.extend(index_queries);

    Ok(queries)
}
