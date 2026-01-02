use sea_query::{Alias, ColumnDef as SeaColumnDef, Query, Table};

use vespertide_core::{ColumnType, ComplexColumnType, TableDef};

use super::create_table::build_create_table_for_backend;
use super::helpers::{apply_column_type_with_table, build_create_enum_type_sql};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

pub fn build_modify_column_type(
    backend: &DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // SQLite does not support direct column type modification, so use temporary table approach
    if *backend == DatabaseBackend::Sqlite {
        // Current schema information is required
        let table_def = current_schema
            .iter()
            .find(|t| t.name == table)
            .ok_or_else(|| QueryError::Other(format!(
                "Table '{}' not found in current schema. SQLite requires current schema information to modify column types.",
                table
            )))?;

        // Create new column definitions with the modified column
        let mut new_columns = table_def.columns.clone();
        let col_index = new_columns
            .iter()
            .position(|c| c.name == column)
            .ok_or_else(|| {
                QueryError::Other(format!(
                    "Column '{}' not found in table '{}'",
                    column, table
                ))
            })?;

        new_columns[col_index].r#type = new_type.clone();

        // Generate temporary table name
        let temp_table = format!("{}_temp", table);

        // 1. Create temporary table with new column types
        let create_temp_table = build_create_table_for_backend(
            backend,
            &temp_table,
            &new_columns,
            &table_def.constraints,
        );
        let create_query = BuiltQuery::CreateTable(Box::new(create_temp_table));

        // 2. Copy data (all columns) - Use INSERT INTO ... SELECT
        let column_aliases: Vec<Alias> = new_columns.iter().map(|c| Alias::new(&c.name)).collect();

        // Build SELECT query
        let mut select_query = Query::select();
        for col_alias in &column_aliases {
            select_query = select_query.column(col_alias.clone()).to_owned();
        }
        select_query = select_query.from(Alias::new(table)).to_owned();

        // Build INSERT query
        let insert_stmt = Query::insert()
            .into_table(Alias::new(&temp_table))
            .columns(column_aliases.clone())
            .select_from(select_query)
            .unwrap()
            .to_owned();

        let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

        // 3. Drop original table
        let drop_table = Table::drop().table(Alias::new(table)).to_owned();
        let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

        // 4. Rename temporary table to original name
        let rename_query = build_rename_table(&temp_table, table);

        // 5. Recreate indexes from Index constraints
        let mut index_queries = Vec::new();
        for constraint in &table_def.constraints {
            if let vespertide_core::TableConstraint::Index { name, columns } = constraint {
                let index_name =
                    vespertide_naming::build_index_name(table, columns, name.as_deref());
                let mut idx_stmt = sea_query::Index::create();
                idx_stmt = idx_stmt.name(&index_name).to_owned();
                for col_name in columns {
                    idx_stmt = idx_stmt.col(Alias::new(col_name)).to_owned();
                }
                idx_stmt = idx_stmt.table(Alias::new(table)).to_owned();
                index_queries.push(BuiltQuery::CreateIndex(Box::new(idx_stmt)));
            }
        }

        let mut queries = vec![create_query, insert_query, drop_query, rename_query];
        queries.extend(index_queries);

        Ok(queries)
    } else {
        // PostgreSQL, MySQL, etc. can use ALTER TABLE directly
        let mut queries = Vec::new();

        // Get the old column type to check if we need special enum handling
        let old_type = current_schema
            .iter()
            .find(|t| t.name == table)
            .and_then(|t| t.columns.iter().find(|c| c.name == column))
            .map(|c| &c.r#type);

        // Check if this is an enum-to-enum migration that needs special handling (PostgreSQL only)
        let needs_enum_migration = if *backend == DatabaseBackend::Postgres {
            matches!(
                (old_type, new_type),
                (
                    Some(ColumnType::Complex(ComplexColumnType::Enum { name: old_name, values: old_values })),
                    ColumnType::Complex(ComplexColumnType::Enum { name: new_name, values: new_values })
                ) if old_name == new_name && old_values != new_values
            )
        } else {
            false
        };

        if needs_enum_migration {
            // Use the safe temp type + USING + RENAME approach for enum value changes
            if let (
                Some(ColumnType::Complex(ComplexColumnType::Enum {
                    name: enum_name, ..
                })),
                ColumnType::Complex(ComplexColumnType::Enum {
                    values: new_values, ..
                }),
            ) = (old_type, new_type)
            {
                // Use table-prefixed enum type names
                let type_name = super::helpers::build_enum_type_name(table, enum_name);
                let temp_type_name = format!("{}_new", type_name);

                // 1. CREATE TYPE {table}_{enum}_new AS ENUM (new values)
                let create_temp_values = new_values.to_sql_values().join(", ");
                queries.push(BuiltQuery::Raw(super::types::RawSql::per_backend(
                    format!(
                        "CREATE TYPE \"{}\" AS ENUM ({})",
                        temp_type_name, create_temp_values
                    ),
                    String::new(),
                    String::new(),
                )));

                // 2. ALTER TABLE ... ALTER COLUMN ... TYPE {table}_{enum}_new USING {column}::text::{table}_{enum}_new
                queries.push(BuiltQuery::Raw(super::types::RawSql::per_backend(
                    format!(
                        "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" TYPE \"{}\" USING \"{}\"::text::\"{}\"",
                        table, column, temp_type_name, column, temp_type_name
                    ),
                    String::new(),
                    String::new(),
                )));

                // 3. DROP TYPE {table}_{enum}
                queries.push(BuiltQuery::Raw(super::types::RawSql::per_backend(
                    format!("DROP TYPE \"{}\"", type_name),
                    String::new(),
                    String::new(),
                )));

                // 4. ALTER TYPE {table}_{enum}_new RENAME TO {table}_{enum}
                queries.push(BuiltQuery::Raw(super::types::RawSql::per_backend(
                    format!(
                        "ALTER TYPE \"{}\" RENAME TO \"{}\"",
                        temp_type_name, type_name
                    ),
                    String::new(),
                    String::new(),
                )));
            }
        } else {
            // Standard column type modification

            // If new type is an enum and different from old, create the type first (PostgreSQL only)
            if let ColumnType::Complex(ComplexColumnType::Enum { name: new_name, .. }) = new_type {
                // Determine if we need to create a new enum type
                // - If old type was a different enum, we need to create the new one
                // - If old type was not an enum, we need to create the enum type
                let should_create = if let Some(ColumnType::Complex(ComplexColumnType::Enum {
                    name: old_name,
                    ..
                })) = old_type
                {
                    old_name != new_name
                } else {
                    // Either old_type is None or it wasn't an enum - need to create enum type
                    true
                };

                if should_create
                    && let Some(create_type_sql) = build_create_enum_type_sql(table, new_type)
                {
                    queries.push(BuiltQuery::Raw(create_type_sql));
                }
            }

            let mut col = SeaColumnDef::new(Alias::new(column));
            apply_column_type_with_table(&mut col, new_type, table);

            let stmt = Table::alter()
                .table(Alias::new(table))
                .modify_column(col)
                .to_owned();
            queries.push(BuiltQuery::AlterTable(Box::new(stmt)));

            // If old type was an enum and new type is different, drop the old enum type
            if let Some(ColumnType::Complex(ComplexColumnType::Enum { name: old_name, .. })) =
                old_type
            {
                let should_drop = match new_type {
                    ColumnType::Complex(ComplexColumnType::Enum { name: new_name, .. }) => {
                        old_name != new_name
                    }
                    _ => true, // New type is not an enum
                };

                if should_drop {
                    // Use table-prefixed enum type name
                    let old_type_name = super::helpers::build_enum_type_name(table, old_name);
                    queries.push(BuiltQuery::Raw(super::types::RawSql::per_backend(
                        format!("DROP TYPE IF EXISTS \"{}\"", old_type_name),
                        String::new(),
                        String::new(),
                    )));
                }
            }
        }

        Ok(queries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, TableDef,
    };

    #[rstest]
    #[case::modify_column_type_postgres(
        "modify_column_type_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\"", "\"age\""]
    )]
    #[case::modify_column_type_mysql(
        "modify_column_type_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` MODIFY COLUMN `age` varchar(50)"]
    )]
    #[case::modify_column_type_sqlite(
        "modify_column_type_sqlite",
        DatabaseBackend::Sqlite,
        &[]
    )]
    fn test_modify_column_type(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        // For SQLite, we need to provide current schema
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "age".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
        }];

        let result = build_modify_column_type(
            &backend,
            "users",
            "age",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
            &current_schema,
        );

        // SQLite may return multiple queries
        let sql = result
            .unwrap()
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }
        println!("sql: {}", sql);

        with_settings!({ snapshot_suffix => format!("modify_column_type_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_modify_column_type_table_not_found() {
        let result = build_modify_column_type(
            &DatabaseBackend::Sqlite,
            "nonexistent_table",
            "age",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            &[],
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Table 'nonexistent_table' not found")
        );
    }

    #[test]
    fn test_modify_column_type_column_not_found() {
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_modify_column_type(
            &DatabaseBackend::Sqlite,
            "users",
            "nonexistent_column",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            &current_schema,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Column 'nonexistent_column' not found")
        );
    }

    #[rstest]
    #[case::modify_column_type_with_index_postgres(
        "modify_column_type_with_index_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_column_type_with_index_mysql(
        "modify_column_type_with_index_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_column_type_with_index_sqlite(
        "modify_column_type_with_index_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_column_type_with_index(#[case] title: &str, #[case] backend: DatabaseBackend) {
        // Test modify column type with indexes
        use vespertide_core::TableConstraint;

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "age".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::Index {
                name: Some("idx_age".into()),
                columns: vec!["age".into()],
            }],
        }];

        let result = build_modify_column_type(
            &backend,
            "users",
            "age",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            &current_schema,
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        // For SQLite, should recreate index
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_age"));
        }

        with_settings!({ snapshot_suffix => format!("modify_column_type_with_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::modify_column_type_with_unique_constraint_postgres(
        "modify_column_type_with_unique_constraint_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_column_type_with_unique_constraint_mysql(
        "modify_column_type_with_unique_constraint_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_column_type_with_unique_constraint_sqlite(
        "modify_column_type_with_unique_constraint_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_column_type_with_unique_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        // Test modify column type with unique constraint
        use vespertide_core::TableConstraint;

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }],
        }];

        let result = build_modify_column_type(
            &backend,
            "users",
            "email",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
            &current_schema,
        )
        .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        // For SQLite, unique constraint should be in CREATE TABLE statement
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE TABLE"));
        }

        with_settings!({ snapshot_suffix => format!("modify_column_type_with_unique_constraint_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::enum_values_changed_postgres(
        "enum_values_changed_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_values_changed_mysql(
        "enum_values_changed_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_values_changed_sqlite(
        "enum_values_changed_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_same_values_postgres(
        "enum_same_values_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_same_values_mysql(
        "enum_same_values_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_same_values_sqlite(
        "enum_same_values_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_postgres(
        "enum_name_changed_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_mysql(
        "enum_name_changed_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_sqlite(
        "enum_name_changed_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_postgres(
        "text_to_enum_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_mysql(
        "text_to_enum_mysql",
        DatabaseBackend::MySql,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_sqlite(
        "text_to_enum_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_to_text_postgres(
        "enum_to_text_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    #[case::enum_to_text_mysql(
        "enum_to_text_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    #[case::enum_to_text_sqlite(
        "enum_to_text_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    fn test_modify_enum_types(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] old_type: ColumnType,
        #[case] new_type: ColumnType,
    ) {
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: old_type,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let result =
            build_modify_column_type(&backend, "users", "status", &new_type, &current_schema)
                .unwrap();

        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        with_settings!({ snapshot_suffix => format!("modify_enum_types_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_modify_column_type_to_enum_with_empty_schema() {
        // Test the None branch in line 195-200
        // When current_schema is empty, old_type will be None
        use vespertide_core::ComplexColumnType;

        let result = build_modify_column_type(
            &DatabaseBackend::Postgres,
            "users",
            "status",
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "status_type".into(),
                values: EnumValues::String(vec!["active".into(), "inactive".into()]),
            }),
            &[], // Empty schema - old_type will be None
        );

        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<String>>()
            .join(";\n");

        // Should create the enum type since old_type is None
        assert!(sql.contains("CREATE TYPE"));
        assert!(sql.contains("status_type"));
        assert!(sql.contains("ALTER TABLE"));
    }
}
