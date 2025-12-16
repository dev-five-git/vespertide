use sea_query::{Alias, Table};

use super::types::BuiltQuery;

pub fn build_delete_column(table: &str, column: &str) -> BuiltQuery {
    let stmt = Table::alter()
        .table(Alias::new(table))
        .drop_column(Alias::new(column))
        .to_owned();
    BuiltQuery::AlterTable(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case::delete_column_postgres(
        "delete_column_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    #[case::delete_column_mysql(
        "delete_column_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` DROP COLUMN `email`"]
    )]
    #[case::delete_column_sqlite(
        "delete_column_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    fn test_delete_column(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_delete_column("users", "email");
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("delete_column_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
