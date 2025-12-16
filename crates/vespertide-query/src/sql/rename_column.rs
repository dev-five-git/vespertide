use sea_query::{Alias, Table};

use super::types::{BuiltQuery, DatabaseBackend};

pub fn build_rename_column(table: &str, from: &str, to: &str) -> BuiltQuery {
    let stmt = Table::alter()
        .table(Alias::new(table))
        .rename_column(Alias::new(from), Alias::new(to))
        .to_owned();
    BuiltQuery::AlterTable(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case::rename_column_postgres(
        "rename_column_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" RENAME COLUMN \"email\" TO \"contact_email\""]
    )]
    #[case::rename_column_mysql(
        "rename_column_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` RENAME COLUMN `email` TO `contact_email`"]
    )]
    #[case::rename_column_sqlite(
        "rename_column_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" RENAME COLUMN \"email\" TO \"contact_email\""]
    )]
    fn test_rename_column(#[case] title: &str, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {
        let result = build_rename_column("users", "email", "contact_email");
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("rename_column_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
