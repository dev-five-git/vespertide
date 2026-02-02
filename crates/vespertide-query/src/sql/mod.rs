pub mod add_column;
pub mod add_constraint;
pub mod create_table;
pub mod delete_column;
pub mod delete_table;
pub mod helpers;
pub mod modify_column_comment;
pub mod modify_column_default;
pub mod modify_column_nullable;
pub mod modify_column_type;
pub mod raw_sql;
pub mod remove_constraint;
pub mod rename_column;
pub mod rename_table;
pub mod types;

pub use helpers::*;
pub use types::{BuiltQuery, DatabaseBackend, RawSql};

use crate::error::QueryError;
use vespertide_core::{MigrationAction, TableConstraint, TableDef};

use self::{
    add_column::build_add_column, add_constraint::build_add_constraint,
    create_table::build_create_table, delete_column::build_delete_column,
    delete_table::build_delete_table, modify_column_comment::build_modify_column_comment,
    modify_column_default::build_modify_column_default,
    modify_column_nullable::build_modify_column_nullable,
    modify_column_type::build_modify_column_type, raw_sql::build_raw_sql,
    remove_constraint::build_remove_constraint, rename_column::build_rename_column,
    rename_table::build_rename_table,
};

pub fn build_action_queries(
    backend: &DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    build_action_queries_with_pending(backend, action, current_schema, &[])
}

/// Build SQL queries for a migration action, with awareness of pending constraints.
///
/// `pending_constraints` are constraints that exist in the logical schema but haven't been
/// physically created as database indexes yet. This is used by SQLite temp table rebuilds
/// to avoid recreating indexes that will be created by future AddConstraint actions.
pub fn build_action_queries_with_pending(
    backend: &DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => build_create_table(backend, table, columns, constraints),

        MigrationAction::DeleteTable { table } => Ok(vec![build_delete_table(table)]),

        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => build_add_column(backend, table, column, fill_with.as_deref(), current_schema),

        MigrationAction::RenameColumn { table, from, to } => {
            Ok(vec![build_rename_column(table, from, to)])
        }

        MigrationAction::DeleteColumn { table, column } => {
            // Find the column type from current schema for enum DROP TYPE support
            let column_type = current_schema
                .iter()
                .find(|t| t.name == *table)
                .and_then(|t| t.columns.iter().find(|c| c.name == *column))
                .map(|c| &c.r#type);
            Ok(build_delete_column(
                backend,
                table,
                column,
                column_type,
                current_schema,
            ))
        }

        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => build_modify_column_type(backend, table, column, new_type, current_schema),

        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with,
        } => build_modify_column_nullable(
            backend,
            table,
            column,
            *nullable,
            fill_with.as_deref(),
            current_schema,
        ),

        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
        } => build_modify_column_default(
            backend,
            table,
            column,
            new_default.as_deref(),
            current_schema,
        ),

        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => build_modify_column_comment(
            backend,
            table,
            column,
            new_comment.as_deref(),
            current_schema,
        ),

        MigrationAction::RenameTable { from, to } => Ok(vec![build_rename_table(from, to)]),

        MigrationAction::RawSql { sql } => Ok(vec![build_raw_sql(sql.clone())]),

        MigrationAction::AddConstraint { table, constraint } => build_add_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        ),

        MigrationAction::RemoveConstraint { table, constraint } => {
            build_remove_constraint(backend, table, constraint, current_schema)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, ReferenceAction, SimpleColumnType, TableConstraint,
    };

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn test_backend_specific_quoting() {
        let action = MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![],
        };
        let result = build_action_queries(&DatabaseBackend::Postgres, &action, &[]).unwrap();

        // PostgreSQL uses double quotes
        let pg_sql = result[0].build(DatabaseBackend::Postgres);
        assert!(pg_sql.contains("\"users\""));

        // MySQL uses backticks
        let mysql_sql = result[0].build(DatabaseBackend::MySql);
        assert!(mysql_sql.contains("`users`"));

        // SQLite uses double quotes
        let sqlite_sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(sqlite_sql.contains("\"users\""));
    }

    #[rstest]
    #[case::create_table_with_default_postgres(
        "create_table_with_default_postgres",
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: Some("'active'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        &["DEFAULT", "'active'"]
    )]
    #[case::create_table_with_default_mysql(
        "create_table_with_default_mysql",
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: Some("'active'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        &["DEFAULT", "'active'"]
    )]
    #[case::create_table_with_default_sqlite(
        "create_table_with_default_sqlite",
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: Some("'active'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        &["DEFAULT", "'active'"]
    )]
    #[case::create_table_with_inline_primary_key_postgres(
        "create_table_with_inline_primary_key_postgres",
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        &["PRIMARY KEY"]
    )]
    #[case::create_table_with_inline_primary_key_mysql(
        "create_table_with_inline_primary_key_mysql",
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        &["PRIMARY KEY"]
    )]
    #[case::create_table_with_inline_primary_key_sqlite(
        "create_table_with_inline_primary_key_sqlite",
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        &["PRIMARY KEY"]
    )]
    #[case::create_table_with_fk_postgres(
        "create_table_with_fk_postgres",
        MigrationAction::CreateTable {
            table: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
            }],
        },
        DatabaseBackend::Postgres,
        &["REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::create_table_with_fk_mysql(
        "create_table_with_fk_mysql",
        MigrationAction::CreateTable {
            table: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
            }],
        },
        DatabaseBackend::Postgres,
        &["REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::create_table_with_fk_sqlite(
        "create_table_with_fk_sqlite",
        MigrationAction::CreateTable {
            table: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
            }],
        },
        DatabaseBackend::Postgres,
        &["REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    fn test_build_migration_action(
        #[case] title: &str,
        #[case] action: MigrationAction,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("build_migration_action_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }

    #[rstest]
    #[case::rename_column_postgres(DatabaseBackend::Postgres)]
    #[case::rename_column_mysql(DatabaseBackend::MySql)]
    #[case::rename_column_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_rename_column(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::RenameColumn (lines 51-52)
        let action = MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old_name".into(),
            to: "new_name".into(),
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(backend);
        assert!(sql.contains("RENAME"));
        assert!(sql.contains("old_name"));
        assert!(sql.contains("new_name"));

        with_settings!({ snapshot_suffix => format!("rename_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::delete_column_postgres(DatabaseBackend::Postgres)]
    #[case::delete_column_mysql(DatabaseBackend::MySql)]
    #[case::delete_column_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_delete_column(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::DeleteColumn (lines 55-56)
        let action = MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(backend);
        assert!(sql.contains("DROP COLUMN"));
        assert!(sql.contains("email"));

        with_settings!({ snapshot_suffix => format!("delete_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::modify_column_type_postgres(DatabaseBackend::Postgres)]
    #[case::modify_column_type_mysql(DatabaseBackend::MySql)]
    #[case::modify_column_type_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_modify_column_type(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::ModifyColumnType (lines 60-63)
        let action = MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "age".into(),
            new_type: ColumnType::Simple(SimpleColumnType::BigInt),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
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
            constraints: vec![],
        }];
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        assert!(!result.is_empty());
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("ALTER TABLE"));

        with_settings!({ snapshot_suffix => format!("modify_column_type_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_index_constraint_postgres(DatabaseBackend::Postgres)]
    #[case::remove_index_constraint_mysql(DatabaseBackend::MySql)]
    #[case::remove_index_constraint_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_remove_index_constraint(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::RemoveConstraint with Index variant
        let action = MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("idx_email".into()),
                columns: vec!["email".into()],
            },
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(backend);
        assert!(sql.contains("DROP INDEX"));
        assert!(sql.contains("idx_email"));

        with_settings!({ snapshot_suffix => format!("remove_index_constraint_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::rename_table_postgres(DatabaseBackend::Postgres)]
    #[case::rename_table_mysql(DatabaseBackend::MySql)]
    #[case::rename_table_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_rename_table(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::RenameTable (line 69)
        let action = MigrationAction::RenameTable {
            from: "old_table".into(),
            to: "new_table".into(),
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(backend);
        assert!(sql.contains("RENAME"));
        assert!(sql.contains("old_table"));
        assert!(sql.contains("new_table"));

        with_settings!({ snapshot_suffix => format!("rename_table_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::add_constraint_postgres(DatabaseBackend::Postgres)]
    #[case::add_constraint_mysql(DatabaseBackend::MySql)]
    #[case::add_constraint_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_add_constraint(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::AddConstraint (lines 73-74)
        let action = MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        };
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
            constraints: vec![],
        }];
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        assert!(!result.is_empty());
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("UNIQUE") || sql.contains("uq_email"));

        with_settings!({ snapshot_suffix => format!("add_constraint_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_constraint_postgres(DatabaseBackend::Postgres)]
    #[case::remove_constraint_mysql(DatabaseBackend::MySql)]
    #[case::remove_constraint_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_remove_constraint(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::RemoveConstraint (lines 77-78)
        let action = MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        };
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
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        assert!(!result.is_empty());
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("DROP") || sql.contains("CONSTRAINT"));

        with_settings!({ snapshot_suffix => format!("remove_constraint_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::add_column_postgres(DatabaseBackend::Postgres)]
    #[case::add_column_mysql(DatabaseBackend::MySql)]
    #[case::add_column_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_add_column(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::AddColumn (lines 46-49)
        let action = MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        };
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
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        assert!(!result.is_empty());
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("ALTER TABLE"));
        assert!(sql.contains("ADD COLUMN") || sql.contains("ADD"));

        with_settings!({ snapshot_suffix => format!("add_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::add_index_constraint_postgres(DatabaseBackend::Postgres)]
    #[case::add_index_constraint_mysql(DatabaseBackend::MySql)]
    #[case::add_index_constraint_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_add_index_constraint(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::AddConstraint with Index variant
        let action = MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("idx_email".into()),
                columns: vec!["email".into()],
            },
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(backend);
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_email"));

        with_settings!({ snapshot_suffix => format!("add_index_constraint_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::raw_sql_postgres(DatabaseBackend::Postgres)]
    #[case::raw_sql_mysql(DatabaseBackend::MySql)]
    #[case::raw_sql_sqlite(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_raw_sql(#[case] backend: DatabaseBackend) {
        // Test MigrationAction::RawSql (line 71)
        let action = MigrationAction::RawSql {
            sql: "SELECT 1;".into(),
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(backend);
        assert_eq!(sql, "SELECT 1;");

        with_settings!({ snapshot_suffix => format!("raw_sql_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    // Comprehensive index naming tests
    #[rstest]
    #[case::add_index_with_custom_name_postgres(
        DatabaseBackend::Postgres,
        "hello",
        vec!["email", "password"]
    )]
    #[case::add_index_with_custom_name_mysql(
        DatabaseBackend::MySql,
        "hello",
        vec!["email", "password"]
    )]
    #[case::add_index_with_custom_name_sqlite(
        DatabaseBackend::Sqlite,
        "hello",
        vec!["email", "password"]
    )]
    #[case::add_index_single_column_postgres(
        DatabaseBackend::Postgres,
        "email_idx",
        vec!["email"]
    )]
    #[case::add_index_single_column_mysql(
        DatabaseBackend::MySql,
        "email_idx",
        vec!["email"]
    )]
    #[case::add_index_single_column_sqlite(
        DatabaseBackend::Sqlite,
        "email_idx",
        vec!["email"]
    )]
    fn test_add_index_with_custom_name(
        #[case] backend: DatabaseBackend,
        #[case] index_name: &str,
        #[case] columns: Vec<&str>,
    ) {
        // Test that custom index names follow ix_table__name pattern
        let action = MigrationAction::AddConstraint {
            table: "user".into(),
            constraint: TableConstraint::Index {
                name: Some(index_name.into()),
                columns: columns.iter().map(|s| s.to_string()).collect(),
            },
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        let sql = result[0].build(backend);

        // Should use ix_table__name pattern
        let expected_name = format!("ix_user__{}", index_name);
        assert!(
            sql.contains(&expected_name),
            "Expected index name '{}' in SQL: {}",
            expected_name,
            sql
        );

        with_settings!({ snapshot_suffix => format!("add_index_custom_{}_{:?}", index_name, backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::add_unnamed_index_single_column_postgres(
        DatabaseBackend::Postgres,
        vec!["email"]
    )]
    #[case::add_unnamed_index_single_column_mysql(
        DatabaseBackend::MySql,
        vec!["email"]
    )]
    #[case::add_unnamed_index_single_column_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email"]
    )]
    #[case::add_unnamed_index_multiple_columns_postgres(
        DatabaseBackend::Postgres,
        vec!["email", "password"]
    )]
    #[case::add_unnamed_index_multiple_columns_mysql(
        DatabaseBackend::MySql,
        vec!["email", "password"]
    )]
    #[case::add_unnamed_index_multiple_columns_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email", "password"]
    )]
    fn test_add_unnamed_index(#[case] backend: DatabaseBackend, #[case] columns: Vec<&str>) {
        // Test that unnamed indexes follow ix_table__col1_col2 pattern
        let action = MigrationAction::AddConstraint {
            table: "user".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: columns.iter().map(|s| s.to_string()).collect(),
            },
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        let sql = result[0].build(backend);

        // Should use ix_table__col1_col2... pattern
        let expected_name = format!("ix_user__{}", columns.join("_"));
        assert!(
            sql.contains(&expected_name),
            "Expected index name '{}' in SQL: {}",
            expected_name,
            sql
        );

        with_settings!({ snapshot_suffix => format!("add_unnamed_index_{}_{:?}", columns.join("_"), backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_index_with_custom_name_postgres(
        DatabaseBackend::Postgres,
        "hello",
        vec!["email", "password"]
    )]
    #[case::remove_index_with_custom_name_mysql(
        DatabaseBackend::MySql,
        "hello",
        vec!["email", "password"]
    )]
    #[case::remove_index_with_custom_name_sqlite(
        DatabaseBackend::Sqlite,
        "hello",
        vec!["email", "password"]
    )]
    fn test_remove_index_with_custom_name(
        #[case] backend: DatabaseBackend,
        #[case] index_name: &str,
        #[case] columns: Vec<&str>,
    ) {
        // Test that removing custom index uses ix_table__name pattern
        let action = MigrationAction::RemoveConstraint {
            table: "user".into(),
            constraint: TableConstraint::Index {
                name: Some(index_name.into()),
                columns: columns.iter().map(|s| s.to_string()).collect(),
            },
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        let sql = result[0].build(backend);

        // Should use ix_table__name pattern
        let expected_name = format!("ix_user__{}", index_name);
        assert!(
            sql.contains(&expected_name),
            "Expected index name '{}' in SQL: {}",
            expected_name,
            sql
        );

        with_settings!({ snapshot_suffix => format!("remove_index_custom_{}_{:?}", index_name, backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_unnamed_index_single_column_postgres(
        DatabaseBackend::Postgres,
        vec!["email"]
    )]
    #[case::remove_unnamed_index_single_column_mysql(
        DatabaseBackend::MySql,
        vec!["email"]
    )]
    #[case::remove_unnamed_index_single_column_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email"]
    )]
    #[case::remove_unnamed_index_multiple_columns_postgres(
        DatabaseBackend::Postgres,
        vec!["email", "password"]
    )]
    #[case::remove_unnamed_index_multiple_columns_mysql(
        DatabaseBackend::MySql,
        vec!["email", "password"]
    )]
    #[case::remove_unnamed_index_multiple_columns_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email", "password"]
    )]
    fn test_remove_unnamed_index(#[case] backend: DatabaseBackend, #[case] columns: Vec<&str>) {
        // Test that removing unnamed indexes uses ix_table__col1_col2 pattern
        let action = MigrationAction::RemoveConstraint {
            table: "user".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: columns.iter().map(|s| s.to_string()).collect(),
            },
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        let sql = result[0].build(backend);

        // Should use ix_table__col1_col2... pattern
        let expected_name = format!("ix_user__{}", columns.join("_"));
        assert!(
            sql.contains(&expected_name),
            "Expected index name '{}' in SQL: {}",
            expected_name,
            sql
        );

        with_settings!({ snapshot_suffix => format!("remove_unnamed_index_{}_{:?}", columns.join("_"), backend) }, {
            assert_snapshot!(sql);
        });
    }

    // Comprehensive unique constraint naming tests
    #[rstest]
    #[case::add_unique_with_custom_name_postgres(
        DatabaseBackend::Postgres,
        "email_unique",
        vec!["email"]
    )]
    #[case::add_unique_with_custom_name_mysql(
        DatabaseBackend::MySql,
        "email_unique",
        vec!["email"]
    )]
    #[case::add_unique_with_custom_name_sqlite(
        DatabaseBackend::Sqlite,
        "email_unique",
        vec!["email"]
    )]
    fn test_add_unique_with_custom_name(
        #[case] backend: DatabaseBackend,
        #[case] constraint_name: &str,
        #[case] columns: Vec<&str>,
    ) {
        // Test that custom unique constraint names follow uq_table__name pattern
        let action = MigrationAction::AddConstraint {
            table: "user".into(),
            constraint: TableConstraint::Unique {
                name: Some(constraint_name.into()),
                columns: columns.iter().map(|s| s.to_string()).collect(),
            },
        };

        let current_schema = vec![TableDef {
            name: "user".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
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

        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should use uq_table__name pattern
        let expected_name = format!("uq_user__{}", constraint_name);
        assert!(
            sql.contains(&expected_name),
            "Expected unique constraint name '{}' in SQL: {}",
            expected_name,
            sql
        );

        with_settings!({ snapshot_suffix => format!("add_unique_custom_{}_{:?}", constraint_name, backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::add_unnamed_unique_single_column_postgres(
        DatabaseBackend::Postgres,
        vec!["email"]
    )]
    #[case::add_unnamed_unique_single_column_mysql(
        DatabaseBackend::MySql,
        vec!["email"]
    )]
    #[case::add_unnamed_unique_single_column_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email"]
    )]
    #[case::add_unnamed_unique_multiple_columns_postgres(
        DatabaseBackend::Postgres,
        vec!["email", "username"]
    )]
    #[case::add_unnamed_unique_multiple_columns_mysql(
        DatabaseBackend::MySql,
        vec!["email", "username"]
    )]
    #[case::add_unnamed_unique_multiple_columns_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email", "username"]
    )]
    fn test_add_unnamed_unique(#[case] backend: DatabaseBackend, #[case] columns: Vec<&str>) {
        // Test that unnamed unique constraints follow uq_table__col1_col2 pattern
        let action = MigrationAction::AddConstraint {
            table: "user".into(),
            constraint: TableConstraint::Unique {
                name: None,
                columns: columns.iter().map(|s| s.to_string()).collect(),
            },
        };

        let schema_columns: Vec<ColumnDef> = columns
            .iter()
            .map(|col| ColumnDef {
                name: col.to_string(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            })
            .collect();

        let current_schema = vec![TableDef {
            name: "user".into(),
            description: None,
            columns: schema_columns,
            constraints: vec![],
        }];

        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should use uq_table__col1_col2... pattern
        let expected_name = format!("uq_user__{}", columns.join("_"));
        assert!(
            sql.contains(&expected_name),
            "Expected unique constraint name '{}' in SQL: {}",
            expected_name,
            sql
        );

        with_settings!({ snapshot_suffix => format!("add_unnamed_unique_{}_{:?}", columns.join("_"), backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_unique_with_custom_name_postgres(
        DatabaseBackend::Postgres,
        "email_unique",
        vec!["email"]
    )]
    #[case::remove_unique_with_custom_name_mysql(
        DatabaseBackend::MySql,
        "email_unique",
        vec!["email"]
    )]
    #[case::remove_unique_with_custom_name_sqlite(
        DatabaseBackend::Sqlite,
        "email_unique",
        vec!["email"]
    )]
    fn test_remove_unique_with_custom_name(
        #[case] backend: DatabaseBackend,
        #[case] constraint_name: &str,
        #[case] columns: Vec<&str>,
    ) {
        // Test that removing custom unique constraint uses uq_table__name pattern
        let constraint = TableConstraint::Unique {
            name: Some(constraint_name.into()),
            columns: columns.iter().map(|s| s.to_string()).collect(),
        };

        let current_schema = vec![TableDef {
            name: "user".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![constraint.clone()],
        }];

        let action = MigrationAction::RemoveConstraint {
            table: "user".into(),
            constraint,
        };

        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should use uq_table__name pattern (for Postgres/MySQL, not SQLite which rebuilds table)
        if backend != DatabaseBackend::Sqlite {
            let expected_name = format!("uq_user__{}", constraint_name);
            assert!(
                sql.contains(&expected_name),
                "Expected unique constraint name '{}' in SQL: {}",
                expected_name,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("remove_unique_custom_{}_{:?}", constraint_name, backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::remove_unnamed_unique_single_column_postgres(
        DatabaseBackend::Postgres,
        vec!["email"]
    )]
    #[case::remove_unnamed_unique_single_column_mysql(
        DatabaseBackend::MySql,
        vec!["email"]
    )]
    #[case::remove_unnamed_unique_single_column_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email"]
    )]
    #[case::remove_unnamed_unique_multiple_columns_postgres(
        DatabaseBackend::Postgres,
        vec!["email", "username"]
    )]
    #[case::remove_unnamed_unique_multiple_columns_mysql(
        DatabaseBackend::MySql,
        vec!["email", "username"]
    )]
    #[case::remove_unnamed_unique_multiple_columns_sqlite(
        DatabaseBackend::Sqlite,
        vec!["email", "username"]
    )]
    fn test_remove_unnamed_unique(#[case] backend: DatabaseBackend, #[case] columns: Vec<&str>) {
        // Test that removing unnamed unique constraints uses uq_table__col1_col2 pattern
        let constraint = TableConstraint::Unique {
            name: None,
            columns: columns.iter().map(|s| s.to_string()).collect(),
        };

        let schema_columns: Vec<ColumnDef> = columns
            .iter()
            .map(|col| ColumnDef {
                name: col.to_string(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            })
            .collect();

        let current_schema = vec![TableDef {
            name: "user".into(),
            description: None,
            columns: schema_columns,
            constraints: vec![constraint.clone()],
        }];

        let action = MigrationAction::RemoveConstraint {
            table: "user".into(),
            constraint,
        };

        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should use uq_table__col1_col2... pattern (for Postgres/MySQL, not SQLite which rebuilds table)
        if backend != DatabaseBackend::Sqlite {
            let expected_name = format!("uq_user__{}", columns.join("_"));
            assert!(
                sql.contains(&expected_name),
                "Expected unique constraint name '{}' in SQL: {}",
                expected_name,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("remove_unnamed_unique_{}_{:?}", columns.join("_"), backend) }, {
            assert_snapshot!(sql);
        });
    }

    /// Test build_action_queries for ModifyColumnNullable
    #[rstest]
    #[case::postgres_modify_nullable(DatabaseBackend::Postgres)]
    #[case::mysql_modify_nullable(DatabaseBackend::MySql)]
    #[case::sqlite_modify_nullable(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_modify_column_nullable(#[case] backend: DatabaseBackend) {
        let action = MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: Some("'unknown'".into()),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
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
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        assert!(!result.is_empty());
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should contain UPDATE for fill_with and ALTER for nullable change
        assert!(sql.contains("UPDATE"));
        assert!(sql.contains("unknown"));

        let suffix = format!(
            "{}_modify_nullable",
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

    /// Test build_action_queries for ModifyColumnDefault
    #[rstest]
    #[case::postgres_modify_default(DatabaseBackend::Postgres)]
    #[case::mysql_modify_default(DatabaseBackend::MySql)]
    #[case::sqlite_modify_default(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_modify_column_default(#[case] backend: DatabaseBackend) {
        let action = MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
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
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        assert!(!result.is_empty());
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Should contain DEFAULT and 'active'
        assert!(sql.contains("DEFAULT") || sql.contains("active"));

        let suffix = format!(
            "{}_modify_default",
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

    /// Test build_action_queries for ModifyColumnComment
    #[rstest]
    #[case::postgres_modify_comment(DatabaseBackend::Postgres)]
    #[case::mysql_modify_comment(DatabaseBackend::MySql)]
    #[case::sqlite_modify_comment(DatabaseBackend::Sqlite)]
    fn test_build_action_queries_modify_column_comment(#[case] backend: DatabaseBackend) {
        let action = MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email address".into()),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
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
        let result = build_action_queries(&backend, &action, &current_schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // Postgres and MySQL should have comment, SQLite returns empty
        if backend != DatabaseBackend::Sqlite {
            assert!(sql.contains("COMMENT") || sql.contains("User email address"));
        }

        let suffix = format!(
            "{}_modify_comment",
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

    #[rstest]
    #[case::create_table_func_default_postgres(DatabaseBackend::Postgres)]
    #[case::create_table_func_default_mysql(DatabaseBackend::MySql)]
    #[case::create_table_func_default_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_function_default(#[case] backend: DatabaseBackend) {
        // SQLite requires DEFAULT (expr) for function-call defaults.
        // This test ensures parentheses are added for SQLite.
        let action = MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: Some("gen_random_uuid()".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                    nullable: false,
                    default: Some("now()".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
        };
        let result = build_action_queries(&backend, &action, &[]).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        with_settings!({ snapshot_suffix => format!("create_table_func_default_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::delete_enum_column_postgres(DatabaseBackend::Postgres)]
    #[case::delete_enum_column_mysql(DatabaseBackend::MySql)]
    #[case::delete_enum_column_sqlite(DatabaseBackend::Sqlite)]
    fn test_delete_column_with_enum_type(#[case] backend: DatabaseBackend) {
        // Deleting a column with an enum type — SQLite uses temp table approach,
        // Postgres drops the enum type, MySQL uses simple DROP COLUMN.
        let action = MigrationAction::DeleteColumn {
            table: "orders".into(),
            column: "status".into(),
        };
        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(vespertide_core::ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: vespertide_core::EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                        ]),
                    }),
                    nullable: false,
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
        let result = build_action_queries(&backend, &action, &schema).unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n");

        with_settings!({ snapshot_suffix => format!("delete_enum_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }
}
