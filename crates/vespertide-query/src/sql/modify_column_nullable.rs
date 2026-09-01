use vespertide_core::TableDef;

use super::fill_with::convert_fill_with_for_backend;
use super::helpers::{
    build_mysql_modify_column_with, build_pg_alter_column_sql, build_sqlite_modify_column_with,
    normalize_fill_with, quote_ident,
};
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for changing column nullability.
/// For nullable -> non-nullable transitions, `fill_with` should be provided to update NULL values.
#[expect(
    clippy::too_many_arguments,
    reason = "nullability builder needs action fields, fill strategy, backend, and SQLite rebuild context; NullabilityContext is deferred"
)]
pub fn build_modify_column_nullable(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    nullable: bool,
    fill_with: Option<&str>,
    delete_null_rows: bool,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = Vec::new();

    // If delete_null_rows is set, delete rows with NULL values instead of updating
    if !nullable && delete_null_rows {
        let quoted_table = quote_ident(table, backend);
        let quoted_column = quote_ident(column, backend);
        let delete_sql = format!("DELETE FROM {quoted_table} WHERE {quoted_column} IS NULL");
        queries.push(BuiltQuery::Raw(RawSql::uniform(delete_sql)));
    }
    // If changing to NOT NULL, first update existing NULL values if fill_with is provided
    else if !nullable && let Some(fill_value) = normalize_fill_with(fill_with) {
        let fill_value = convert_fill_with_for_backend(fill_value, backend);
        let quoted_table = quote_ident(table, backend);
        let quoted_column = quote_ident(column, backend);
        let update_sql = format!(
            "UPDATE {quoted_table} SET {quoted_column} = {fill_value} WHERE {quoted_column} IS NULL"
        );
        queries.push(BuiltQuery::Raw(RawSql::uniform(update_sql)));
    }

    // Generate ALTER TABLE statement based on backend
    match backend {
        DatabaseBackend::Postgres => {
            let alter_sql = if nullable {
                build_pg_alter_column_sql(table, column, "DROP NOT NULL")
            } else {
                build_pg_alter_column_sql(table, column, "SET NOT NULL")
            };
            queries.push(BuiltQuery::Raw(RawSql::uniform(alter_sql)));
        }
        DatabaseBackend::MySql => {
            // MySQL requires the full column definition in MODIFY COLUMN.
            queries.push(build_mysql_modify_column_with(
                table,
                column,
                current_schema,
                "MySQL requires current schema information to modify column nullability",
                |c| c.nullable = nullable,
            )?);
        }
        DatabaseBackend::Sqlite => {
            // SQLite doesn't support ALTER COLUMN for nullability changes;
            // use the canonical temp-table rebuild with the modified column.
            queries.extend(build_sqlite_modify_column_with(
                table,
                column,
                current_schema,
                pending_constraints,
                "SQLite requires current schema information to modify column nullability",
                |c| c.nullable = nullable,
            )?);
        }
    }

    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{backend_tag, col_n as col, joined_sql, table_def};
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnType, SimpleColumnType, TableConstraint};

    #[rstest]
    #[case::postgres_set_not_null(DatabaseBackend::Postgres, false, None)]
    #[case::postgres_drop_not_null(DatabaseBackend::Postgres, true, None)]
    #[case::postgres_set_not_null_with_fill(DatabaseBackend::Postgres, false, Some("'unknown'"))]
    #[case::mysql_set_not_null(DatabaseBackend::MySql, false, None)]
    #[case::mysql_drop_not_null(DatabaseBackend::MySql, true, None)]
    #[case::mysql_set_not_null_with_fill(DatabaseBackend::MySql, false, Some("'unknown'"))]
    #[case::sqlite_set_not_null(DatabaseBackend::Sqlite, false, None)]
    #[case::sqlite_drop_not_null(DatabaseBackend::Sqlite, true, None)]
    #[case::sqlite_set_not_null_with_fill(DatabaseBackend::Sqlite, false, Some("'unknown'"))]
    fn test_build_modify_column_nullable(
        #[case] backend: DatabaseBackend,
        #[case] nullable: bool,
        #[case] fill_with: Option<&str>,
    ) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    !nullable,
                ),
            ],
            vec![],
        )];

        let result = build_modify_column_nullable(
            backend,
            "users",
            "email",
            nullable,
            fill_with,
            false,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!(
            "{}_{}_users{}",
            backend_tag(backend),
            if nullable { "nullable" } else { "not_null" },
            if fill_with.is_some() {
                "_with_fill"
            } else {
                ""
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test table not found error
    #[rstest]
    #[case::postgres_table_not_found(DatabaseBackend::Postgres)]
    #[case::mysql_table_not_found(DatabaseBackend::MySql)]
    #[case::sqlite_table_not_found(DatabaseBackend::Sqlite)]
    fn test_table_not_found(#[case] backend: DatabaseBackend) {
        // Postgres doesn't need schema lookup for nullability changes
        if backend == DatabaseBackend::Postgres {
            return;
        }

        let result =
            build_modify_column_nullable(backend, "users", "email", false, None, false, &[], &[]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'users' not found"));
    }

    /// Test column not found error
    #[rstest]
    #[case::postgres_column_not_found(DatabaseBackend::Postgres)]
    #[case::mysql_column_not_found(DatabaseBackend::MySql)]
    #[case::sqlite_column_not_found(DatabaseBackend::Sqlite)]
    fn test_column_not_found(#[case] backend: DatabaseBackend) {
        // Postgres doesn't need schema lookup for nullability changes
        // SQLite doesn't validate column existence in modify_column_nullable
        if backend == DatabaseBackend::Postgres || backend == DatabaseBackend::Sqlite {
            return;
        }

        let schema = vec![table_def(
            "users",
            vec![col(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            vec![],
        )];

        let result = build_modify_column_nullable(
            backend,
            "users",
            "email",
            false,
            None,
            false,
            &schema,
            &[],
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Column 'email' not found"));
    }

    /// Test with index - should recreate index after table rebuild (`SQLite`)
    #[rstest]
    #[case::postgres_with_index(DatabaseBackend::Postgres)]
    #[case::mysql_with_index(DatabaseBackend::MySql)]
    #[case::sqlite_with_index(DatabaseBackend::Sqlite)]
    fn test_modify_nullable_with_index(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![TableConstraint::Index {
                name: Some("idx_email".into()),
                columns: vec!["email".into()],
            }],
        )];

        let result = build_modify_column_nullable(
            backend,
            "users",
            "email",
            false,
            None,
            false,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        // SQLite should recreate the index after table rebuild
        if backend == DatabaseBackend::Sqlite {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_email"));
        }

        let suffix = format!("{}_with_index", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test `fill_with` containing `NOW()`.
    ///
    /// `fill_with` is a raw SQL expression slot, so PostgreSQL — the dialect
    /// it is authored in — now emits it verbatim. MySQL and SQLite still get
    /// `CURRENT_TIMESTAMP` because `NOW()` is not a SQLite function; that
    /// rewrite is safe only because the spelling is matched against the whole
    /// value, never a fragment of a larger expression.
    #[rstest]
    #[case::postgres_fill_now(DatabaseBackend::Postgres)]
    #[case::mysql_fill_now(DatabaseBackend::MySql)]
    #[case::sqlite_fill_now(DatabaseBackend::Sqlite)]
    fn test_fill_with_now_converted_to_current_timestamp(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "paid_at",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                    true,
                ),
            ],
            vec![],
        )];

        let result = build_modify_column_nullable(
            backend,
            "orders",
            "paid_at",
            false,
            Some("NOW()"),
            false,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        if backend == DatabaseBackend::Postgres {
            assert!(
                sql.contains("NOW()"),
                "PostgreSQL must emit fill_with verbatim, got: {sql}"
            );
        } else {
            assert!(
                !sql.contains("NOW()"),
                "SQL should not contain NOW(), got: {sql}"
            );
            assert!(
                sql.contains("CURRENT_TIMESTAMP"),
                "SQL should contain CURRENT_TIMESTAMP, got: {sql}"
            );
        }

        let suffix = format!("{}_fill_now", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test with default value - should preserve default in MODIFY COLUMN (`MySQL`)
    #[rstest]
    #[case::postgres_with_default(DatabaseBackend::Postgres)]
    #[case::mysql_with_default(DatabaseBackend::MySql)]
    #[case::sqlite_with_default(DatabaseBackend::Sqlite)]
    fn test_with_default_value(#[case] backend: DatabaseBackend) {
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text), true);
        email_col.default = Some("'default@example.com'".into());

        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                email_col,
            ],
            vec![],
        )];

        let result = build_modify_column_nullable(
            backend,
            "users",
            "email",
            false,
            None,
            false,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        // MySQL and SQLite should include DEFAULT clause
        if backend == DatabaseBackend::MySql || backend == DatabaseBackend::Sqlite {
            assert!(sql.contains("DEFAULT"));
        }

        let suffix = format!("{}_with_default", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test `delete_null_rows` generates DELETE instead of UPDATE
    #[rstest]
    #[case::postgres_delete_null_rows(DatabaseBackend::Postgres)]
    #[case::mysql_delete_null_rows(DatabaseBackend::MySql)]
    #[case::sqlite_delete_null_rows(DatabaseBackend::Sqlite)]
    fn test_delete_null_rows(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "user_id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    true,
                ),
            ],
            vec![],
        )];

        let result = build_modify_column_nullable(
            backend,
            "orders",
            "user_id",
            false,
            None,
            true,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        assert!(
            sql.contains("DELETE FROM"),
            "Expected DELETE FROM in SQL, got: {sql}"
        );
        assert!(
            sql.contains("IS NULL"),
            "Expected IS NULL in SQL, got: {sql}"
        );
        assert!(
            !sql.contains("UPDATE"),
            "Should NOT contain UPDATE, got: {sql}"
        );

        let suffix = format!("{}_delete_null_rows", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test `delete_null_rows=true` with nullable=true does nothing special
    #[rstest]
    #[case::postgres_delete_null_rows_nullable(DatabaseBackend::Postgres)]
    fn test_delete_null_rows_with_nullable_true(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "user_id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                ),
            ],
            vec![],
        )];

        let result = build_modify_column_nullable(
            backend,
            "orders",
            "user_id",
            true,
            None,
            true,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        assert!(
            !sql.contains("DELETE FROM"),
            "Should NOT contain DELETE when nullable=true, got: {sql}"
        );
    }
}
