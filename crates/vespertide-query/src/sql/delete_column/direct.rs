use sea_query::{Alias, Table};

use vespertide_core::ColumnType;

use crate::sql::helpers::build_drop_enum_type_sql;
use crate::sql::types::BuiltQuery;

/// Build direct `ALTER TABLE ... DROP COLUMN` queries.
///
/// `PostgreSQL` receives an additional `DROP TYPE` statement when the deleted
/// column is backed by a native enum type. Other backends build the raw query
/// too, but it renders to an empty string outside `PostgreSQL`.
pub(super) fn build_direct_delete_column(
    table: &str,
    column: &str,
    column_type: Option<&ColumnType>,
) -> Vec<BuiltQuery> {
    let mut stmts = Vec::new();

    let stmt = Table::alter()
        .table(Alias::new(table))
        .drop_column(Alias::new(column))
        .to_owned();
    stmts.push(BuiltQuery::AlterTable(Box::new(stmt)));

    // If column type is an enum, drop the type after (PostgreSQL only).
    // Note: Only drop if this is the last column using this enum type.
    if let Some(col_type) = column_type
        && let Some(drop_type_sql) = build_drop_enum_type_sql(table, col_type)
    {
        stmts.push(BuiltQuery::Raw(drop_type_sql));
    }

    stmts
}
