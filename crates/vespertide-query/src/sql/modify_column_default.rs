use sea_query::{Alias, Query, Table};

use vespertide_core::{ColumnDef, TableDef};

use super::create_table::build_create_table_for_backend;
use super::helpers::{build_sea_column_def_with_table, normalize_enum_default};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for changing column default value.
pub fn build_modify_column_default(
    backend: &DatabaseBackend,
    table: &str,
    column: &str,
    new_default: Option<&str>,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = Vec::new();

    match backend {
        DatabaseBackend::Postgres => {
            let alter_sql = if let Some(default_value) = new_default {
                // Look up column type to properly quote enum defaults
                let column_type = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .and_then(|t| t.columns.iter().find(|c| c.name == column))
                    .map(|c| &c.r#type);

                let normalized_default = if let Some(col_type) = column_type {
                    normalize_enum_default(col_type, default_value)
                } else {
                    default_value.to_string()
                };

                format!(
                    "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" SET DEFAULT {}",
                    table, column, normalized_default
                )
            } else {
                format!(
                    "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" DROP DEFAULT",
                    table, column
                )
            };
            queries.push(BuiltQuery::Raw(RawSql::uniform(alter_sql)));
        }
        DatabaseBackend::MySql => {
            // MySQL requires the full column definition in ALTER COLUMN
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

            // Create a modified column def with the new default
            let modified_col_def = ColumnDef {
                default: new_default.map(|s| s.into()),
                ..column_def.clone()
            };

            let sea_col = build_sea_column_def_with_table(backend, table, &modified_col_def);

            let stmt = Table::alter()
                .table(Alias::new(table))
                .modify_column(sea_col)
                .to_owned();
            queries.push(BuiltQuery::AlterTable(Box::new(stmt)));
        }
        DatabaseBackend::Sqlite => {
            // SQLite doesn't support ALTER COLUMN for default changes
            // Use temporary table approach
            let table_def = current_schema
                .iter()
                .find(|t| t.name == table)
                .ok_or_else(|| {
                    QueryError::Other(format!("Table '{}' not found in current schema.", table))
                })?;

            // Create modified columns with the new default
            let mut new_columns = table_def.columns.clone();
            if let Some(col) = new_columns.iter_mut().find(|c| c.name == column) {
                col.default = new_default.map(|s| s.into());
            }

            // Generate temporary table name
            let temp_table = format!("{}_temp", table);

            // 1. Create temporary table with modified column
            let create_temp_table = build_create_table_for_backend(
                backend,
                &temp_table,
                &new_columns,
                &table_def.constraints,
            );
            queries.push(BuiltQuery::CreateTable(Box::new(create_temp_table)));

            // 2. Copy data (all columns)
            let column_aliases: Vec<Alias> = table_def
                .columns
                .iter()
                .map(|c| Alias::new(&c.name))
                .collect();
            let mut select_query = Query::select();
            for col_alias in &column_aliases {
                select_query = select_query.column(col_alias.clone()).to_owned();
            }
            select_query = select_query.from(Alias::new(table)).to_owned();

            let insert_stmt = Query::insert()
                .into_table(Alias::new(&temp_table))
                .columns(column_aliases.clone())
                .select_from(select_query)
                .unwrap()
                .to_owned();
            queries.push(BuiltQuery::Insert(Box::new(insert_stmt)));

            // 3. Drop original table
            let drop_table = Table::drop().table(Alias::new(table)).to_owned();
            queries.push(BuiltQuery::DropTable(Box::new(drop_table)));

            // 4. Rename temporary table to original name
            queries.push(build_rename_table(&temp_table, table));

            // 5. Recreate indexes from Index constraints
            for constraint in &table_def.constraints {
                if let vespertide_core::TableConstraint::Index {
                    name: idx_name,
                    columns: idx_cols,
                } = constraint
                {
                    let index_name =
                        vespertide_naming::build_index_name(table, idx_cols, idx_name.as_deref());
                    let mut idx_stmt = sea_query::Index::create();
                    idx_stmt = idx_stmt.name(&index_name).to_owned();
                    for col_name in idx_cols {
                        idx_stmt = idx_stmt.col(Alias::new(col_name)).to_owned();
                    }
                    idx_stmt = idx_stmt.table(Alias::new(table)).to_owned();
                    queries.push(BuiltQuery::CreateIndex(Box::new(idx_stmt)));
                }
            }
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
            description: None,
            columns,
            constraints,
        }
    }

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

        let result = build_modify_column_default(&backend, "users", "email", new_default, &schema);
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

        let result =
            build_modify_column_default(&backend, "users", "email", Some("'default'"), &[]);
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

        let result =
            build_modify_column_default(&backend, "users", "email", Some("'default'"), &schema);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Column 'email' not found"));
    }

    /// Test Postgres default change when column is not in schema
    /// This covers the fallback path where column_type is None
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
            &DatabaseBackend::Postgres,
            "users",
            "status", // column not in schema
            Some("'active'"),
            &schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<String>>()
            .join("\n");

        // Should still generate valid SQL, using the default value as-is
        assert!(sql.contains("ALTER TABLE \"users\" ALTER COLUMN \"status\" SET DEFAULT 'active'"));
    }

    /// Test with index - should recreate index after table rebuild (SQLite)
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
            &backend,
            "users",
            "email",
            Some("'default@example.com'"),
            &schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // SQLite should recreate the index after table rebuild
        if backend == DatabaseBackend::Sqlite {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_users_email"));
        }

        let suffix = format!(
            "{}_with_index",
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
            &backend,
            "users",
            "email",
            Some("'new@example.com'"),
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
            "{}_change_default",
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

        let result =
            build_modify_column_default(&backend, "products", "quantity", Some("0"), &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_integer_default",
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

        let result =
            build_modify_column_default(&backend, "users", "is_active", Some("true"), &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_boolean_default",
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

    /// Test with function default (e.g., NOW(), CURRENT_TIMESTAMP)
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
            DatabaseBackend::MySql => "CURRENT_TIMESTAMP",
            DatabaseBackend::Sqlite => "CURRENT_TIMESTAMP",
        };

        let result = build_modify_column_default(
            &backend,
            "events",
            "created_at",
            Some(default_value),
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
            "{}_function_default",
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
            &backend, "orders", "status", None, // Drop default
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
            "{}_drop_existing_default",
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
