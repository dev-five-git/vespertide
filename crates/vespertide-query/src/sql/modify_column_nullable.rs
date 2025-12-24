use sea_query::{Alias, Query, Table};

use vespertide_core::{ColumnDef, TableDef};

use super::create_table::build_create_table_for_backend;
use super::helpers::{build_sea_column_def_with_table, normalize_fill_with};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for changing column nullability.
/// For nullable -> non-nullable transitions, fill_with should be provided to update NULL values.
pub fn build_modify_column_nullable(
    backend: &DatabaseBackend,
    table: &str,
    column: &str,
    nullable: bool,
    fill_with: Option<&str>,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = Vec::new();

    // If changing to NOT NULL, first update existing NULL values if fill_with is provided
    if !nullable && let Some(fill_value) = normalize_fill_with(fill_with) {
        let update_sql = match backend {
            DatabaseBackend::Postgres | DatabaseBackend::Sqlite => format!(
                "UPDATE \"{}\" SET \"{}\" = {} WHERE \"{}\" IS NULL",
                table, column, fill_value, column
            ),
            DatabaseBackend::MySql => format!(
                "UPDATE `{}` SET `{}` = {} WHERE `{}` IS NULL",
                table, column, fill_value, column
            ),
        };
        queries.push(BuiltQuery::Raw(RawSql::uniform(update_sql)));
    }

    // Generate ALTER TABLE statement based on backend
    match backend {
        DatabaseBackend::Postgres => {
            let alter_sql = if nullable {
                format!(
                    "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" DROP NOT NULL",
                    table, column
                )
            } else {
                format!(
                    "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" SET NOT NULL",
                    table, column
                )
            };
            queries.push(BuiltQuery::Raw(RawSql::uniform(alter_sql)));
        }
        DatabaseBackend::MySql => {
            // MySQL requires the full column definition in MODIFY COLUMN
            // We need to get the column type from current schema
            let table_def = current_schema
                .iter()
                .find(|t| t.name == table)
                .ok_or_else(|| {
                    QueryError::Other(format!(
                        "Table '{}' not found in current schema. MySQL requires current schema information to modify column nullability.",
                        table
                    ))
                })?;

            let column_def = table_def
                .columns
                .iter()
                .find(|c| c.name == column)
                .ok_or_else(|| {
                    QueryError::Other(format!(
                        "Column '{}' not found in table '{}'. MySQL requires column information to modify nullability.",
                        column, table
                    ))
                })?;

            // Create a modified column def with the new nullability
            let modified_col_def = ColumnDef {
                nullable,
                ..column_def.clone()
            };

            // Build sea-query ColumnDef with all properties (type, nullable, default)
            let sea_col = build_sea_column_def_with_table(backend, table, &modified_col_def);

            let stmt = Table::alter()
                .table(Alias::new(table))
                .modify_column(sea_col)
                .to_owned();
            queries.push(BuiltQuery::AlterTable(Box::new(stmt)));
        }
        DatabaseBackend::Sqlite => {
            // SQLite doesn't support ALTER COLUMN for nullability changes
            // Use temporary table approach
            let table_def = current_schema
                .iter()
                .find(|t| t.name == table)
                .ok_or_else(|| {
                    QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to modify column nullability.",
                        table
                    ))
                })?;

            // Create modified columns with the new nullability
            let mut new_columns = table_def.columns.clone();
            if let Some(col) = new_columns.iter_mut().find(|c| c.name == column) {
                col.nullable = nullable;
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
            columns,
            constraints,
        }
    }

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

        let result =
            build_modify_column_nullable(&backend, "users", "email", nullable, fill_with, &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        let suffix = format!(
            "{}_{}_users{}",
            match backend {
                DatabaseBackend::Postgres => "postgres",
                DatabaseBackend::MySql => "mysql",
                DatabaseBackend::Sqlite => "sqlite",
            },
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

        let result = build_modify_column_nullable(&backend, "users", "email", false, None, &[]);
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

        let result = build_modify_column_nullable(&backend, "users", "email", false, None, &schema);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Column 'email' not found"));
    }

    /// Test with index - should recreate index after table rebuild (SQLite)
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

        let result = build_modify_column_nullable(&backend, "users", "email", false, None, &schema);
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
            assert!(sql.contains("idx_email"));
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

    /// Test with default value - should preserve default in MODIFY COLUMN (MySQL)
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

        let result = build_modify_column_nullable(&backend, "users", "email", false, None, &schema);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // MySQL and SQLite should include DEFAULT clause
        if backend == DatabaseBackend::MySql || backend == DatabaseBackend::Sqlite {
            assert!(sql.contains("DEFAULT"));
        }

        let suffix = format!(
            "{}_with_default",
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
