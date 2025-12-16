use sea_query::{Alias, Table};

use super::types::{BuiltQuery, DatabaseBackend};

pub fn build_rename_table(from: &str, to: &str) -> BuiltQuery {
    let stmt = Table::rename()
        .table(Alias::new(from), Alias::new(to))
        .to_owned();
    BuiltQuery::RenameTable(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case::rename_table_action_postgres(
        "rename_table_action_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" RENAME TO \"accounts\""]
    )]
    #[case::rename_table_action_mysql(
        "rename_table_action_mysql",
        DatabaseBackend::MySql,
        &["RENAME TABLE `users` TO `accounts`"]
    )]
    #[case::rename_table_action_sqlite(
        "rename_table_action_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" RENAME TO \"accounts\""]
    )]
    fn test_rename_table(#[case] title: &str, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {
        let result = build_rename_table("users", "accounts");
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("rename_table_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
