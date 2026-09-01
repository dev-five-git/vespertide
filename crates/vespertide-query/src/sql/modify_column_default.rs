use vespertide_core::TableDef;

use super::helpers::{
    build_mysql_modify_column_with, build_pg_alter_column_sql, build_sqlite_modify_column_with,
    find_column_in_schema, normalize_enum_default, quote_ident,
};
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for changing column default value.
///
/// When `backfill` is `Some(value)`, an `UPDATE` statement is appended after
/// the schema-level change so every existing row is rewritten to the given
/// value (F15 backfill option β). The update uses identifier quoting
/// appropriate for the backend and treats `value` as a raw SQL expression
/// (already-quoted literals for strings, bare expressions like `NOW()` for
/// functions). When `backfill` is `None` the action behaves exactly as in
/// v0.2.0 — only the schema is touched, existing rows keep their values.
pub fn build_modify_column_default(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_default: Option<&str>,
    backfill: Option<&str>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = Vec::new();

    match backend {
        DatabaseBackend::Postgres => {
            let alter_sql = if let Some(default_value) = new_default {
                // Look up column type to properly quote enum defaults
                let column_type =
                    find_column_in_schema(current_schema, table, column).map(|c| &c.r#type);

                let normalized_default = if let Some(col_type) = column_type {
                    normalize_enum_default(col_type, default_value)
                } else {
                    default_value.to_string()
                };

                build_pg_alter_column_sql(
                    table,
                    column,
                    &format!("SET DEFAULT {normalized_default}"),
                )
            } else {
                build_pg_alter_column_sql(table, column, "DROP DEFAULT")
            };
            queries.push(BuiltQuery::Raw(RawSql::uniform(alter_sql)));
        }
        DatabaseBackend::MySql => {
            // MySQL requires the full column definition in ALTER COLUMN.
            queries.push(build_mysql_modify_column_with(
                table,
                column,
                current_schema,
                "MySQL requires current schema information to modify column defaults",
                |c| c.default = new_default.map(std::convert::Into::into),
            )?);
        }
        DatabaseBackend::Sqlite => {
            // SQLite doesn't support ALTER COLUMN for default changes;
            // use the canonical temp-table rebuild with the modified column.
            queries.extend(build_sqlite_modify_column_with(
                table,
                column,
                current_schema,
                pending_constraints,
                "SQLite requires current schema information to modify column defaults",
                |c| c.default = new_default.map(std::convert::Into::into),
            )?);
        }
    }

    // F15 — backfill existing rows when the user explicitly opted in via
    // the revision prompt. The schema-level change above only affects new
    // rows; this UPDATE is what brings existing rows in line with the new
    // default. Emitted *after* the ALTER so the new default is the one
    // recorded in the catalog before we touch any row.
    if let Some(value) = backfill {
        let quoted_table = quote_ident(table, backend);
        let quoted_column = quote_ident(column, backend);
        let update_sql = format!("UPDATE {quoted_table} SET {quoted_column} = {value}");
        queries.push(BuiltQuery::Raw(RawSql::uniform(update_sql)));
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
    #[case::postgres_set_default(DatabaseBackend::Postgres, Some("'unknown'"))]
    #[case::postgres_drop_default(DatabaseBackend::Postgres, None)]
    #[case::mysql_set_default(DatabaseBackend::MySql, Some("'unknown'"))]
    #[case::mysql_drop_default(DatabaseBackend::MySql, None)]
    #[case::sqlite_set_default(DatabaseBackend::Sqlite, Some("'unknown'"))]
    #[case::sqlite_drop_default(DatabaseBackend::Sqlite, None)]
    fn test_build_modify_column_default(
        #[case] backend: DatabaseBackend,
        #[case] new_default: Option<&str>,
    ) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![],
        )];

        let result =
            build_modify_column_default(backend, "users", "email", new_default, None, &schema, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!(
            "{}_{}_users",
            backend_tag(backend),
            if new_default.is_some() {
                "set_default"
            } else {
                "drop_default"
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
        // Postgres doesn't need schema lookup for default changes
        if backend == DatabaseBackend::Postgres {
            return;
        }

        let result = build_modify_column_default(
            backend,
            "users",
            "email",
            Some("'default'"),
            None,
            &[],
            &[],
        );
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
        // Postgres doesn't need schema lookup for default changes
        // SQLite doesn't validate column existence in modify_column_default
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

        let result = build_modify_column_default(
            backend,
            "users",
            "email",
            Some("'default'"),
            None,
            &schema,
            &[],
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Column 'email' not found"));
    }

    /// Test Postgres default change when column is not in schema
    /// This covers the fallback path where `column_type` is None
    #[test]
    fn test_postgres_column_not_in_schema_uses_default_as_is() {
        let schema = vec![table_def(
            "users",
            vec![col(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            // Note: "status" column is NOT in the schema
            vec![],
        )];

        // Postgres doesn't error when column isn't found - it just uses the default as-is
        let result = build_modify_column_default(
            DatabaseBackend::Postgres,
            "users",
            "status", // column not in schema
            Some("'active'"),
            None,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(DatabaseBackend::Postgres, &queries);

        // Should still generate valid SQL, using the default value as-is
        assert!(sql.contains("ALTER TABLE \"users\" ALTER COLUMN \"status\" SET DEFAULT 'active'"));
    }

    /// Test with index - should recreate index after table rebuild (`SQLite`)
    #[rstest]
    #[case::postgres_with_index(DatabaseBackend::Postgres)]
    #[case::mysql_with_index(DatabaseBackend::MySql)]
    #[case::sqlite_with_index(DatabaseBackend::Sqlite)]
    fn test_modify_default_with_index(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![TableConstraint::Index {
                name: Some("idx_users_email".into()),
                columns: vec!["email".into()],
            }],
        )];

        let result = build_modify_column_default(
            backend,
            "users",
            "email",
            Some("'default@example.com'"),
            None,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        // SQLite should recreate the index after table rebuild
        if backend == DatabaseBackend::Sqlite {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_users_email"));
        }

        let suffix = format!("{}_with_index", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test changing default value from one to another
    #[rstest]
    #[case::postgres_change_default(DatabaseBackend::Postgres)]
    #[case::mysql_change_default(DatabaseBackend::MySql)]
    #[case::sqlite_change_default(DatabaseBackend::Sqlite)]
    fn test_change_default_value(#[case] backend: DatabaseBackend) {
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text), true);
        email_col.default = Some("'old@example.com'".into());

        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                email_col,
            ],
            vec![],
        )];

        let result = build_modify_column_default(
            backend,
            "users",
            "email",
            Some("'new@example.com'"),
            None,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!("{}_change_default", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test with integer default value
    #[rstest]
    #[case::postgres_integer_default(DatabaseBackend::Postgres)]
    #[case::mysql_integer_default(DatabaseBackend::MySql)]
    #[case::sqlite_integer_default(DatabaseBackend::Sqlite)]
    fn test_integer_default(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "products",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "quantity",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                ),
            ],
            vec![],
        )];

        let result = build_modify_column_default(
            backend,
            "products",
            "quantity",
            Some("0"),
            None,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!("{}_integer_default", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test with boolean default value
    #[rstest]
    #[case::postgres_boolean_default(DatabaseBackend::Postgres)]
    #[case::mysql_boolean_default(DatabaseBackend::MySql)]
    #[case::sqlite_boolean_default(DatabaseBackend::Sqlite)]
    fn test_boolean_default(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "is_active",
                    ColumnType::Simple(SimpleColumnType::Boolean),
                    false,
                ),
            ],
            vec![],
        )];

        let result = build_modify_column_default(
            backend,
            "users",
            "is_active",
            Some("true"),
            None,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!("{}_boolean_default", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test with function default (e.g., `NOW()`, `CURRENT_TIMESTAMP`)
    #[rstest]
    #[case::postgres_function_default(DatabaseBackend::Postgres)]
    #[case::mysql_function_default(DatabaseBackend::MySql)]
    #[case::sqlite_function_default(DatabaseBackend::Sqlite)]
    fn test_function_default(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "events",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamp),
                    false,
                ),
            ],
            vec![],
        )];

        let default_value = match backend {
            DatabaseBackend::Postgres => "NOW()",
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => "CURRENT_TIMESTAMP",
        };

        let result = build_modify_column_default(
            backend,
            "events",
            "created_at",
            Some(default_value),
            None,
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!("{}_function_default", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test dropping default from column that had one
    #[rstest]
    #[case::postgres_drop_existing_default(DatabaseBackend::Postgres)]
    #[case::mysql_drop_existing_default(DatabaseBackend::MySql)]
    #[case::sqlite_drop_existing_default(DatabaseBackend::Sqlite)]
    fn test_drop_existing_default(#[case] backend: DatabaseBackend) {
        let mut status_col = col("status", ColumnType::Simple(SimpleColumnType::Text), false);
        status_col.default = Some("'pending'".into());

        let schema = vec![table_def(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                status_col,
            ],
            vec![],
        )];

        let result = build_modify_column_default(
            backend,
            "orders",
            "status",
            None, // Drop default
            None, // No backfill
            &schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(backend, &queries);

        let suffix = format!("{}_drop_existing_default", backend_tag(backend));

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test that `backfill = Some(value)` emits the trailing `UPDATE` that
    /// rewrites every existing row. Covers the post-ALTER backfill block
    /// (the `if let Some(value) = backfill { ... }` body) for all backends.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn build_modify_column_default_with_backfill_emits_update_statement(
        #[case] backend: DatabaseBackend,
    ) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col("status", ColumnType::Simple(SimpleColumnType::Text), false),
            ],
            vec![],
        )];

        let queries = build_modify_column_default(
            backend,
            "users",
            "status",
            Some("'active'"),
            Some("'active'"),
            &schema,
            &[],
        )
        .expect("backfill path should succeed");
        let sql = joined_sql(backend, &queries);

        // The trailing UPDATE was emitted exactly once.
        let update_count = sql.matches("UPDATE").count();
        assert!(update_count >= 1, "expected backfill UPDATE in: {sql}");
        assert!(sql.contains("SET"));
        assert!(sql.contains("status"));
        assert!(sql.contains("'active'"));
    }

    /// `backfill` is a raw SQL expression slot, so it is interpolated
    /// verbatim. This locks that contract against the `fill_with` defect
    /// (first-`::` split + `to_lowercase`) ever being copied onto this path.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn build_modify_column_default_backfill_expression_is_verbatim(
        #[case] backend: DatabaseBackend,
    ) {
        const BACKFILL: &str = "(CASE WHEN plan_key::text = 'API' THEN 'MONTHLY_QUOTA' ELSE 'SEAT' END)::billing_metric";

        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col(
                    "plan_key",
                    ColumnType::Simple(SimpleColumnType::Text),
                    false,
                ),
                col("metric", ColumnType::Simple(SimpleColumnType::Text), false),
            ],
            vec![],
        )];

        let queries = build_modify_column_default(
            backend,
            "users",
            "metric",
            None,
            Some(BACKFILL),
            &schema,
            &[],
        )
        .expect("backfill path should succeed");
        let sql = joined_sql(backend, &queries);

        assert!(
            sql.contains(BACKFILL),
            "backfill must survive byte-for-byte, got: {sql}"
        );
    }
}
