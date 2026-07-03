mod check;
mod foreign_key;
mod index;
mod primary_key;
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
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }
        } else if title.contains("unique") {
            TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }
        } else if title.contains("foreign_key") {
            TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            }
        } else {
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            }
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
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
        let result =
            build_add_constraint(backend, "users", &constraint, &current_schema, &[]).unwrap();
        // F3 introduced multi-statement output for FK additions (pre-cleanup +
        // ADD CONSTRAINT). Search across every emitted statement so the
        // assertion still locates the constraint SQL regardless of position.
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join("\n");
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{exp}', got: {sql}"
            );
        }
        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("add_constraint_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }
    #[test]
    fn test_add_constraint_primary_key_sqlite_table_not_found() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let current_schema = vec![]; // Empty schema - table not found
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'users' not found in current schema"));
    }

    #[test]
    fn add_check_constraint_escapes_adversarial_identifiers() {
        let constraint = TableConstraint::Check {
            name: "chk_age\"quote".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let current_schema = vec![TableDef {
            name: "users\"archive".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "age".into(),
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

        let pg_results = build_add_constraint(
            DatabaseBackend::Postgres,
            "users\"archive",
            &constraint,
            &current_schema,
            &[],
        )
        .unwrap();
        // F4 introduced a pre-cleanup statement and F11 split the PG path
        // into NOT VALID + VALIDATE. Validate identifier escaping appears in
        // both statements by joining the full result and asserting on every
        // distinct emitted form.
        let pg_sql = pg_results
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(pg_sql.contains("ALTER TABLE \"users\"\"archive\" ADD CONSTRAINT \"chk_age\"\"quote\" CHECK (age > 0) NOT VALID"), "PG NOT VALID statement missing or mis-escaped, got: {pg_sql}");
        assert!(
            pg_sql.contains(
                "ALTER TABLE \"users\"\"archive\" VALIDATE CONSTRAINT \"chk_age\"\"quote\""
            ),
            "PG VALIDATE statement missing or mis-escaped, got: {pg_sql}"
        );

        let mysql_constraint = TableConstraint::Check {
            name: "chk_age`quote".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let mysql_results = build_add_constraint(
            DatabaseBackend::MySql,
            "users`archive",
            &mysql_constraint,
            &current_schema,
            &[],
        )
        .unwrap();
        let mysql_sql = mysql_results
            .last()
            .expect("at least one query emitted")
            .build(DatabaseBackend::MySql);
        assert_eq!(
            mysql_sql,
            "ALTER TABLE `users``archive` ADD CONSTRAINT `chk_age``quote` CHECK (age > 0)"
        );
    }

    #[test]
    fn test_add_constraint_primary_key_sqlite_with_check_constraints() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
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
            constraints: vec![TableConstraint::Check {
                name: "chk_id".into(),
                expr: "id > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            }],
        }];
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CONSTRAINT \"chk_id\" CHECK"));
    }
    #[test]
    fn test_add_constraint_primary_key_sqlite_with_indexes() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
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
            constraints: vec![TableConstraint::Index {
                name: Some("idx_id".into()),
                columns: vec!["id".into()],
            }],
        }];
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_id"));
    }
    #[test]
    fn test_add_constraint_primary_key_sqlite_with_unique_constraint() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
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
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }],
        }];
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
    }
    #[test]
    fn test_add_constraint_check_sqlite_table_not_found() {
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let current_schema = vec![]; // Empty schema - table not found
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'users' not found in current schema"));
    }
    #[test]
    fn test_add_constraint_check_sqlite_without_existing_check() {
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
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
            constraints: vec![], // No existing CHECK constraints
        }];
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("CONSTRAINT \"chk_age\" CHECK"));
    }
    #[test]
    fn test_add_constraint_primary_key_sqlite_without_existing_check() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
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
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("PRIMARY KEY"));
    }

    #[test]
    fn test_add_constraint_check_sqlite_with_indexes() {
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
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
            constraints: vec![TableConstraint::Index {
                name: Some("idx_age".into()),
                columns: vec!["age".into()],
            }],
        }];
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_age"));
    }
    #[test]
    fn test_add_constraint_check_sqlite_with_unique_constraint() {
        let constraint = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
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
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_age".into()),
                columns: vec!["age".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }],
        }];
        let result = build_add_constraint(
            DatabaseBackend::Sqlite,
            "users",
            &constraint,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
    }
    #[test]
    fn test_add_constraint_composite_primary_key_postgres() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["user_id".into(), "role_id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let current_schema = vec![TableDef {
            name: "user_roles".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "user_id".into(),
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
                    name: "role_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
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
        let result = build_add_constraint(
            DatabaseBackend::Postgres,
            "user_roles",
            &constraint,
            &current_schema,
            &[],
        )
        .unwrap();
        let sql = result[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("ADD PRIMARY KEY"));
        assert!(sql.contains("\"user_id\""));
        assert!(sql.contains("\"role_id\""));
    }
    #[test]
    fn test_add_constraint_composite_primary_key_mysql() {
        let constraint = TableConstraint::PrimaryKey {
            columns: vec!["user_id".into(), "role_id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let current_schema = vec![TableDef {
            name: "user_roles".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "user_id".into(),
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
                    name: "role_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
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
        let result = build_add_constraint(
            DatabaseBackend::MySql,
            "user_roles",
            &constraint,
            &current_schema,
            &[],
        )
        .unwrap();
        let sql = result[0].build(DatabaseBackend::MySql);
        assert!(sql.contains("ADD PRIMARY KEY"));
        assert!(sql.contains("`user_id`"));
        assert!(sql.contains("`role_id`"));
    }
    #[test]
    fn test_constraints_overlap_primary_key_same_columns() {
        let a = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let b = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: true,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        assert!(constraints_overlap(&a, &b));
    }
    #[test]
    fn test_constraints_overlap_primary_key_different_columns() {
        let a = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let b = TableConstraint::PrimaryKey {
            columns: vec!["uid".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        assert!(!constraints_overlap(&a, &b));
    }
    #[test]
    fn test_constraints_overlap_check_same() {
        let a = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let b = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        assert!(constraints_overlap(&a, &b));
    }
    #[test]
    fn test_constraints_overlap_check_different_name() {
        let a = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let b = TableConstraint::Check {
            name: "chk_age2".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        assert!(!constraints_overlap(&a, &b));
    }
    #[test]
    fn test_constraints_overlap_check_different_expr() {
        let a = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        let b = TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 10".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        assert!(!constraints_overlap(&a, &b));
    }
    #[test]
    fn test_constraints_overlap_different_variants() {
        let a = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let b = TableConstraint::Check {
            name: "chk".into(),
            expr: "id > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };
        assert!(!constraints_overlap(&a, &b));
    }
    #[test]
    fn test_constraints_overlap_fk_same_columns() {
        let a = TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        };
        let b = TableConstraint::ForeignKey {
            name: Some("fk".into()),
            columns: vec!["user_id".into()],
            ref_table: "other".into(),
            ref_columns: vec!["oid".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        };
        assert!(constraints_overlap(&a, &b));
    }
    #[test]
    fn test_merge_constraint_replaces_overlapping() {
        let existing = vec![
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        ];
        let new_pk = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: true,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let result = merge_constraint(&existing, &new_pk);
        assert_eq!(result.len(), 2); // replaced, not added
    }
    /// Mutant target: `merge_constraint` `if !replaced` dedup guards.
    /// Mixes a non-overlapping `Index` between two overlapping
    /// `PrimaryKey`s so inner/outer guard-removal mutations produce
    /// distinct positional/cardinal results from the original.
    #[test]
    fn merge_constraint_dedups_when_multiple_existing_overlap() {
        let pk = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let idx = TableConstraint::Index {
            name: Some("idx_email".into()),
            columns: vec!["email".into()],
        };
        let existing = vec![pk.clone(), idx.clone(), pk.clone()];
        let new_pk = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: true,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let result = merge_constraint(&existing, &new_pk);

        // Pins OUTER `if !replaced` guard: without it, the trailing
        // fallback push adds a duplicate copy of the new PK → len 3.
        assert_eq!(
            result.len(),
            2,
            "merge_constraint must dedupe (one new PK + one preserved \
             index); got: {result:?}"
        );

        // Pins INNER `if !replaced` guard: without it, the new PK is
        // never pushed inside the loop body; the trailing fallback
        // pushes it at the END, so result[0] becomes the Index.
        let TableConstraint::PrimaryKey { auto_increment, .. } = &result[0] else {
            panic!(
                "result[0] must be the replacement PrimaryKey (pushed at \
                 the position of the FIRST overlap), not appended at the \
                 end via the trailing fallback; got: {:?}",
                result[0]
            );
        };
        assert!(
            *auto_increment,
            "the kept PK must be the new one (auto_increment = true); \
             got: {result:?}"
        );

        // Cardinality: exactly one PrimaryKey survives — direct
        // expression of "the second overlap was deduped".
        let pk_count = result
            .iter()
            .filter(|c| matches!(c, TableConstraint::PrimaryKey { .. }))
            .count();
        assert_eq!(
            pk_count, 1,
            "exactly one PrimaryKey must remain after merging away two \
             overlapping existing PKs; got {pk_count} in: {result:?}"
        );

        // Non-overlapping index must survive the merge.
        assert!(
            result.iter().any(|c| matches!(
                c,
                TableConstraint::Index { name: Some(n), .. } if n == "idx_email"
            )),
            "non-overlapping idx_email must be preserved; got: {result:?}"
        );
    }

    #[test]
    fn test_merge_constraint_appends_non_overlapping() {
        let existing = vec![TableConstraint::Index {
            name: None,
            columns: vec!["email".into()],
        }];
        let new_pk = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        };
        let result = merge_constraint(&existing, &new_pk);
        assert_eq!(result.len(), 2); // appended
    }
    #[test]
    fn test_extract_check_clauses_with_mixed_constraints() {
        let constraints = vec![
            TableConstraint::Check {
                name: "chk1".into(),
                expr: "a > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Check {
                name: "chk2".into(),
                expr: "b < 100".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            TableConstraint::Unique {
                name: Some("uq".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ];
        let clauses = crate::sql::helpers::extract_check_clauses(&constraints);
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
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ];
        let clauses = crate::sql::helpers::extract_check_clauses(&constraints);
        assert!(clauses.is_empty());
    }
}

mod unique;

use sea_query::{Alias, Query, Table};
use vespertide_core::{TableConstraint, TableDef};

use super::helpers::{build_sqlite_temp_table_create, recreate_indexes_after_rebuild};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

pub fn build_add_constraint(
    backend: DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    if let TableConstraint::PrimaryKey {
        columns, strategy, ..
    } = constraint
    {
        return primary_key::build_primary_key(
            backend,
            table,
            columns,
            strategy,
            constraint,
            current_schema,
            pending_constraints,
        );
    }

    match constraint {
        TableConstraint::Unique {
            name,
            columns,
            strategy,
        } => unique::build_unique(
            backend,
            table,
            name.as_deref(),
            columns,
            strategy,
            current_schema,
        ),
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
            orphan_strategy,
        } => foreign_key::build_foreign_key(
            backend,
            table,
            name.as_deref(),
            columns,
            ref_table,
            ref_columns,
            on_delete.as_ref(),
            on_update.as_ref(),
            *orphan_strategy,
            constraint,
            current_schema,
            pending_constraints,
        ),
        TableConstraint::Index { name, columns } => {
            Ok(index::build_index(table, name.as_deref(), columns))
        }
        TableConstraint::Check {
            name,
            expr,
            strategy,
        } => check::build_check(
            backend,
            table,
            name,
            expr,
            strategy,
            constraint,
            current_schema,
            pending_constraints,
        ),
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

pub(super) fn merge_constraint(
    existing: &[TableConstraint],
    constraint: &TableConstraint,
) -> Vec<TableConstraint> {
    let mut out = Vec::with_capacity(existing.len() + 1);
    let mut replaced = false;
    for c in existing {
        if constraints_overlap(c, constraint) {
            if !replaced {
                out.push(constraint.clone());
                replaced = true;
            }
        } else {
            out.push(c.clone());
        }
    }
    if !replaced {
        out.push(constraint.clone());
    }
    out
}

pub(super) fn constraints_overlap(a: &TableConstraint, b: &TableConstraint) -> bool {
    match (a, b) {
        (
            TableConstraint::ForeignKey {
                columns: a_cols, ..
            },
            TableConstraint::ForeignKey {
                columns: b_cols, ..
            },
        )
        | (
            TableConstraint::PrimaryKey {
                columns: a_cols, ..
            },
            TableConstraint::PrimaryKey {
                columns: b_cols, ..
            },
        ) => a_cols == b_cols,
        (
            TableConstraint::Check {
                name: a_name,
                expr: a_expr,
                ..
            },
            TableConstraint::Check {
                name: b_name,
                expr: b_expr,
                ..
            },
        ) => a_name == b_name && a_expr == b_expr,
        _ => false,
    }
}

pub(super) fn rebuild_sqlite_table_with_added_constraint(
    backend: DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let table_def = current_schema.iter().find(|t| t.name == table).ok_or_else(|| QueryError::SchemaError(format!("Table '{table}' not found in current schema. SQLite requires current schema information to add constraints.")))?;
    let new_constraints = merge_constraint(&table_def.constraints, constraint);
    let temp_table = format!("{table}_temp");
    let create_query = build_sqlite_temp_table_create(
        backend,
        &temp_table,
        table,
        &table_def.columns,
        &new_constraints,
    );
    let column_aliases: Vec<Alias> = table_def
        .columns
        .iter()
        .map(|c| Alias::new(&c.name))
        .collect();
    let mut select_query = Query::select();
    for col_alias in &column_aliases {
        select_query.column(col_alias.clone());
    }
    select_query.from(Alias::new(table));
    let insert_stmt = Query::insert()
        .into_table(Alias::new(&temp_table))
        .columns(column_aliases)
        .select_from(select_query)
        .unwrap()
        .to_owned();
    let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));
    let drop_table = Table::drop().table(Alias::new(table)).to_owned();
    let drop_query = BuiltQuery::DropTable(Box::new(drop_table));
    let rename_query = build_rename_table(&temp_table, table);
    let index_queries =
        recreate_indexes_after_rebuild(table, &table_def.constraints, pending_constraints);
    let mut queries = vec![create_query, insert_query, drop_query, rename_query];
    queries.extend(index_queries);
    Ok(queries)
}
