use sea_query::{Alias, ForeignKey, Index, Query, Table};

use vespertide_core::{TableConstraint, TableDef};

use super::create_table::build_create_table_for_backend;
use super::helpers::{build_schema_statement, to_sea_fk_action};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;
use crate::sql::RawSql;

/// Extract CHECK constraint clauses from a list of constraints
fn extract_check_clauses(constraints: &[TableConstraint]) -> Vec<String> {
    constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Check { name, expr } = c {
                Some(format!("CONSTRAINT \"{}\" CHECK ({})", name, expr))
            } else {
                None
            }
        })
        .collect()
}

/// Build CREATE TABLE query with CHECK constraints properly embedded
fn build_create_with_checks(
    backend: &DatabaseBackend,
    create_stmt: &sea_query::TableCreateStatement,
    check_clauses: &[String],
) -> BuiltQuery {
    if check_clauses.is_empty() {
        BuiltQuery::CreateTable(Box::new(create_stmt.clone()))
    } else {
        let base_sql = build_schema_statement(create_stmt, *backend);
        let mut modified_sql = base_sql;
        if let Some(pos) = modified_sql.rfind(')') {
            let check_sql = check_clauses.join(", ");
            modified_sql.insert_str(pos, &format!(", {}", check_sql));
        }
        BuiltQuery::Raw(RawSql::per_backend(
            modified_sql.clone(),
            modified_sql.clone(),
            modified_sql,
        ))
    }
}

pub fn build_add_constraint(
    backend: &DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            if *backend == DatabaseBackend::Sqlite {
                // SQLite does not support ALTER TABLE ... ADD PRIMARY KEY
                // Use temporary table approach
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to add constraints.",
                        table
                    )))?;

                // Create new constraints with the added primary key constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.push(constraint.clone());

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table with new constraints
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );

                // Handle CHECK constraints (sea-query doesn't support them natively)
                let check_clauses = extract_check_clauses(&new_constraints);
                let create_query =
                    build_create_with_checks(backend, &create_temp_table, &check_clauses);

                // 2. Copy data
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

                // 4. Rename temporary table
                let rename_query = build_rename_table(&temp_table, table);

                // 5. Recreate indexes from Index constraints
                let mut index_queries = Vec::new();
                for c in &table_def.constraints {
                    if let TableConstraint::Index {
                        name: idx_name,
                        columns: idx_cols,
                    } = c
                    {
                        let index_name = vespertide_naming::build_index_name(
                            table,
                            idx_cols,
                            idx_name.as_deref(),
                        );
                        let mut idx_stmt = sea_query::Index::create();
                        idx_stmt = idx_stmt.name(&index_name).to_owned();
                        for col_name in idx_cols {
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
                // sea_query lacks ALTER TABLE ADD PRIMARY KEY; emit backend SQL
                let pg_cols = columns
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mysql_cols = columns
                    .iter()
                    .map(|c| format!("`{}`", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let pg_sql = format!("ALTER TABLE \"{}\" ADD PRIMARY KEY ({})", table, pg_cols);
                let mysql_sql = format!("ALTER TABLE `{}` ADD PRIMARY KEY ({})", table, mysql_cols);
                Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                    pg_sql.clone(),
                    mysql_sql,
                    pg_sql,
                ))])
            }
        }
        TableConstraint::Unique { name, columns } => {
            // SQLite does not support ALTER TABLE ... ADD CONSTRAINT UNIQUE
            // Always generate a proper name: uq_{table}_{key} or uq_{table}_{columns}
            let index_name =
                super::helpers::build_unique_constraint_name(table, columns, name.as_deref());
            let mut idx = Index::create()
                .table(Alias::new(table))
                .name(&index_name)
                .unique()
                .to_owned();
            for col in columns {
                idx = idx.col(Alias::new(col)).to_owned();
            }
            Ok(vec![BuiltQuery::CreateIndex(Box::new(idx))])
        }
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => {
            // SQLite does not support ALTER TABLE ... ADD CONSTRAINT FOREIGN KEY
            if *backend == DatabaseBackend::Sqlite {
                // Use temporary table approach for SQLite
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to add constraints.",
                        table
                    )))?;

                // Create new constraints with the added foreign key constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.push(constraint.clone());

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table with new constraints
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );

                // Handle CHECK constraints (sea-query doesn't support them natively)
                let check_clauses = extract_check_clauses(&new_constraints);
                let create_query =
                    build_create_with_checks(backend, &create_temp_table, &check_clauses);

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

                // 5. Recreate indexes from Index constraints
                let mut index_queries = Vec::new();
                for c in &table_def.constraints {
                    if let TableConstraint::Index {
                        name: idx_name,
                        columns: idx_cols,
                    } = c
                    {
                        let index_name = vespertide_naming::build_index_name(
                            table,
                            idx_cols,
                            idx_name.as_deref(),
                        );
                        let mut idx_stmt = sea_query::Index::create();
                        idx_stmt = idx_stmt.name(&index_name).to_owned();
                        for col_name in idx_cols {
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
                // Build foreign key using ForeignKey::create
                let fk_name = vespertide_naming::build_foreign_key_name(
                    table,
                    columns,
                    name.as_deref(),
                );
                let mut fk = ForeignKey::create();
                fk = fk.name(&fk_name).to_owned();
                fk = fk.from_tbl(Alias::new(table)).to_owned();
                for col in columns {
                    fk = fk.from_col(Alias::new(col)).to_owned();
                }
                fk = fk.to_tbl(Alias::new(ref_table)).to_owned();
                for col in ref_columns {
                    fk = fk.to_col(Alias::new(col)).to_owned();
                }
                if let Some(action) = on_delete {
                    fk = fk.on_delete(to_sea_fk_action(action)).to_owned();
                }
                if let Some(action) = on_update {
                    fk = fk.on_update(to_sea_fk_action(action)).to_owned();
                }
                Ok(vec![BuiltQuery::CreateForeignKey(Box::new(fk))])
            }
        }
        TableConstraint::Index { name, columns } => {
            // Index constraints are simple CREATE INDEX statements for all backends
            let index_name = vespertide_naming::build_index_name(table, columns, name.as_deref());
            let mut idx = Index::create()
                .table(Alias::new(table))
                .name(&index_name)
                .to_owned();
            for col in columns {
                idx = idx.col(Alias::new(col)).to_owned();
            }
            Ok(vec![BuiltQuery::CreateIndex(Box::new(idx))])
        }
        TableConstraint::Check { name, expr } => {
            // SQLite does not support ALTER TABLE ... ADD CONSTRAINT CHECK
            if *backend == DatabaseBackend::Sqlite {
                // Use temporary table approach for SQLite
                let table_def = current_schema
                    .iter()
                    .find(|t| t.name == table)
                    .ok_or_else(|| QueryError::Other(format!(
                        "Table '{}' not found in current schema. SQLite requires current schema information to add constraints.",
                        table
                    )))?;

                // Create new constraints with the added check constraint
                let mut new_constraints = table_def.constraints.clone();
                new_constraints.push(constraint.clone());

                // Generate temporary table name
                let temp_table = format!("{}_temp", table);

                // 1. Create temporary table with new constraints
                let create_temp_table = build_create_table_for_backend(
                    backend,
                    &temp_table,
                    &table_def.columns,
                    &new_constraints,
                );

                // Handle CHECK constraints (sea-query doesn't support them natively)
                let check_clauses = extract_check_clauses(&new_constraints);
                let create_query =
                    build_create_with_checks(backend, &create_temp_table, &check_clauses);

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

                // 5. Recreate indexes from Index constraints
                let mut index_queries = Vec::new();
                for c in &table_def.constraints {
                    if let TableConstraint::Index {
                        name: idx_name,
                        columns: idx_cols,
                    } = c
                    {
                        let index_name = vespertide_naming::build_index_name(
                            table,
                            idx_cols,
                            idx_name.as_deref(),
                        );
                        let mut idx_stmt = sea_query::Index::create();
                        idx_stmt = idx_stmt.name(&index_name).to_owned();
                        for col_name in idx_cols {
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
                let pg_sql = format!(
                    "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" CHECK ({})",
                    table, name, expr
                );
                let mysql_sql = format!(
                    "ALTER TABLE `{}` ADD CONSTRAINT `{}` CHECK ({})",
                    table, name, expr
                );
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
    use vespertide_core::{
        ColumnDef, ColumnType, ReferenceAction, SimpleColumnType, TableConstraint, TableDef,
    };

    #[rstest]
    #[case::add_constraint_primary_key_postgres(
        "add_constraint_primary_key_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD PRIMARY KEY (\"id\")"]
    )]
    #[case::add_constraint_primary_key_mysql(
        "add_constraint_primary_key_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD PRIMARY KEY (`id`)"]
    )]
    #[case::add_constraint_primary_key_sqlite(
        "add_constraint_primary_key_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::add_constraint_unique_named_postgres(
        "add_constraint_unique_named_postgres",
        DatabaseBackend::Postgres,
        &["CREATE UNIQUE INDEX \"uq_users__uq_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_constraint_unique_named_mysql(
        "add_constraint_unique_named_mysql",
        DatabaseBackend::MySql,
        &["CREATE UNIQUE INDEX `uq_users__uq_email` ON `users` (`email`)"]
    )]
    #[case::add_constraint_unique_named_sqlite(
        "add_constraint_unique_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE UNIQUE INDEX \"uq_users__uq_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_constraint_foreign_key_postgres(
        "add_constraint_foreign_key_postgres",
        DatabaseBackend::Postgres,
        &["FOREIGN KEY (\"user_id\")", "REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::add_constraint_foreign_key_mysql(
        "add_constraint_foreign_key_mysql",
        DatabaseBackend::MySql,
        &["FOREIGN KEY (`user_id`)", "REFERENCES `users` (`id`)", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::add_constraint_foreign_key_sqlite(
        "add_constraint_foreign_key_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::add_constraint_check_named_postgres(
        "add_constraint_check_named_postgres",
        DatabaseBackend::Postgres,
        &["ADD CONSTRAINT \"chk_age\" CHECK (age > 0)"]
    )]
    #[case::add_constraint_check_named_mysql(
        "add_constraint_check_named_mysql",
        DatabaseBackend::MySql,
        &["ADD CONSTRAINT `chk_age` CHECK (age > 0)"]
    )]
    #[case::add_constraint_check_named_sqlite(
        "add_constraint_check_named_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    fn test_add_constraint(
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
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
            }
        } else {
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            }
        };

        // For SQLite, we need to provide current schema
        let current_schema = vec![TableDef {
            name: "users".into(),
            columns: if title.contains("foreign_key") {
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
                        name: if title.contains("check") {
                            "age".into()
                        } else {
                            "email".into()
                        },
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: true,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    },
                ]
            },
            constraints: vec![],
        }];

        let result = build_add_constraint(&backend, "users", &constraint, &current_schema).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("add_constraint_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }

    #[test]
    fn test_add_constraint_primary_key_sqlite_table_not_found() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        };
        let current_schema = vec![]; // Empty schema - table not found
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'users' not found in current schema"));
    }

    #[test]
    fn test_add_constraint_primary_key_sqlite_with_check_constraints() {
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
            constraints: vec![TableConstraint::Check {
                name: "chk_id".into(),
                expr: "id > 0".into(),
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should include CHECK constraint in CREATE TABLE
        assert!(sql.contains("CONSTRAINT \"chk_id\" CHECK"));
    }

    #[test]
    fn test_add_constraint_primary_key_sqlite_with_indexes() {
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
            constraints: vec![TableConstraint::Index {
                name: Some("idx_id".into()),
                columns: vec!["id".into()],
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should recreate index
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_id"));
    }

    #[test]
    fn test_add_constraint_primary_key_sqlite_with_unique_constraint() {
        // Note: Unique indexes are now TableConstraint::Unique, not Index
        // Index constraints don't have a unique flag - use Unique constraint instead
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
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Unique constraint should be in CREATE TABLE statement (for SQLite temp table approach)
        assert!(sql.contains("CREATE TABLE"));
    }

    #[test]
    fn test_add_constraint_foreign_key_sqlite_table_not_found() {
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let current_schema = vec![]; // Empty schema - table not found
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "posts",
            &constraint,
            &current_schema,
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'posts' not found in current schema"));
    }

    #[test]
    fn test_add_constraint_foreign_key_sqlite_with_check_constraints() {
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
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::Check {
                name: "chk_user_id".into(),
                expr: "user_id > 0".into(),
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "posts",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should include CHECK constraint in CREATE TABLE
        assert!(sql.contains("CONSTRAINT \"chk_user_id\" CHECK"));
    }

    #[test]
    fn test_add_constraint_foreign_key_sqlite_with_indexes() {
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
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::Index {
                name: Some("idx_user_id".into()),
                columns: vec!["user_id".into()],
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "posts",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should recreate index
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_user_id"));
    }

    #[test]
    fn test_add_constraint_foreign_key_sqlite_with_unique_constraint() {
        // Note: Unique indexes are now TableConstraint::Unique
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
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_user_id".into()),
                columns: vec!["user_id".into()],
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "posts",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Unique constraint should be in CREATE TABLE statement
        assert!(sql.contains("CREATE TABLE"));
    }

    #[test]
    fn test_add_constraint_check_sqlite_table_not_found() {
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![]; // Empty schema - table not found
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'users' not found in current schema"));
    }

    #[test]
    fn test_add_constraint_check_sqlite_without_existing_check() {
        // Test when there are no existing CHECK constraints (line 376)
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            columns: vec![ColumnDef {
                name: "age".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![], // No existing CHECK constraints
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should create table with CHECK constraint
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("CONSTRAINT \"chk_age\" CHECK"));
    }

    #[test]
    fn test_add_constraint_primary_key_sqlite_without_existing_check() {
        // Test PrimaryKey addition when there are no existing CHECK constraints (line 84)
        // This should hit the else branch: BuiltQuery::CreateTable(Box::new(create_temp_table))
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![], // No existing CHECK constraints
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should create table without CHECK constraints (using BuiltQuery::CreateTable)
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("PRIMARY KEY"));
    }

    #[test]
    fn test_add_constraint_foreign_key_sqlite_without_existing_check() {
        // Test ForeignKey addition when there are no existing CHECK constraints (line 238)
        // This should hit the else branch: BuiltQuery::CreateTable(Box::new(create_temp_table))
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
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![], // No existing CHECK constraints
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "posts",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should create table without CHECK constraints (using BuiltQuery::CreateTable)
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("FOREIGN KEY"));
    }

    #[test]
    fn test_add_constraint_check_sqlite_with_indexes() {
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            columns: vec![ColumnDef {
                name: "age".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::Index {
                name: Some("idx_age".into()),
                columns: vec!["age".into()],
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Should recreate index
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_age"));
    }

    #[test]
    fn test_add_constraint_check_sqlite_with_unique_constraint() {
        // Note: Unique indexes are now TableConstraint::Unique
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            columns: vec![ColumnDef {
                name: "age".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_age".into()),
                columns: vec!["age".into()],
            }],
        }];
        let result = build_add_constraint(
            &DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        // Unique constraint should be in CREATE TABLE statement
        assert!(sql.contains("CREATE TABLE"));
    }

    #[test]
    fn test_extract_check_clauses_with_mixed_constraints() {
        // Test that extract_check_clauses filters out non-Check constraints
        let constraints = vec![
            TableConstraint::Check {
                name: "chk1".into(),
                expr: "a > 0".into(),
            },
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
            },
            TableConstraint::Check {
                name: "chk2".into(),
                expr: "b < 100".into(),
            },
            TableConstraint::Unique {
                name: Some("uq".into()),
                columns: vec!["email".into()],
            },
        ];
        let clauses = extract_check_clauses(&constraints);
        assert_eq!(clauses.len(), 2);
        assert!(clauses[0].contains("chk1"));
        assert!(clauses[1].contains("chk2"));
    }

    #[test]
    fn test_extract_check_clauses_with_no_check_constraints() {
        let constraints = vec![
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        ];
        let clauses = extract_check_clauses(&constraints);
        assert!(clauses.is_empty());
    }

    #[test]
    fn test_build_create_with_checks_empty_clauses() {
        use super::build_create_table_for_backend;

        let create_stmt = build_create_table_for_backend(
            &DatabaseBackend::Sqlite,
            "test_table",
            &[ColumnDef {
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
            &[],
        );

        // Empty check_clauses should return CreateTable variant
        let result = build_create_with_checks(&DatabaseBackend::Sqlite, &create_stmt, &[]);
        let sql = result.build(DatabaseBackend::Sqlite);
        assert!(sql.contains("CREATE TABLE"));
    }

    #[test]
    fn test_build_create_with_checks_with_clauses() {
        use super::build_create_table_for_backend;

        let create_stmt = build_create_table_for_backend(
            &DatabaseBackend::Sqlite,
            "test_table",
            &[ColumnDef {
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
            &[],
        );

        // Non-empty check_clauses should return Raw variant with embedded checks
        let check_clauses = vec!["CONSTRAINT \"chk1\" CHECK (id > 0)".to_string()];
        let result =
            build_create_with_checks(&DatabaseBackend::Sqlite, &create_stmt, &check_clauses);
        let sql = result.build(DatabaseBackend::Sqlite);
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("CONSTRAINT \"chk1\" CHECK (id > 0)"));
    }
}
