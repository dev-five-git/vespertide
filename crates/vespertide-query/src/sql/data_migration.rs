use vespertide_core::DataMigrationSql;

use super::types::{BuiltQuery, RawSql};

/// Emit a `DataMigration` action's SQL **verbatim**, per backend.
///
/// The statement is passed through untouched: no case folding, no cast
/// rewriting, no reformatting. Every backend-normalising helper in this crate
/// (`convert_default_for_backend`, `normalize_fill_with`, …) is deliberately
/// bypassed — the user wrote executable SQL and gets exactly that SQL back.
pub fn build_data_migration(sql: &DataMigrationSql) -> Vec<BuiltQuery> {
    vec![BuiltQuery::Raw(RawSql::per_backend(
        sql.postgres().to_string(),
        sql.mysql().to_string(),
        sql.sqlite().to_string(),
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    /// SQL deliberately loaded with everything the other emitters rewrite:
    /// mixed-case keywords, a `::` cast, a quoted identifier, a string
    /// literal, and multi-line formatting.
    const HOSTILE_SQL: &str =
        "UpDaTe \"User\"\n  SET meta = '{\"a\": 1}'::jsonb, n = N + 1\n  WHERE Kind = 'Legacy';";

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn uniform_sql_is_emitted_byte_for_byte(#[case] backend: DatabaseBackend) {
        let queries = build_data_migration(&DataMigrationSql::Uniform(HOSTILE_SQL.to_string()));

        assert_eq!(queries.len(), 1);
        assert_eq!(
            queries[0].build(backend),
            HOSTILE_SQL,
            "data_migration SQL must survive emission unchanged"
        );

        with_settings!({ snapshot_suffix => format!("data_migration_uniform_{backend:?}") }, {
            assert_snapshot!(queries[0].build(backend));
        });
    }

    #[rstest]
    #[case::postgres(
        DatabaseBackend::Postgres,
        "UPDATE t SET j = jsonb_build_object('ko', c)"
    )]
    #[case::mysql(DatabaseBackend::MySql, "UPDATE t SET j = JSON_OBJECT('ko', c)")]
    #[case::sqlite(DatabaseBackend::Sqlite, "UPDATE t SET j = json_object('ko', c)")]
    fn per_backend_sql_selects_the_matching_statement(
        #[case] backend: DatabaseBackend,
        #[case] expected: &str,
    ) {
        let queries = build_data_migration(&DataMigrationSql::PerBackend {
            postgres: "UPDATE t SET j = jsonb_build_object('ko', c)".to_string(),
            mysql: "UPDATE t SET j = JSON_OBJECT('ko', c)".to_string(),
            sqlite: "UPDATE t SET j = json_object('ko', c)".to_string(),
        });

        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].build(backend), expected);

        with_settings!({ snapshot_suffix => format!("data_migration_per_backend_{backend:?}") }, {
            assert_snapshot!(queries[0].build(backend));
        });
    }
}
