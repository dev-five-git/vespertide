use std::collections::BTreeMap;

use vespertide_core::{ColumnType, TableConstraint, TableDef};

use crate::error::QueryError;
use crate::sql::helpers::{build_sqlite_table_rebuild, require_table_in_schema};
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
    let table_def = require_table_in_schema(
        current_schema,
        table,
        "SQLite requires current schema information to modify column types",
    )?;

    let mut new_columns = table_def.columns.clone();
    let col_index = new_columns
        .iter()
        .position(|c| c.name == column)
        .ok_or_else(|| {
            QueryError::SchemaError(format!("Column '{column}' not found in table '{table}'"))
        })?;
    new_columns[col_index].r#type = new_type.clone();

    let mut queries = Vec::new();

    // Fill-with UPDATE statements run BEFORE the rebuild so the rows
    // copied into the temp table already carry the remapped values.
    super::extend_fill_with_updates(&mut queries, table, column, fill_with);

    queries.extend(build_sqlite_table_rebuild(
        backend,
        table,
        &new_columns,
        &table_def.constraints,
        &new_columns,
        &table_def.constraints,
        pending_constraints,
    ));

    Ok(queries)
}
