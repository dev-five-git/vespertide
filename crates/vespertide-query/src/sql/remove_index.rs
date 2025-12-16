use sea_query::{Alias, Index};

use super::types::BuiltQuery;

pub fn build_remove_index(table: &str, name: &str) -> BuiltQuery {
    let stmt = Index::drop()
        .name(name)
        // MySQL requires ON <table>; other backends accept this form
        .table(Alias::new(table))
        .to_owned();
    BuiltQuery::DropIndex(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case::remove_index_postgres(
        "remove_index_postgres",
        DatabaseBackend::Postgres,
        &["DROP INDEX \"idx_email\""]
    )]
    #[case::remove_index_mysql(
        "remove_index_mysql",
        DatabaseBackend::MySql,
        &["DROP INDEX `idx_email` ON `users`"]
    )]
    #[case::remove_index_sqlite(
        "remove_index_sqlite",
        DatabaseBackend::Sqlite,
        &["\"idx_email\""]
    )]
    fn test_remove_index(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_remove_index("users", "idx_email");
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("remove_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
