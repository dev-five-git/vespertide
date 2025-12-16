use sea_query::{Alias, Index};

use vespertide_core::IndexDef;

use super::types::{BuiltQuery, DatabaseBackend};

pub fn build_add_index(table: &str, index: &IndexDef) -> BuiltQuery {
    let mut stmt = Index::create()
        .name(&index.name)
        .table(Alias::new(table))
        .to_owned();

    for col in &index.columns {
        stmt = stmt.col(Alias::new(col)).to_owned();
    }

    if index.unique {
        stmt = stmt.unique().to_owned();
    }

    BuiltQuery::CreateIndex(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::IndexDef;

    #[rstest]
    #[case::add_index_postgres(
        "add_index_postgres",
        DatabaseBackend::Postgres,
        &["CREATE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_index_mysql(
        "add_index_mysql",
        DatabaseBackend::MySql,
        &["CREATE INDEX `idx_email` ON `users` (`email`)"]
    )]
    #[case::add_index_sqlite(
        "add_index_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_unique_index_postgres(
        "add_unique_index_postgres",
        DatabaseBackend::Postgres,
        &["CREATE UNIQUE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_unique_index_mysql(
        "add_unique_index_mysql",
        DatabaseBackend::MySql,
        &["CREATE UNIQUE INDEX `idx_email` ON `users` (`email`)"]
    )]
    #[case::add_unique_index_sqlite(
        "add_unique_index_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE UNIQUE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    fn test_add_index(#[case] title: &str, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {
        let index = IndexDef {
            name: "idx_email".into(),
            columns: vec!["email".into()],
            unique: title.contains("unique"),
        };
        let result = build_add_index("users", &index);
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("add_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
