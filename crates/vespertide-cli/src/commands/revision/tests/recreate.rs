use super::*;

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_writes_migration() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config();
    write_model("users");
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    cmd_revision("init".into(), vec![], vec![]).await.unwrap();

    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir()).unwrap().collect();
    assert!(!entries.is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_no_changes_short_circuits() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config();
    // no models, no migrations -> plan with no actions -> early return
    assert!(cmd_revision("noop".into(), vec![], vec![]).await.is_ok());
    // migrations dir should not be created
    assert!(!cfg.migrations_dir().exists());
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_writes_yaml_when_configured() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config_with_format(Some(FileFormat::Yaml));
    write_model("users");
    // ensure migrations dir absent to exercise create_dir_all branch
    if cfg.migrations_dir().exists() {
        std_fs::remove_dir_all(cfg.migrations_dir()).unwrap();
    }

    cmd_revision("yaml".into(), vec![], vec![]).await.unwrap();

    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir()).unwrap().collect();
    assert!(!entries.is_empty());
    let has_yaml = entries.iter().any(|e| {
        e.as_ref()
            .unwrap()
            .path()
            .extension()
            .is_some_and(|s| s == "yaml")
    });
    assert!(has_yaml);
}

#[test]
fn find_non_nullable_fk_add_column_detects_recreate() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("1".into()),
            },
            MigrationAction::AddConstraint {
                table: "post".into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            },
        ],
    };
    let result = find_non_nullable_fk_add_columns(&plan, &[]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].table, "post");
    assert_eq!(result[0].column, "user_id");
    assert_eq!(result[0].reason, RecreateReason::AddColumnWithFk);
}

#[test]
fn find_non_nullable_inline_fk_add_column_detects_recreate() {
    use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
    use vespertide_core::{ColumnDef, ColumnType, ReferenceAction, SimpleColumnType};

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::AddColumn {
            table: "post".into(),
            column: Box::new(ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            }),
            fill_with: None,
        }],
    };

    let result = find_non_nullable_fk_add_columns(&plan, &[]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].table, "post");
    assert_eq!(result[0].column, "user_id");
    assert_eq!(result[0].reason, RecreateReason::AddColumnWithFk);
}

#[test]
fn find_nullable_fk_add_column_returns_empty() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "post".into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            },
        ],
    };
    assert!(find_non_nullable_fk_add_columns(&plan, &[]).is_empty());
}

#[test]
fn find_non_nullable_no_fk_returns_empty() {
    // Regular non-nullable column without FK should NOT trigger recreation
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::AddColumn {
            table: "post".into(),
            column: Box::new(ColumnDef {
                name: "user_id1".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };
    // Should return empty — this column needs fill_with but that's handled separately
    assert!(find_non_nullable_fk_add_columns(&plan, &[]).is_empty());
}

#[test]
fn find_fk_on_existing_non_nullable_column_detects_recreate() {
    // Adding FK constraint to an existing non-nullable column should trigger recreation
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::AddConstraint {
            table: "post".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }],
    };
    let models = vec![TableDef {
        name: "post".into(),
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
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
    let result = find_non_nullable_fk_add_columns(&plan, &models);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].table, "post");
    assert_eq!(result[0].column, "user_id");
    assert_eq!(result[0].reason, RecreateReason::AddFkToExistingColumn);
}

#[test]
fn find_fk_on_existing_nullable_column_returns_empty() {
    // Adding FK constraint to an existing nullable column should NOT trigger recreation
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::AddConstraint {
            table: "post".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }],
    };
    let models = vec![TableDef {
        name: "post".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
    assert!(find_non_nullable_fk_add_columns(&plan, &models).is_empty());
}

#[test]
fn find_fk_on_existing_column_with_default_returns_empty() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::AddConstraint {
            table: "post".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }],
    };
    let models = vec![TableDef {
        name: "post".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: Some(true.into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    }];

    assert!(find_non_nullable_fk_add_columns(&plan, &models).is_empty());
}

#[test]
fn find_fk_on_existing_column_missing_from_model_returns_empty() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::AddConstraint {
            table: "post".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }],
    };
    let models = vec![TableDef {
        name: "post".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "other_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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

    assert!(find_non_nullable_fk_add_columns(&plan, &models).is_empty());
}

#[test]
fn rewrite_plan_replaces_actions_with_recreate() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "post".into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            },
        ],
    };

    let recreate = vec![RecreateTableRequired {
        table: "post".into(),
        column: "user_id".into(),
        reason: RecreateReason::AddColumnWithFk,
    }];

    let models = vec![TableDef {
        name: "post".into(),
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
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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

    rewrite_plan_for_recreation(&mut plan, &recreate, &models);

    assert_eq!(plan.actions.len(), 2);
    assert!(matches!(&plan.actions[0], MigrationAction::DeleteTable { table } if table == "post"));
    assert!(
        matches!(&plan.actions[1], MigrationAction::CreateTable { table, .. } if table == "post")
    );
}

#[test]
fn rewrite_plan_keeps_non_table_actions() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![
            MigrationAction::RawSql {
                sql: "select 1".into(),
            },
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
        ],
    };

    let recreate = vec![RecreateTableRequired {
        table: "post".into(),
        column: "user_id".into(),
        reason: RecreateReason::AddColumnWithFk,
    }];

    let models = vec![TableDef {
        name: "post".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "user_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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

    rewrite_plan_for_recreation(&mut plan, &recreate, &models);

    assert!(matches!(&plan.actions[0], MigrationAction::RawSql { sql } if sql == "select 1"));
    assert!(matches!(&plan.actions[1], MigrationAction::DeleteTable { table } if table == "post"));
    assert!(
        matches!(&plan.actions[2], MigrationAction::CreateTable { table, .. } if table == "post")
    );
}

#[test]
fn handle_recreate_requirements_returns_ok_when_no_fk() {
    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::RawSql {
            sql: "select 1".into(),
        }],
    };

    handle_recreate_requirements(&mut plan, &[], |_| Ok(true)).unwrap();

    assert_eq!(plan.actions.len(), 1);
}

#[test]
fn handle_recreate_requirements_bails_when_prompt_rejected() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "post".into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            },
        ],
    };

    let err = handle_recreate_requirements(&mut plan, &[], |_| Ok(false)).unwrap_err();

    assert!(
        err.to_string()
            .contains("Migration cancelled. To proceed without recreation")
    );
}

#[test]
fn handle_recreate_requirements_empties_plan_when_model_missing() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "post".into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            },
        ],
    };

    handle_recreate_requirements(&mut plan, &[], |_| Ok(true)).unwrap();

    assert!(plan.actions.is_empty());
}

#[test]
fn handle_recreate_requirements_rewrites_plan_when_model_exists() {
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::AddColumn {
                table: "post".into(),
                column: Box::new(ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "post".into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            },
        ],
    };

    let models = vec![TableDef {
        name: "post".into(),
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
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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

    handle_recreate_requirements(&mut plan, &models, |_| Ok(true)).unwrap();

    assert_eq!(plan.actions.len(), 2);
    assert!(matches!(&plan.actions[0], MigrationAction::DeleteTable { table } if table == "post"));
    assert!(
        matches!(&plan.actions[1], MigrationAction::CreateTable { table, .. } if table == "post")
    );
}
