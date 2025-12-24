use sea_query::Alias;

use vespertide_core::TableDef;

use super::helpers::build_sea_column_def_with_table;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for changing column comment.
/// Note: SQLite does not support column comments natively.
pub fn build_modify_column_comment(
    backend: &DatabaseBackend,
    table: &str,
    column: &str,
    new_comment: Option<&str>,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = Vec::new();

    match backend {
        DatabaseBackend::Postgres => {
            let comment_sql = if let Some(comment) = new_comment {
                // Escape single quotes in comment
                let escaped = comment.replace('\'', "''");
                format!(
                    "COMMENT ON COLUMN \"{}\".\"{}\" IS '{}'",
                    table, column, escaped
                )
            } else {
                format!("COMMENT ON COLUMN \"{}\".\"{}\" IS NULL", table, column)
            };
            queries.push(BuiltQuery::Raw(RawSql::uniform(comment_sql)));
        }
        DatabaseBackend::MySql => {
            // MySQL requires the full column definition in MODIFY COLUMN to change comment
            let table_def = current_schema
                .iter()
                .find(|t| t.name == table)
                .ok_or_else(|| {
                    QueryError::Other(format!("Table '{}' not found in current schema.", table))
                })?;

            let column_def = table_def
                .columns
                .iter()
                .find(|c| c.name == column)
                .ok_or_else(|| {
                    QueryError::Other(format!(
                        "Column '{}' not found in table '{}'.",
                        column, table
                    ))
                })?;

            // Build the full column definition with updated comment
            let modified_col_def = vespertide_core::ColumnDef {
                comment: new_comment.map(|s| s.to_string()),
                ..column_def.clone()
            };

            // Build base ALTER TABLE statement using sea-query for type/nullable/default
            let sea_col = build_sea_column_def_with_table(backend, table, &modified_col_def);

            // Build the ALTER TABLE ... MODIFY COLUMN statement
            let stmt = sea_query::Table::alter()
                .table(Alias::new(table))
                .modify_column(sea_col)
                .to_owned();

            // Get the base SQL from sea-query
            let base_sql = super::helpers::build_schema_statement(&stmt, *backend);

            // Add COMMENT clause if needed (sea-query doesn't support COMMENT)
            let final_sql = if let Some(comment) = new_comment {
                let escaped = comment.replace('\'', "''");
                format!("{} COMMENT '{}'", base_sql, escaped)
            } else {
                base_sql
            };

            queries.push(BuiltQuery::Raw(RawSql::uniform(final_sql)));
        }
        DatabaseBackend::Sqlite => {
            // SQLite doesn't support column comments
            // We could store the comment in a separate table or just ignore it
            // For now, we'll skip this operation for SQLite since it doesn't affect the schema
            // Just update the internal schema representation (handled by apply.rs)
        }
    }

    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};

    fn col(name: &str, ty: ColumnType, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table_def(
        name: &str,
        columns: Vec<ColumnDef>,
        constraints: Vec<TableConstraint>,
    ) -> TableDef {
        TableDef {
            name: name.to_string(),
            columns,
            constraints,
        }
    }

    #[rstest]
    #[case::postgres_set_comment(DatabaseBackend::Postgres, Some("User email address"))]
    #[case::postgres_drop_comment(DatabaseBackend::Postgres, None)]
    #[case::mysql_set_comment(DatabaseBackend::MySql, Some("User email address"))]
    #[case::mysql_drop_comment(DatabaseBackend::MySql, None)]
    #[case::sqlite_set_comment(DatabaseBackend::Sqlite, Some("User email address"))]
    #[case::sqlite_drop_comment(DatabaseBackend::Sqlite, None)]
    fn test_build_modify_column_comment(
        #[case] backend: DatabaseBackend,
        #[case] new_comment: Option<&str>,
    ) {
        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![],
        )];

        let result = build_modify_column_comment(&backend, "users", "email", new_comment, &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_{}_users",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            },
            if new_comment.is_some() {
                "set_comment"
            } else {
                "drop_comment"
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test comment with quotes escaping
    #[rstest]
    #[case::postgres_comment_with_quotes(DatabaseBackend::Postgres)]
    #[case::mysql_comment_with_quotes(DatabaseBackend::MySql)]
    #[case::sqlite_comment_with_quotes(DatabaseBackend::Sqlite)]
    fn test_comment_with_quotes(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "users",
            vec![col(
                "email",
                ColumnType::Simple(SimpleColumnType::Text),
                true,
            )],
            vec![],
        )];

        let result = build_modify_column_comment(
            &backend,
            "users",
            "email",
            Some("User's email address"),
            &schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Postgres and MySQL should escape quotes, SQLite returns empty
        if backend != DatabaseBackend::Sqlite {
            assert!(
                sql.contains("User''s email address"),
                "Should escape single quotes"
            );
        }

        let suffix = format!(
            "{}_comment_with_quotes",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
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
        // Postgres and SQLite don't need schema lookup, so skip this test for them
        if backend == DatabaseBackend::Postgres || backend == DatabaseBackend::Sqlite {
            return;
        }

        let result = build_modify_column_comment(&backend, "users", "email", Some("comment"), &[]);
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
        // Postgres and SQLite don't need schema lookup, so skip this test for them
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

        let result =
            build_modify_column_comment(&backend, "users", "email", Some("comment"), &schema);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Column 'email' not found"));
    }

    /// Test with long comment
    #[rstest]
    #[case::postgres_long_comment(DatabaseBackend::Postgres)]
    #[case::mysql_long_comment(DatabaseBackend::MySql)]
    #[case::sqlite_long_comment(DatabaseBackend::Sqlite)]
    fn test_long_comment(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "users",
            vec![col("bio", ColumnType::Simple(SimpleColumnType::Text), true)],
            vec![],
        )];

        let long_comment = "This is a very long comment that describes the bio field in great detail. It contains multiple sentences and provides thorough documentation for this column.";

        let result =
            build_modify_column_comment(&backend, "users", "bio", Some(long_comment), &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_long_comment",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test preserves column properties when modifying comment
    #[rstest]
    #[case::postgres_preserves_properties(DatabaseBackend::Postgres)]
    #[case::mysql_preserves_properties(DatabaseBackend::MySql)]
    #[case::sqlite_preserves_properties(DatabaseBackend::Sqlite)]
    fn test_preserves_column_properties(#[case] backend: DatabaseBackend) {
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

        let result = build_modify_column_comment(
            &backend,
            "users",
            "email",
            Some("User email address"),
            &schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // MySQL should preserve the default value in the MODIFY COLUMN statement
        if backend == DatabaseBackend::MySql {
            assert!(sql.contains("DEFAULT"), "Should preserve DEFAULT clause");
        }

        let suffix = format!(
            "{}_preserves_properties",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test changing comment from one value to another
    #[rstest]
    #[case::postgres_change_comment(DatabaseBackend::Postgres)]
    #[case::mysql_change_comment(DatabaseBackend::MySql)]
    #[case::sqlite_change_comment(DatabaseBackend::Sqlite)]
    fn test_change_comment(#[case] backend: DatabaseBackend) {
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text), true);
        email_col.comment = Some("Old comment".into());

        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                email_col,
            ],
            vec![],
        )];

        let result =
            build_modify_column_comment(&backend, "users", "email", Some("New comment"), &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_change_comment",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test dropping existing comment
    #[rstest]
    #[case::postgres_drop_existing_comment(DatabaseBackend::Postgres)]
    #[case::mysql_drop_existing_comment(DatabaseBackend::MySql)]
    #[case::sqlite_drop_existing_comment(DatabaseBackend::Sqlite)]
    fn test_drop_existing_comment(#[case] backend: DatabaseBackend) {
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text), true);
        email_col.comment = Some("Existing comment".into());

        let schema = vec![table_def(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                email_col,
            ],
            vec![],
        )];

        let result = build_modify_column_comment(
            &backend, "users", "email", None, // Drop comment
            &schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_drop_existing_comment",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test with different column types
    #[rstest]
    #[case::postgres_integer_column(
        DatabaseBackend::Postgres,
        SimpleColumnType::Integer,
        "Auto-increment ID"
    )]
    #[case::mysql_integer_column(
        DatabaseBackend::MySql,
        SimpleColumnType::Integer,
        "Auto-increment ID"
    )]
    #[case::sqlite_integer_column(
        DatabaseBackend::Sqlite,
        SimpleColumnType::Integer,
        "Auto-increment ID"
    )]
    #[case::postgres_boolean_column(
        DatabaseBackend::Postgres,
        SimpleColumnType::Boolean,
        "Is user active"
    )]
    #[case::mysql_boolean_column(
        DatabaseBackend::MySql,
        SimpleColumnType::Boolean,
        "Is user active"
    )]
    #[case::sqlite_boolean_column(
        DatabaseBackend::Sqlite,
        SimpleColumnType::Boolean,
        "Is user active"
    )]
    #[case::postgres_timestamp_column(
        DatabaseBackend::Postgres,
        SimpleColumnType::Timestamp,
        "Creation timestamp"
    )]
    #[case::mysql_timestamp_column(
        DatabaseBackend::MySql,
        SimpleColumnType::Timestamp,
        "Creation timestamp"
    )]
    #[case::sqlite_timestamp_column(
        DatabaseBackend::Sqlite,
        SimpleColumnType::Timestamp,
        "Creation timestamp"
    )]
    fn test_comment_on_different_types(
        #[case] backend: DatabaseBackend,
        #[case] column_type: SimpleColumnType,
        #[case] comment: &str,
    ) {
        let schema = vec![table_def(
            "data",
            vec![col("field", ColumnType::Simple(column_type.clone()), false)],
            vec![],
        )];

        let result = build_modify_column_comment(&backend, "data", "field", Some(comment), &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let type_name = format!("{:?}", column_type).to_lowercase();
        let suffix = format!(
            "{}_{}_comment",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            },
            type_name
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }

    /// Test with NOT NULL column
    #[rstest]
    #[case::postgres_not_null_column(DatabaseBackend::Postgres)]
    #[case::mysql_not_null_column(DatabaseBackend::MySql)]
    #[case::sqlite_not_null_column(DatabaseBackend::Sqlite)]
    fn test_comment_on_not_null_column(#[case] backend: DatabaseBackend) {
        let schema = vec![table_def(
            "users",
            vec![col(
                "username",
                ColumnType::Simple(SimpleColumnType::Text),
                false,
            )],
            vec![],
        )];

        let result = build_modify_column_comment(
            &backend,
            "users",
            "username",
            Some("Required username"),
            &schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_not_null_column",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            }
        );

        with_settings!({ snapshot_suffix => suffix }, {
            assert_snapshot!(sql);
        });
    }
}
