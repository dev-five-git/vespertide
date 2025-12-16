use super::types::{BuiltQuery, RawSql};

pub fn build_raw_sql(sql: String) -> BuiltQuery {
    BuiltQuery::Raw(RawSql::uniform(sql))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case::raw_sql_action_postgres(
        "raw_sql_action_postgres",
        DatabaseBackend::Postgres,
        &["SELECT 1"]
    )]
    #[case::raw_sql_action_mysql(
        "raw_sql_action_mysql",
        DatabaseBackend::MySql,
        &["SELECT 1"]
    )]
    #[case::raw_sql_action_sqlite(
        "raw_sql_action_sqlite",
        DatabaseBackend::Sqlite,
        &["SELECT 1"]
    )]
    fn test_raw_sql(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_raw_sql("SELECT 1".into());
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("raw_sql_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
