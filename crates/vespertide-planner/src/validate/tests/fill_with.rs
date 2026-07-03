use super::*;

// Tests for find_missing_fill_with function
#[test]
fn find_missing_fill_with_add_column_not_null_no_default() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
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

    let missing = find_missing_fill_with(&plan, &[]);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].table, "users");
    assert_eq!(missing[0].column, "email");
    assert_eq!(missing[0].action_type, "AddColumn");
    assert!(!missing[0].column_type.is_empty());
}

#[test]
fn find_missing_fill_with_add_column_with_default() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: Some("'default@example.com'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert!(missing.is_empty());
}

#[test]
fn find_missing_fill_with_add_column_nullable() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
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
        }],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert!(missing.is_empty());
}

#[test]
fn find_missing_fill_with_add_column_with_fill_with() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("'default@example.com'".into()),
        }],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert!(missing.is_empty());
}

#[test]
fn find_missing_fill_with_modify_nullable_to_not_null() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].table, "users");
    assert_eq!(missing[0].column, "email");
    assert_eq!(missing[0].action_type, "ModifyColumnNullable");
    // With no schema provided, falls back to column name as type display
    assert_eq!(missing[0].column_type, "email");
}

#[test]
fn find_missing_fill_with_modify_to_nullable() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: true,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert!(missing.is_empty());
}

#[test]
fn find_missing_fill_with_modify_not_null_with_fill_with() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: Some("'default'".into()),
            delete_null_rows: None,
        }],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert!(missing.is_empty());
}

#[test]
fn find_missing_fill_with_modify_nullable_to_not_null_with_column_default() {
    // Column has a default value in the schema, so fill_with should NOT be required
    let schema = vec![TableDef {
        name: "users".into(),
        columns: vec![ColumnDef {
            name: "status".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: true,
            default: Some(DefaultValue::String("'active'".into())),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
        description: None,
    }];

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "status".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let missing = find_missing_fill_with(&plan, &schema);
    assert!(
        missing.is_empty(),
        "fill_with should not be required when column has a default value"
    );
}

#[test]
fn find_missing_fill_with_modify_nullable_to_not_null_without_column_default() {
    // Column has NO default value, so fill_with IS required
    let schema = vec![TableDef {
        name: "users".into(),
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
        description: None,
    }];

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let missing = find_missing_fill_with(&plan, &schema);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].column, "email");
}

#[test]
fn find_missing_fill_with_multiple_actions() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
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
            MigrationAction::ModifyColumnNullable {
                table: "orders".into(),
                column: "status".into(),
                nullable: false,
                fill_with: None,
                delete_null_rows: None,
            },
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "name".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true, // nullable, so not missing
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

    let missing = find_missing_fill_with(&plan, &[]);
    assert_eq!(missing.len(), 2);
    assert_eq!(missing[0].action_index, 0);
    assert_eq!(missing[0].table, "users");
    assert_eq!(missing[0].column, "email");
    assert_eq!(missing[1].action_index, 1);
    assert_eq!(missing[1].table, "orders");
    assert_eq!(missing[1].column, "status");
}

#[test]
fn find_missing_fill_with_other_actions_ignored() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![pk(vec!["id"])],
            },
            MigrationAction::DeleteColumn {
                table: "orders".into(),
                column: "old_column".into(),
            },
        ],
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert!(missing.is_empty());
}

#[test]
fn find_missing_fill_with_parallel_path_sorts_by_action_index() {
    let mut actions: Vec<MigrationAction> = (0..10_000)
        .map(|idx| MigrationAction::DeleteColumn {
            table: "noop".into(),
            column: format!("old_{idx}").into(),
        })
        .collect();
    actions.push(MigrationAction::AddColumn {
        table: "users".into(),
        column: Box::new(ColumnDef::new(
            "email",
            ColumnType::Simple(SimpleColumnType::Text),
            false,
        )),
        fill_with: None,
    });
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    };

    let missing = find_missing_fill_with(&plan, &[]);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].action_index, 10_000);
    assert_eq!(missing[0].column, "email");
}

#[test]
fn validate_auto_increment_on_text_column_fails() {
    let table_def = table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
        vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    );

    let result = validate_table(&table_def, &std::collections::BTreeMap::new());
    assert!(result.is_err());
    match result {
        Err(PlannerError::InvalidAutoIncrement(table_name, col_name, _)) => {
            assert_eq!(table_name, "users");
            assert_eq!(col_name, "id");
        }
        _ => panic!("Expected InvalidAutoIncrement error"),
    }
}

#[test]
fn validate_auto_increment_on_integer_column_succeeds() {
    let table_def = table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    );

    let result = validate_table(&table_def, &std::collections::BTreeMap::new());
    assert!(result.is_ok());
}

#[test]
fn validate_inline_auto_increment_on_text_column_fails() {
    let mut col_def = col("id", ColumnType::Simple(SimpleColumnType::Text));
    col_def.primary_key = Some(PrimaryKeySyntax::Object(PrimaryKeyDef {
        auto_increment: true,
    }));

    let table_def = table("users", vec![col_def], vec![]);

    let result = validate_table(&table_def, &std::collections::BTreeMap::new());
    assert!(result.is_err());
    match result {
        Err(PlannerError::InvalidAutoIncrement(table_name, col_name, _)) => {
            assert_eq!(table_name, "users");
            assert_eq!(col_name, "id");
        }
        _ => panic!("Expected InvalidAutoIncrement error"),
    }
}

#[test]
fn validate_inline_primary_key_bool_does_not_check_auto_increment() {
    // PrimaryKeySyntax::Bool(true) has no auto_increment field, so validation
    // should pass even on a non-integer column.
    let mut col_def = col("code", ColumnType::Simple(SimpleColumnType::Text));
    col_def.primary_key = Some(PrimaryKeySyntax::Bool(true));

    let table_def = table("items", vec![col_def], vec![]);
    let result = validate_table(&table_def, &std::collections::BTreeMap::new());
    assert!(
        result.is_ok(),
        "Bool primary key should not trigger auto_increment validation"
    );
}
