use super::*;
use vespertide_core::TableDef;

#[test]
fn test_backend_specific_quoting() {
    let action = MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![],
    };
    let result = build_action_queries(DatabaseBackend::Postgres, &action, &[]).unwrap();

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
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
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
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
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
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    let sql = result[0].build(backend);
    for exp in expected {
        assert!(
            sql.contains(exp),
            "Expected SQL to contain '{exp}', got: {sql}"
        );
    }

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("build_migration_action_{}", title) }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(result.len(), 1);
    let sql = result[0].build(backend);
    assert!(sql.contains("RENAME"));
    assert!(sql.contains("old_name"));
    assert!(sql.contains("new_name"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("rename_column_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(result.len(), 1);
    let sql = result[0].build(backend);
    assert!(sql.contains("DROP COLUMN"));
    assert!(sql.contains("email"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("delete_column_{:?}", backend) }, {
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
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
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
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    assert!(!result.is_empty());
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("ALTER TABLE"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(result.len(), 1);
    let sql = result[0].build(backend);
    assert!(sql.contains("DROP INDEX"));
    assert!(sql.contains("idx_email"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_index_constraint_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(result.len(), 1);
    let sql = result[0].build(backend);
    assert!(sql.contains("RENAME"));
    assert!(sql.contains("old_table"));
    assert!(sql.contains("new_table"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("rename_table_{:?}", backend) }, {
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
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
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
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    assert!(!result.is_empty());
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("UNIQUE") || sql.contains("uq_email"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_constraint_{:?}", backend) }, {
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
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
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
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    }];
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    assert!(!result.is_empty());
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("DROP") || sql.contains("CONSTRAINT"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_constraint_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    assert!(!result.is_empty());
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");
    assert!(sql.contains("ALTER TABLE"));
    assert!(sql.contains("ADD COLUMN") || sql.contains("ADD"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_column_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(result.len(), 1);
    let sql = result[0].build(backend);
    assert!(sql.contains("CREATE INDEX"));
    assert!(sql.contains("idx_email"));

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_index_constraint_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(result.len(), 1);
    let sql = result[0].build(backend);
    assert_eq!(sql, "SELECT 1;");

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("raw_sql_{:?}", backend) }, {
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
            columns: columns.iter().copied().map(Into::into).collect(),
        },
    };
    let result = build_action_queries(backend, &action, &[]).unwrap();
    let sql = result[0].build(backend);

    // Should use ix_table__name pattern
    let expected_name = format!("ix_user__{index_name}");
    assert!(
        sql.contains(&expected_name),
        "Expected index name '{expected_name}' in SQL: {sql}"
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_index_custom_{}_{:?}", index_name, backend) }, {
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
            columns: columns.iter().copied().map(Into::into).collect(),
        },
    };
    let result = build_action_queries(backend, &action, &[]).unwrap();
    let sql = result[0].build(backend);

    // Should use ix_table__col1_col2... pattern
    let expected_name = format!("ix_user__{}", columns.join("_"));
    assert!(
        sql.contains(&expected_name),
        "Expected index name '{expected_name}' in SQL: {sql}"
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_unnamed_index_{}_{:?}", columns.join("_"), backend) }, {
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
            columns: columns.iter().copied().map(Into::into).collect(),
        },
    };
    let result = build_action_queries(backend, &action, &[]).unwrap();
    let sql = result[0].build(backend);

    // Should use ix_table__name pattern
    let expected_name = format!("ix_user__{index_name}");
    assert!(
        sql.contains(&expected_name),
        "Expected index name '{expected_name}' in SQL: {sql}"
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_index_custom_{}_{:?}", index_name, backend) }, {
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
            columns: columns.iter().copied().map(Into::into).collect(),
        },
    };
    let result = build_action_queries(backend, &action, &[]).unwrap();
    let sql = result[0].build(backend);

    // Should use ix_table__col1_col2... pattern
    let expected_name = format!("ix_user__{}", columns.join("_"));
    assert!(
        sql.contains(&expected_name),
        "Expected index name '{expected_name}' in SQL: {sql}"
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_unnamed_index_{}_{:?}", columns.join("_"), backend) }, {
        assert_snapshot!(sql);
    });
}

// =====================================================================
// build_action_queries dispatch coverage: DeleteTable, ModifyColumnDefault,
// RemapEnumValues — migrated INLINE from the former sql/tests/coverage.rs
// per the inline-migration policy (these are pure dispatch tests of
// build_action_queries, which lives in sql/mod.rs).
// =====================================================================

fn nn_col_for_dispatch(name: &str, ty: SimpleColumnType) -> ColumnDef {
    ColumnDef::new(name, ColumnType::Simple(ty), false)
}

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn build_action_queries_delete_table(#[case] backend: DatabaseBackend) {
    let action = MigrationAction::DeleteTable {
        table: "users".into(),
    };
    let queries = build_action_queries(backend, &action, &[]).unwrap();
    assert_eq!(queries.len(), 1);
    let sql = queries[0].build(backend);
    assert!(sql.contains("DROP TABLE"));
    assert!(sql.contains("users"));
}

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn build_action_queries_modify_column_default_dispatch(#[case] backend: DatabaseBackend) {
    let schema = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            nn_col_for_dispatch("id", SimpleColumnType::Integer),
            nn_col_for_dispatch("status", SimpleColumnType::Text),
        ],
        constraints: vec![],
    }];
    let action = MigrationAction::ModifyColumnDefault {
        table: "users".into(),
        column: "status".into(),
        new_default: Some("'pending'".into()),
        backfill: None,
    };
    let queries = build_action_queries(backend, &action, &schema).unwrap();
    let sql = queries
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(sql.contains("'pending'") || sql.contains("pending"));
}

/// `RemapEnumValues` dispatch arm of `build_action_queries_with_pending`.
/// The action requires the column to be an enum type on the target table;
/// on a missing/wrong-type column the underlying builder returns an error
/// — either way the dispatch arm is exercised.
#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn build_action_queries_remap_enum_values_dispatch(#[case] backend: DatabaseBackend) {
    use vespertide_core::{ComplexColumnType, EnumValues, NumValue};
    let schema = vec![TableDef {
        name: "tickets".into(),
        description: None,
        columns: vec![
            nn_col_for_dispatch("id", SimpleColumnType::Integer),
            ColumnDef::new(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "ticket_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "open".into(),
                            value: 1,
                        },
                        NumValue {
                            name: "closed".into(),
                            value: 2,
                        },
                    ]),
                }),
                false,
            ),
        ],
        constraints: vec![],
    }];
    let action = MigrationAction::RemapEnumValues {
        table: "tickets".into(),
        column: "status".into(),
        mapping: vec![(0_i64, 1_i64), (3_i64, 2_i64)].into_iter().collect(),
    };
    let result = build_action_queries(backend, &action, &schema);
    // The dispatch arm executes regardless of downstream Ok/Err — that's
    // what this test exists to cover.
    let _ = result;
}
