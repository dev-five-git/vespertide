use sea_query::{Alias, Table};

use super::types::BuiltQuery;

pub fn build_delete_table(table: &str) -> BuiltQuery {
    let stmt = Table::drop().table(Alias::new(table)).to_owned();
    BuiltQuery::DropTable(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case::delete_table_postgres(
        "delete_table_postgres",
        DatabaseBackend::Postgres,
        &["DROP TABLE \"users\""]
    )]
    #[case::delete_table_mysql(
        "delete_table_mysql",
        DatabaseBackend::MySql,
        &["DROP TABLE `users`"]
    )]
    #[case::delete_table_sqlite(
        "delete_table_sqlite",
        DatabaseBackend::Sqlite,
        &["DROP TABLE \"users\""]
    )]
    fn test_delete_table(#[case] title: &str, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {
        let result = build_delete_table("users");
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("delete_table_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
