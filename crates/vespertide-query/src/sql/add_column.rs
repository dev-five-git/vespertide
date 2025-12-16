use sea_query::{Alias, Expr, Query, Table, TableAlterStatement};

use vespertide_core::ColumnDef;

use super::helpers::build_sea_column_def;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

fn build_add_column_alter_for_backend(
    backend: &DatabaseBackend,
    table: &str,
    column: &ColumnDef,
) -> TableAlterStatement {
    let col_def = build_sea_column_def(backend, column);
    Table::alter()
        .table(Alias::new(table))
        .add_column(col_def)
        .to_owned()
}

pub fn build_add_column(
    backend: &DatabaseBackend,
    table: &str,
    column: &ColumnDef,
    fill_with: Option<&str>,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut stmts: Vec<BuiltQuery> = Vec::new();

    // If adding NOT NULL without default, we need special handling
    let needs_backfill = !column.nullable && column.default.is_none() && fill_with.is_some();

    if needs_backfill {
        // Add as nullable first
        let mut temp_col = column.clone();
        temp_col.nullable = true;

        stmts.push(BuiltQuery::AlterTable(Box::new(
            build_add_column_alter_for_backend(backend, table, &temp_col),
        )));

        // Backfill with provided value
        if let Some(fill) = fill_with {
            // Build UPDATE query using sea_query
            let update_stmt = Query::update()
                .table(Alias::new(table))
                .value(Alias::new(&column.name), Expr::cust(fill))
                .to_owned();
            stmts.push(BuiltQuery::Update(Box::new(update_stmt)));
        }

        // Set NOT NULL using ALTER TABLE with modify_column
        // For SQLite, this will need table recreation, but we generate the SQL anyway
        let not_null_col = build_sea_column_def(backend, column);
        let alter_not_null = Table::alter()
            .table(Alias::new(table))
            .modify_column(not_null_col)
            .to_owned();
        stmts.push(BuiltQuery::AlterTable(Box::new(alter_not_null)));
    } else {
        stmts.push(BuiltQuery::AlterTable(Box::new(
            build_add_column_alter_for_backend(backend, table, column),
        )));
    }

    Ok(stmts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnType, SimpleColumnType};

    #[rstest]
    #[case::add_column_with_backfill_postgres(
        "add_column_with_backfill_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD COLUMN \"age\""]
    )]
    #[case::add_column_with_backfill_mysql(
        "add_column_with_backfill_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD COLUMN `age` int"]
    )]
    #[case::add_column_with_backfill_sqlite(
        "add_column_with_backfill_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" ADD COLUMN \"age\""]
    )]
    #[case::add_column_simple_postgres(
        "add_column_simple_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD COLUMN \"nickname\""]
    )]
    #[case::add_column_simple_mysql(
        "add_column_simple_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD COLUMN `nickname` text"]
    )]
    #[case::add_column_simple_sqlite(
        "add_column_simple_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" ADD COLUMN \"nickname\""]
    )]
    fn test_add_column(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let column = ColumnDef {
            name: if title.contains("age") {
                "age"
            } else {
                "nickname"
            }
            .into(),
            r#type: if title.contains("age") {
                ColumnType::Simple(SimpleColumnType::Integer)
            } else {
                ColumnType::Simple(SimpleColumnType::Text)
            },
            nullable: !title.contains("backfill"),
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let fill_with = if title.contains("backfill") {
            Some("0")
        } else {
            None
        };
        let result = build_add_column(&backend, "users", &column, fill_with).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("add_column_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }
}
