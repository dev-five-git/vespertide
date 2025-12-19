pub mod add_column;
pub mod add_constraint;
pub mod create_table;
pub mod delete_column;
pub mod delete_table;
pub mod helpers;
pub mod modify_column_type;
pub mod raw_sql;
pub mod remove_constraint;
pub mod rename_column;
pub mod rename_table;
pub mod types;

pub use helpers::*;
pub use types::{BuiltQuery, DatabaseBackend, RawSql};

use crate::error::QueryError;
use vespertide_core::{MigrationAction, TableDef};

use self::{
    add_column::build_add_column, add_constraint::build_add_constraint,
    create_table::build_create_table, delete_column::build_delete_column,
    delete_table::build_delete_table, modify_column_type::build_modify_column_type,
    raw_sql::build_raw_sql, remove_constraint::build_remove_constraint,
    rename_column::build_rename_column, rename_table::build_rename_table,
};

pub fn build_action_queries(
    backend: &DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
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
            Ok(build_delete_column(table, column, column_type))
        }

        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => build_modify_column_type(backend, table, column, new_type, current_schema),

        MigrationAction::RenameTable { from, to } => Ok(vec![build_rename_table(from, to)]),

        MigrationAction::RawSql { sql } => Ok(vec![build_raw_sql(sql.clone())]),

        MigrationAction::AddConstraint { table, constraint } => {
            build_add_constraint(backend, table, constraint, current_schema)
        }

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
}
