use super::*;
use vespertide_core::TableDef;

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
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
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

    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");

    // Should use uq_table__name pattern
    let expected_name = format!("uq_user__{constraint_name}");
    assert!(
        sql.contains(&expected_name),
        "Expected unique constraint name '{expected_name}' in SQL: {sql}"
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_unique_custom_{}_{:?}", constraint_name, backend) }, {
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
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        },
    };

    let schema_columns: Vec<ColumnDef> = columns
        .iter()
        .map(|col| ColumnDef::new(*col, ColumnType::Simple(SimpleColumnType::Text), true))
        .collect();

    let current_schema = vec![TableDef {
        name: "user".into(),
        description: None,
        columns: schema_columns,
        constraints: vec![],
    }];

    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");

    // Should use uq_table__col1_col2... pattern
    let expected_name = format!("uq_user__{}", columns.join("_"));
    assert!(
        sql.contains(&expected_name),
        "Expected unique constraint name '{expected_name}' in SQL: {sql}"
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_unnamed_unique_{}_{:?}", columns.join("_"), backend) }, {
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
        columns: columns.iter().copied().map(Into::into).collect(),
        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First,
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
        constraints: vec![constraint.clone()],
    }];

    let action = MigrationAction::RemoveConstraint {
        table: "user".into(),
        constraint,
    };

    let result = build_action_queries(backend, &action, &current_schema).unwrap();
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n");

    // Should use uq_table__name pattern (for Postgres/MySQL, not SQLite which rebuilds table)
    if backend != DatabaseBackend::Sqlite {
        let expected_name = format!("uq_user__{constraint_name}");
        assert!(
            sql.contains(&expected_name),
            "Expected unique constraint name '{expected_name}' in SQL: {sql}"
        );
    }

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_unique_custom_{}_{:?}", constraint_name, backend) }, {
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
        columns: columns.iter().copied().map(Into::into).collect(),
        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First,
        },
    };

    let schema_columns: Vec<ColumnDef> = columns
        .iter()
        .map(|col| ColumnDef::new(*col, ColumnType::Simple(SimpleColumnType::Text), true))
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

    let result = build_action_queries(backend, &action, &current_schema).unwrap();
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
            "Expected unique constraint name '{expected_name}' in SQL: {sql}"
        );
    }

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_unnamed_unique_{}_{:?}", columns.join("_"), backend) }, {
        assert_snapshot!(sql);
    });
}

/// Test `build_action_queries` for `ModifyColumnNullable`
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
        delete_null_rows: None,
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
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
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

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => suffix }, {
        assert_snapshot!(sql);
    });
}

/// Test `build_action_queries` for `ModifyColumnDefault`
#[rstest]
#[case::postgres_modify_default(DatabaseBackend::Postgres)]
#[case::mysql_modify_default(DatabaseBackend::MySql)]
#[case::sqlite_modify_default(DatabaseBackend::Sqlite)]
fn test_build_action_queries_modify_column_default(#[case] backend: DatabaseBackend) {
    let action = MigrationAction::ModifyColumnDefault {
        table: "users".into(),
        column: "status".into(),
        new_default: Some("'active'".into()),
        backfill: None,
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
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
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

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => suffix }, {
        assert_snapshot!(sql);
    });
}

/// Test `build_action_queries` for `ModifyColumnComment`
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
    let result = build_action_queries(backend, &action, &current_schema).unwrap();
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

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => suffix }, {
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
    let result = build_action_queries(backend, &action, &[]).unwrap();
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<_>>()
        .join(";\n");

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("create_table_func_default_{:?}", backend) }, {
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
    let result = build_action_queries(backend, &action, &schema).unwrap();
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<_>>()
        .join(";\n");

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("delete_enum_column_{:?}", backend) }, {
        assert_snapshot!(sql);
    });
}

#[rstest]
#[case::replace_fk_constraint_postgres(DatabaseBackend::Postgres)]
#[case::replace_fk_constraint_mysql(DatabaseBackend::MySql)]
#[case::replace_fk_constraint_sqlite(DatabaseBackend::Sqlite)]
fn test_replace_fk_constraint(#[case] backend: DatabaseBackend) {
    let schema = vec![TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    }];
    let action = MigrationAction::ReplaceConstraint {
        table: "posts".into(),
        from: TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
        to: TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    };
    let result = build_action_queries(backend, &action, &schema).unwrap();
    let sql = result
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<_>>()
        .join(";\n");

    let suffix = format!(
        "replace_fk_constraint_{}",
        match backend {
            DatabaseBackend::Postgres => "postgres",
            DatabaseBackend::MySql => "mysql",
            DatabaseBackend::Sqlite => "sqlite",
        }
    );

    with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => suffix }, {
        assert_snapshot!(sql);
    });
}
