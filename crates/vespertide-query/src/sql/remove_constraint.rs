use sea_query::{Alias, ForeignKey, Query, Table};

use vespertide_core::{TableConstraint, TableDef};

use super::create_table::build_create_table_for_backend;
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;
use crate::sql::RawSql;

pub fn build_remove_constraint(
    backend: &DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => {
            if *backend == DatabaseBackend::Sqlite {
                // SQLite does not support dropping primary key constraints, use temp table approach
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to remove constraints.",
                        table
                    )))?;

                // Remove the primary key constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.retain(|c| !matches!(c, TableConstraint::PrimaryKey { .. }));

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table without primary key constraint
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );
                let create_query = BuiltQuery::CreateTable(Box::new(create_temp_table));

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
                let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

                // 3. Drop original table
                let drop_table = Table::drop().table(Alias::new(table)).to_owned();
                let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

                // 4. Rename temporary table to original name
                let rename_query = build_rename_table(&temp_table, table);

                // 5. Recreate indexes (if any)
                let mut index_queries = Vec::new();
                for index in &table_def.indexes {
                    let mut idx_stmt = sea_query::Index::create();
                    idx_stmt = idx_stmt.name(&index.name).to_owned();
                    for col_name in &index.columns {
                        idx_stmt = idx_stmt.col(Alias::new(col_name)).to_owned();
                    }
                    if index.unique {
                        idx_stmt = idx_stmt.unique().to_owned();
                    }
                    idx_stmt = idx_stmt.table(Alias::new(table)).to_owned();
                    index_queries.push(BuiltQuery::CreateIndex(Box::new(idx_stmt)));
                }

                let mut queries = vec![create_query, insert_query, drop_query, rename_query];
                queries.extend(index_queries);
                Ok(queries)
            } else {
                // Other backends: use raw SQL
                let pg_sql = format!(
                    "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}_pkey\"",
                    table, table
                );
                let mysql_sql = format!("ALTER TABLE `{}` DROP PRIMARY KEY", table);
                Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                    pg_sql.clone(),
                    mysql_sql,
                    pg_sql,
                ))])
            }
        }
        TableConstraint::Unique { name, columns } => {
            // SQLite does not support ALTER TABLE ... DROP CONSTRAINT UNIQUE
            if *backend == DatabaseBackend::Sqlite {
                // Use temporary table approach for SQLite
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to remove constraints.",
                        table
                    )))?;

                // Create new constraints without the removed unique constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.retain(|c| {
                    match (c, constraint) {
                        (
                            TableConstraint::Unique {
                                name: c_name,
                                columns: c_cols,
                            },
                            TableConstraint::Unique {
                                name: r_name,
                                columns: r_cols,
                            },
                        ) => {
                            // Remove if names match, or if no name and columns match
                            if let (Some(cn), Some(rn)) = (c_name, r_name) {
                                cn != rn
                            } else {
                                c_cols != r_cols
                            }
                        }
                        _ => true,
                    }
                });

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table without the removed constraint
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );
                let create_query = BuiltQuery::CreateTable(Box::new(create_temp_table));

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
                let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

                // 3. Drop original table
                let drop_table = Table::drop().table(Alias::new(table)).to_owned();
                let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

                // 4. Rename temporary table to original name
                let rename_query = build_rename_table(&temp_table, table);

                // 5. Recreate indexes (if any)
                // Note: We need to filter out indexes that might be associated with the unique constraint if any
                // But TableDef separates constraints and indexes.
                let mut index_queries = Vec::new();
                for index in &table_def.indexes {
                    let mut idx_stmt = sea_query::Index::create();
                    idx_stmt = idx_stmt.name(&index.name).to_owned();
                    for col_name in &index.columns {
                        idx_stmt = idx_stmt.col(Alias::new(col_name)).to_owned();
                    }
                    if index.unique {
                        idx_stmt = idx_stmt.unique().to_owned();
                    }
                    idx_stmt = idx_stmt.table(Alias::new(table)).to_owned();
                    index_queries.push(BuiltQuery::CreateIndex(Box::new(idx_stmt)));
                }

                let mut queries = vec![create_query, insert_query, drop_query, rename_query];
                queries.extend(index_queries);
                Ok(queries)
            } else {
                // For unique constraints, PostgreSQL uses DROP CONSTRAINT, MySQL uses DROP INDEX
                // sea_query 0.32 doesn't support dropping unique constraint via Table::alter() directly
                // We'll use Index::drop() which generates DROP INDEX for both backends
                // However, PostgreSQL expects DROP CONSTRAINT, so we need to use Table::alter()
                // Since drop_constraint() doesn't exist, we'll use Index::drop() for now
                // Note: This may not match PostgreSQL's DROP CONSTRAINT syntax
                let constraint_name = if let Some(n) = name {
                    n.clone()
                } else {
                    format!("{}_{}_key", table, columns.join("_"))
                };
                // Try using Table::alter() with drop_constraint if available
                // If not, use Index::drop() as fallback
                // For PostgreSQL, we need DROP CONSTRAINT, but sea_query doesn't support this
                // We'll use raw SQL for PostgreSQL and Index::drop() for MySQL
                let pg_sql = format!(
                    "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"",
                    table, constraint_name
                );
                let mysql_sql = format!("ALTER TABLE `{}` DROP INDEX `{}`", table, constraint_name);
                Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                    pg_sql.clone(),
                    mysql_sql,
                    pg_sql,
                ))])
            }
        }
        TableConstraint::ForeignKey { name, columns, .. } => {
            // SQLite does not support ALTER TABLE ... DROP CONSTRAINT FOREIGN KEY
            if *backend == DatabaseBackend::Sqlite {
                // Use temporary table approach for SQLite
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to remove constraints.",
                        table
                    )))?;

                // Create new constraints without the removed foreign key constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.retain(|c| {
                    match (c, constraint) {
                        (
                            TableConstraint::ForeignKey {
                                name: c_name,
                                columns: c_cols,
                                ..
                            },
                            TableConstraint::ForeignKey {
                                name: r_name,
                                columns: r_cols,
                                ..
                            },
                        ) => {
                            // Remove if names match, or if no name and columns match
                            if let (Some(cn), Some(rn)) = (c_name, r_name) {
                                cn != rn
                            } else {
                                c_cols != r_cols
                            }
                        }
                        _ => true,
                    }
                });

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table without the removed constraint
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );
                let create_query = BuiltQuery::CreateTable(Box::new(create_temp_table));

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
                let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

                // 3. Drop original table
                let drop_table = Table::drop().table(Alias::new(table)).to_owned();
                let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

                // 4. Rename temporary table to original name
                let rename_query = build_rename_table(&temp_table, table);

                // 5. Recreate indexes (if any)
                let mut index_queries = Vec::new();
                for index in &table_def.indexes {
                    let mut idx_stmt = sea_query::Index::create();
                    idx_stmt = idx_stmt.name(&index.name).to_owned();
                    for col_name in &index.columns {
                        idx_stmt = idx_stmt.col(Alias::new(col_name)).to_owned();
                    }
                    if index.unique {
                        idx_stmt = idx_stmt.unique().to_owned();
                    }
                    idx_stmt = idx_stmt.table(Alias::new(table)).to_owned();
                    index_queries.push(BuiltQuery::CreateIndex(Box::new(idx_stmt)));
                }

                let mut queries = vec![create_query, insert_query, drop_query, rename_query];
                queries.extend(index_queries);
                Ok(queries)
            } else {
                // Build foreign key drop using ForeignKey::drop()
                let constraint_name = if let Some(n) = name {
                    n.clone()
                } else {
                    format!("{}_{}_fkey", table, columns.join("_"))
                };
                let fk_drop = ForeignKey::drop()
                    .name(&constraint_name)
                    .table(Alias::new(table))
                    .to_owned();
                Ok(vec![BuiltQuery::DropForeignKey(Box::new(fk_drop))])
            }
        }
        TableConstraint::Check { name, .. } => {
            // SQLite does not support ALTER TABLE ... DROP CONSTRAINT CHECK
            if *backend == DatabaseBackend::Sqlite {
                // Use temporary table approach for SQLite
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to remove constraints.",
                        table
                    )))?;

                // Create new constraints without the removed check constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.retain(|c| match (c, constraint) {
                    (
                        TableConstraint::Check { name: c_name, .. },
                        TableConstraint::Check { name: r_name, .. },
                    ) => c_name != r_name,
                    _ => true,
                });

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table without the removed constraint
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );
                let create_query = BuiltQuery::CreateTable(Box::new(create_temp_table));

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
                let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

                // 3. Drop original table
                let drop_table = Table::drop().table(Alias::new(table)).to_owned();
                let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

                // 4. Rename temporary table to original name
                let rename_query = build_rename_table(&temp_table, table);

                // 5. Recreate indexes (if any)
                let mut index_queries = Vec::new();
                for index in &table_def.indexes {
                    let mut idx_stmt = sea_query::Index::create();
                    idx_stmt = idx_stmt.name(&index.name).to_owned();
                    for col_name in &index.columns {
                        idx_stmt = idx_stmt.col(Alias::new(col_name)).to_owned();
                    }
                    if index.unique {
                        idx_stmt = idx_stmt.unique().to_owned();
                    }
                    idx_stmt = idx_stmt.table(Alias::new(table)).to_owned();
                    index_queries.push(BuiltQuery::CreateIndex(Box::new(idx_stmt)));
                }

                let mut queries = vec![create_query, insert_query, drop_query, rename_query];
                queries.extend(index_queries);
                Ok(queries)
            } else {
                let pg_sql = format!("ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"", table, name);
                let mysql_sql = format!("ALTER TABLE `{}` DROP CHECK `{}`", table, name);
                Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                    pg_sql.clone(),
                    mysql_sql,
                    pg_sql,
                ))])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};

    #[rstest]
    #[case::remove_constraint_primary_key_postgres(
        "remove_constraint_primary_key_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"users_pkey\""]
    )]
    #[case::remove_constraint_primary_key_mysql(
        "remove_constraint_primary_key_mysql",
        DatabaseBackend::MySql,
        &["DROP PRIMARY KEY"]
    )]
    #[case::remove_constraint_primary_key_sqlite(
        "remove_constraint_primary_key_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::remove_constraint_unique_named_postgres(
        "remove_constraint_unique_named_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"uq_email\""]
    )]
    #[case::remove_constraint_unique_named_mysql(
        "remove_constraint_unique_named_mysql",
        DatabaseBackend::MySql,
        &["DROP INDEX `uq_email`"]
    )]
    #[case::remove_constraint_unique_named_sqlite(
        "remove_constraint_unique_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::remove_constraint_foreign_key_named_postgres(
        "remove_constraint_foreign_key_named_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"fk_user\""]
    )]
    #[case::remove_constraint_foreign_key_named_mysql(
        "remove_constraint_foreign_key_named_mysql",
        DatabaseBackend::MySql,
        &["DROP FOREIGN KEY `fk_user`"]
    )]
    #[case::remove_constraint_foreign_key_named_sqlite(
        "remove_constraint_foreign_key_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::remove_constraint_check_named_postgres(
        "remove_constraint_check_named_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"chk_age\""]
    )]
    #[case::remove_constraint_check_named_mysql(
        "remove_constraint_check_named_mysql",
        DatabaseBackend::MySql,
        &["DROP CHECK `chk_age`"]
    )]
    #[case::remove_constraint_check_named_sqlite(
        "remove_constraint_check_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    fn test_remove_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let constraint = if title.contains("primary_key") {
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
            }
        } else if title.contains("unique") {
            TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }
        } else if title.contains("foreign_key") {
            TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }
        } else {
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            }
        };

        // For SQLite, we need to provide current schema with the constraint to be removed
        let current_schema = vec![TableDef {
            name: "users".into(),
            columns: if title.contains("check") {
                vec![
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
                ]
            } else if title.contains("foreign_key") {
                vec![
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
                        name: "user_id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: true,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    },
                ]
            } else {
                // primary key / unique cases
                vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }]
            },
            constraints: vec![constraint.clone()],
            indexes: vec![],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("remove_constraint_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }

    #[test]
    fn test_remove_constraint_primary_key_sqlite_table_not_found() {
        // Test error when table is not found (line 25)
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        };
        let result = build_remove_constraint(
            &DatabaseBackend::Sqlite,
            "nonexistent_table",
            &constraint,
            &[], // Empty schema
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'nonexistent_table' not found in current schema"));
    }

    #[rstest]
    #[case::remove_primary_key_with_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_primary_key_with_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_primary_key_with_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_primary_key_with_index(#[case] backend: DatabaseBackend) {
        // Test PrimaryKey removal with indexes (lines 75-78, 83-84)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_id".into(),
                columns: vec!["id".into()],
                unique: false,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_id"));
        }

        with_settings!({ snapshot_suffix => format!("remove_primary_key_with_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_primary_key_with_unique_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_primary_key_with_unique_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_primary_key_with_unique_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_primary_key_with_unique_index(#[case] backend: DatabaseBackend) {
        // Test PrimaryKey removal with unique index (lines 75-78, 80-81, 83-84)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_email".into(),
                columns: vec!["email".into()],
                unique: true,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE UNIQUE INDEX"));
            assert!(sql.contains("idx_email"));
        }

        with_settings!({ snapshot_suffix => format!("remove_primary_key_with_unique_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_remove_constraint_unique_sqlite_table_not_found() {
        // Test error when table is not found (line 112)
        let constraint = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        };
        let result = build_remove_constraint(
            &DatabaseBackend::Sqlite,
            "nonexistent_table",
            &constraint,
            &[], // Empty schema
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'nonexistent_table' not found in current schema"));
    }

    #[rstest]
    #[case::remove_unique_without_name_postgres(DatabaseBackend::Postgres)]
    #[case::remove_unique_without_name_mysql(DatabaseBackend::MySql)]
    #[case::remove_unique_without_name_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_unique_without_name(#[case] backend: DatabaseBackend) {
        // Test Unique removal without name (lines 134, 137, 210)
        let constraint = TableConstraint::Unique {
            name: None,
            columns: vec!["email".into()],
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should generate default constraint name
        if !matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("users_email_key") || sql.contains("email"));
        }

        with_settings!({ snapshot_suffix => format!("remove_unique_without_name_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_unique_with_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_unique_with_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_unique_with_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_unique_with_index(#[case] backend: DatabaseBackend) {
        // Test Unique removal with indexes (lines 185-188, 193-194)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_id".into(),
                columns: vec!["id".into()],
                unique: false,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_id"));
        }

        with_settings!({ snapshot_suffix => format!("remove_unique_with_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_unique_with_unique_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_unique_with_unique_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_unique_with_unique_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_unique_with_unique_index(#[case] backend: DatabaseBackend) {
        // Test Unique removal with unique index (lines 185-188, 190-191, 193-194)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_name".into(),
                columns: vec!["name".into()],
                unique: true,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE UNIQUE INDEX"));
            assert!(sql.contains("idx_name"));
        }

        with_settings!({ snapshot_suffix => format!("remove_unique_with_unique_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_remove_constraint_foreign_key_sqlite_table_not_found() {
        // Test error when table is not found (line 236)
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let result = build_remove_constraint(
            &DatabaseBackend::Sqlite,
            "nonexistent_table",
            &constraint,
            &[], // Empty schema
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'nonexistent_table' not found in current schema"));
    }

    #[rstest]
    #[case::remove_foreign_key_without_name_postgres(DatabaseBackend::Postgres)]
    #[case::remove_foreign_key_without_name_mysql(DatabaseBackend::MySql)]
    #[case::remove_foreign_key_without_name_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_foreign_key_without_name(#[case] backend: DatabaseBackend) {
        // Test ForeignKey removal without name (lines 260, 263, 329)
        let constraint = TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let current_schema = vec![TableDef {
            name: "posts".into(),
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
                    name: "user_id".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![],
        }];

        let result =
            build_remove_constraint(&backend, "posts", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should generate default constraint name
        if !matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("posts_user_id_fkey") || sql.contains("user_id"));
        }

        with_settings!({ snapshot_suffix => format!("remove_foreign_key_without_name_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_foreign_key_with_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_foreign_key_with_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_foreign_key_with_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_foreign_key_with_index(#[case] backend: DatabaseBackend) {
        // Test ForeignKey removal with indexes (lines 309-312, 317-318)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let current_schema = vec![TableDef {
            name: "posts".into(),
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
                    name: "user_id".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_user_id".into(),
                columns: vec!["user_id".into()],
                unique: false,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "posts", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_user_id"));
        }

        with_settings!({ snapshot_suffix => format!("remove_foreign_key_with_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_foreign_key_with_unique_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_foreign_key_with_unique_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_foreign_key_with_unique_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_foreign_key_with_unique_index(#[case] backend: DatabaseBackend) {
        // Test ForeignKey removal with unique index (lines 309-312, 314-315, 317-318)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let current_schema = vec![TableDef {
            name: "posts".into(),
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
                    name: "user_id".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_user_id".into(),
                columns: vec!["user_id".into()],
                unique: true,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "posts", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE UNIQUE INDEX"));
            assert!(sql.contains("idx_user_id"));
        }

        with_settings!({ snapshot_suffix => format!("remove_foreign_key_with_unique_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_remove_constraint_check_sqlite_table_not_found() {
        // Test error when table is not found (line 346)
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let result = build_remove_constraint(
            &DatabaseBackend::Sqlite,
            "nonexistent_table",
            &constraint,
            &[], // Empty schema
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'nonexistent_table' not found in current schema"));
    }

    #[rstest]
    #[case::remove_check_with_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_check_with_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_check_with_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_check_with_index(#[case] backend: DatabaseBackend) {
        // Test Check removal with indexes (lines 402-405, 410-411)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_age".into(),
                columns: vec!["age".into()],
                unique: false,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_age"));
        }

        with_settings!({ snapshot_suffix => format!("remove_check_with_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_check_with_unique_index_postgres(DatabaseBackend::Postgres)]
    #[case::remove_check_with_unique_index_mysql(DatabaseBackend::MySql)]
    #[case::remove_check_with_unique_index_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_check_with_unique_index(#[case] backend: DatabaseBackend) {
        // Test Check removal with unique index (lines 402-405, 407-408, 410-411)
        use vespertide_core::IndexDef;

        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![constraint.clone()],
            indexes: vec![IndexDef {
                name: "idx_age".into(),
                columns: vec!["age".into()],
                unique: true,
            }],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE UNIQUE INDEX"));
            assert!(sql.contains("idx_age"));
        }

        with_settings!({ snapshot_suffix => format!("remove_check_with_unique_index_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_unique_with_other_constraints_postgres(DatabaseBackend::Postgres)]
    #[case::remove_unique_with_other_constraints_mysql(DatabaseBackend::MySql)]
    #[case::remove_unique_with_other_constraints_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_unique_with_other_constraints(#[case] backend: DatabaseBackend) {
        // Test Unique removal with other constraint types (line 137)
        let constraint = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![
                TableConstraint::PrimaryKey {
                    columns: vec!["id".into()],
                    auto_increment: false,
                },
                constraint.clone(),
                TableConstraint::Check {
                    name: "chk_email".into(),
                    expr: "email IS NOT NULL".into(),
                },
            ],
            indexes: vec![],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should still work with other constraint types present
        assert!(sql.contains("DROP") || sql.contains("CREATE TABLE"));

        with_settings!({ snapshot_suffix => format!("remove_unique_with_other_constraints_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_foreign_key_with_other_constraints_postgres(DatabaseBackend::Postgres)]
    #[case::remove_foreign_key_with_other_constraints_mysql(DatabaseBackend::MySql)]
    #[case::remove_foreign_key_with_other_constraints_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_foreign_key_with_other_constraints(#[case] backend: DatabaseBackend) {
        // Test ForeignKey removal with other constraint types (line 263)
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let current_schema = vec![TableDef {
            name: "posts".into(),
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
                    name: "user_id".into(),
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
            constraints: vec![
                TableConstraint::PrimaryKey {
                    columns: vec!["id".into()],
                    auto_increment: false,
                },
                constraint.clone(),
                TableConstraint::Unique {
                    name: Some("uq_user_id".into()),
                    columns: vec!["user_id".into()],
                },
                TableConstraint::Check {
                    name: "chk_user_id".into(),
                    expr: "user_id > 0".into(),
                },
            ],
            indexes: vec![],
        }];

        let result =
            build_remove_constraint(&backend, "posts", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should still work with other constraint types present
        assert!(sql.contains("DROP") || sql.contains("CREATE TABLE"));

        with_settings!({ snapshot_suffix => format!("remove_foreign_key_with_other_constraints_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_check_with_other_constraints_postgres(DatabaseBackend::Postgres)]
    #[case::remove_check_with_other_constraints_mysql(DatabaseBackend::MySql)]
    #[case::remove_check_with_other_constraints_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_check_with_other_constraints(#[case] backend: DatabaseBackend) {
        // Test Check removal with other constraint types (line 357)
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
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
            constraints: vec![
                TableConstraint::PrimaryKey {
                    columns: vec!["id".into()],
                    auto_increment: false,
                },
                TableConstraint::Unique {
                    name: Some("uq_age".into()),
                    columns: vec!["age".into()],
                },
                constraint.clone(),
            ],
            indexes: vec![],
        }];

        let result =
            build_remove_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should still work with other constraint types present
        assert!(sql.contains("DROP") || sql.contains("CREATE TABLE"));

        with_settings!({ snapshot_suffix => format!("remove_check_with_other_constraints_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }
}
