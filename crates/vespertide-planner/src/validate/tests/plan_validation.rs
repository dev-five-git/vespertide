use super::*;

#[test]
fn validate_migration_plan_missing_fill_with() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

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

    let result = validate_migration_plan(&plan);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::MissingFillWith(table, column) => {
            assert_eq!(table, "users");
            assert_eq!(column, "email");
        }
        _ => panic!("expected MissingFillWith error"),
    }
}

#[test]
fn validate_migration_plan_missing_fill_with_for_not_null_add_column() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

    let action = MigrationAction::AddColumn {
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
    };
    let plan = MigrationPlan {
        id: "test".into(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![action],
    };

    let result = validate_migration_plan(&plan);

    assert!(
        matches!(result, Err(PlannerError::MissingFillWith(_, _))),
        "AddColumn NOT NULL without default + no fill_with must error, got: {result:?}"
    );
}

#[test]
fn validate_migration_plan_with_fill_with() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

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
            fill_with: Some("default@example.com".into()),
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_migration_plan_nullable_column() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

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

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_migration_plan_with_default() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

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
                default: Some("default@example.com".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_string_enum_duplicate_variant_name() {
    let schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "active".into(), // duplicate
                    ]),
                }),
            ),
        ],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )];

    let result = validate_schema(&schema);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::DuplicateEnumVariantName(enum_name, table, column, variant) => {
            assert_eq!(enum_name, "user_status");
            assert_eq!(table, "users");
            assert_eq!(column, "status");
            assert_eq!(variant, "active");
        }
        err => panic!("expected DuplicateEnumVariantName, got {err:?}"),
    }
}

#[test]
fn validate_integer_enum_duplicate_variant_name() {
    let schema = vec![table(
        "tasks",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 1,
                        },
                        NumValue {
                            name: "Low".into(), // duplicate name
                            value: 2,
                        },
                    ]),
                }),
            ),
        ],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )];

    let result = validate_schema(&schema);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::DuplicateEnumVariantName(enum_name, table, column, variant) => {
            assert_eq!(enum_name, "priority_level");
            assert_eq!(table, "tasks");
            assert_eq!(column, "priority");
            assert_eq!(variant, "Low");
        }
        err => panic!("expected DuplicateEnumVariantName, got {err:?}"),
    }
}

#[test]
fn validate_integer_enum_duplicate_value() {
    let schema = vec![table(
        "tasks",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Medium".into(),
                            value: 1,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 0, // duplicate value
                        },
                    ]),
                }),
            ),
        ],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )];

    let result = validate_schema(&schema);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::DuplicateEnumValue(enum_name, table, column, value) => {
            assert_eq!(enum_name, "priority_level");
            assert_eq!(table, "tasks");
            assert_eq!(column, "priority");
            assert_eq!(value, 0);
        }
        err => panic!("expected DuplicateEnumValue, got {err:?}"),
    }
}

#[test]
fn validate_enum_valid() {
    let schema = vec![table(
        "tasks",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "in_progress".into(),
                        "completed".into(),
                    ]),
                }),
            ),
            col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Medium".into(),
                            value: 50,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 100,
                        },
                    ]),
                }),
            ),
        ],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )];

    let result = validate_schema(&schema);
    assert!(result.is_ok());
}

#[test]
fn validate_migration_plan_modify_nullable_to_non_nullable_missing_fill_with() {
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

    let result = validate_migration_plan(&plan);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::MissingFillWith(table, column) => {
            assert_eq!(table, "users");
            assert_eq!(column, "email");
        }
        _ => panic!("expected MissingFillWith error"),
    }
}

#[test]
fn validate_migration_plan_modify_nullable_to_non_nullable_with_fill_with() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: Some("'unknown'".into()),
            delete_null_rows: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_migration_plan_modify_non_nullable_to_nullable() {
    // Changing from non-nullable to nullable does NOT require fill_with
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

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_add_column_invalid_default() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: Some("invalid_value".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::InvalidEnumDefault(err) => {
            assert_eq!(err.enum_name, "user_status");
            assert_eq!(err.table_name, "users");
            assert_eq!(err.column_name, "status");
            assert_eq!(err.value_type, "default");
            assert_eq!(err.value, "invalid_value");
        }
        err => panic!("expected InvalidEnumDefault error, got {err:?}"),
    }
}

#[test]
fn validate_enum_add_column_invalid_fill_with() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("unknown_status".into()),
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::InvalidEnumDefault(err) => {
            assert_eq!(err.enum_name, "user_status");
            assert_eq!(err.table_name, "users");
            assert_eq!(err.column_name, "status");
            assert_eq!(err.value_type, "fill_with");
            assert_eq!(err.value, "unknown_status");
        }
        err => panic!("expected InvalidEnumDefault error, got {err:?}"),
    }
}

#[test]
fn validate_enum_add_column_valid_default_quoted() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: Some("'active'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_add_column_valid_default_unquoted() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: Some("active".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_add_column_valid_fill_with() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("'pending'".into()),
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_schema_invalid_default() {
    // Test that schema validation also catches invalid enum defaults
    let schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
            let mut c = col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
            );
            c.default = Some("invalid".into());
            c
        }],
        vec![pk(vec!["id"])],
    )];

    let result = validate_schema(&schema);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::InvalidEnumDefault(err) => {
            assert_eq!(err.enum_name, "user_status");
            assert_eq!(err.table_name, "users");
            assert_eq!(err.column_name, "status");
            assert_eq!(err.value_type, "default");
            assert_eq!(err.value, "invalid");
        }
        err => panic!("expected InvalidEnumDefault error, got {err:?}"),
    }
}

#[test]
fn validate_enum_schema_valid_default() {
    let schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
            let mut c = col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
            );
            c.default = Some("'active'".into());
            c
        }],
        vec![pk(vec!["id"])],
    )];

    let result = validate_schema(&schema);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_integer_add_column_valid() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "tasks".into(),
            column: Box::new(ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Medium".into(),
                            value: 50,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 100,
                        },
                    ]),
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("Low".into()),
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_integer_add_column_invalid() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "tasks".into(),
            column: Box::new(ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Medium".into(),
                            value: 50,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 100,
                        },
                    ]),
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("Critical".into()), // Not a valid enum name
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::InvalidEnumDefault(err) => {
            assert_eq!(err.enum_name, "priority_level");
            assert_eq!(err.table_name, "tasks");
            assert_eq!(err.column_name, "priority");
            assert_eq!(err.value_type, "fill_with");
            assert_eq!(err.value, "Critical");
        }
        err => panic!("expected InvalidEnumDefault error, got {err:?}"),
    }
}

fn integer_priority_column(default: Option<DefaultValue>) -> ColumnDef {
    ColumnDef {
        name: "priority".into(),
        r#type: ColumnType::Complex(ComplexColumnType::Enum {
            name: "priority_level".into(),
            values: EnumValues::Integer(vec![
                NumValue {
                    name: "low".into(),
                    value: 0,
                },
                NumValue {
                    name: "normal".into(),
                    value: 10,
                },
                NumValue {
                    name: "high".into(),
                    value: 20,
                },
            ]),
        }),
        nullable: false,
        default,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

fn add_integer_priority_plan(
    default: Option<DefaultValue>,
    fill_with: Option<&str>,
) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "tasks".into(),
            column: Box::new(integer_priority_column(default)),
            fill_with: fill_with.map(str::to_string),
        }],
    }
}

#[test]
fn validate_integer_enum_numeric_default_matches_stored_value() {
    let plan = add_integer_priority_plan(Some(10.into()), None);

    let result = validate_migration_plan(&plan);

    assert!(result.is_ok());
}

#[test]
fn validate_integer_enum_numeric_fill_with_matches_stored_value() {
    let plan = add_integer_priority_plan(None, Some("10"));

    let result = validate_migration_plan(&plan);

    assert!(result.is_ok());
}

#[test]
fn validate_integer_enum_invalid_numeric_value_is_rejected() {
    let plan = add_integer_priority_plan(Some(999.into()), None);

    let result = validate_migration_plan(&plan);

    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::InvalidEnumDefault(err) => {
            assert_eq!(err.enum_name, "priority_level");
            assert_eq!(err.table_name, "tasks");
            assert_eq!(err.column_name, "priority");
            assert_eq!(err.value_type, "default");
            assert_eq!(err.value, "999");
        }
        err => panic!("expected InvalidEnumDefault error, got {err:?}"),
    }
}

#[test]
fn validate_integer_enum_name_default_remains_supported() {
    let plan = add_integer_priority_plan(Some("normal".into()), None);

    let result = validate_migration_plan(&plan);

    assert!(result.is_ok());
}

#[test]
fn validate_enum_null_value_skipped() {
    // NULL values should be allowed and skipped during validation
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
                nullable: true,
                default: Some("NULL".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_sql_expression_skipped() {
    // SQL expressions like function calls should be skipped
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: Some("COALESCE(old_status, 'active')".into()),
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn validate_enum_empty_string_fill_with_skipped() {
    // Empty string fill_with should be skipped during enum validation
    // (converted to '' by to_sql, which is empty after trimming)
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            // Empty string - extract_enum_value returns None for empty trimmed values
            fill_with: Some("   ".into()),
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

fn string_enum_default_plan(default: &str) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
                nullable: true,
                default: Some(default.into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    }
}

#[rstest]
#[case::closing_paren("active)")]
#[case::current_timestamp("CURRENT_TIMESTAMP")]
#[case::now("now")]
fn validate_enum_sql_keyword_or_expression_default_skipped(#[case] default: &str) {
    assert!(validate_migration_plan(&string_enum_default_plan(default)).is_ok());
}

#[rstest]
#[case::unclosed_single_quote("'active")]
#[case::unopened_double_quote("active\"")]
fn validate_enum_rejects_unbalanced_quoted_defaults(#[case] default: &str) {
    assert!(validate_migration_plan(&string_enum_default_plan(default)).is_err());
}

/// Batch-reporting contract for [`crate::validate::validate_migration_plan`]:
/// a plan with multiple offending actions collapses into a single
/// [`PlannerError::Multiple`] whose nested errors preserve action-index
/// order. The `Display` impl renders a numbered list so loader / CLI
/// callers surface every violation in one pass instead of forcing the
/// user to fix-and-rerun for each one.
#[test]
fn validate_migration_plan_batches_multiple_violations() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

    // Helper: build a NOT NULL AddColumn action without a default or
    // fill_with — guaranteed to trigger MissingFillWith.
    fn add_not_null(table: &str, column: &str) -> MigrationAction {
        MigrationAction::AddColumn {
            table: table.into(),
            column: Box::new(ColumnDef {
                name: column.into(),
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
        }
    }

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            add_not_null("users", "email"),
            add_not_null("orders", "total"),
            add_not_null("products", "sku"),
        ],
    };

    let err = validate_migration_plan(&plan).unwrap_err();

    let PlannerError::Multiple(batch) = &err else {
        panic!("expected PlannerError::Multiple, got: {err:?}");
    };

    assert_eq!(
        batch.0.len(),
        3,
        "expected exactly 3 violations, got: {:?}",
        batch.0
    );

    // Action-index order is preserved.
    let extract = |idx: usize| -> (&str, &str) {
        match &batch.0[idx] {
            PlannerError::MissingFillWith(t, c) => (t.as_str(), c.as_str()),
            other => panic!("violation #{idx} not MissingFillWith: {other:?}"),
        }
    };
    assert_eq!(extract(0), ("users", "email"));
    assert_eq!(extract(1), ("orders", "total"));
    assert_eq!(extract(2), ("products", "sku"));

    // Display contract — numbered list, fix-all footer.
    let rendered = format!("{err}");
    assert!(rendered.starts_with("3 validation violation(s):"));
    assert!(rendered.contains("\n  1. "));
    assert!(rendered.contains("\n  3. "));
    assert!(rendered.ends_with("Fix all of the above before re-running this command."));
}

/// Single-violation contract preservation: a plan with exactly one
/// offending action must still return the bare variant (not wrapped in
/// `Multiple`). This is the compatibility guarantee that lets every
/// pre-existing `matches!(err, PlannerError::Xxx(_))` test keep working
/// after the batch change.
#[test]
fn validate_migration_plan_single_violation_returns_bare_variant() {
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

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

    let err = validate_migration_plan(&plan).unwrap_err();
    assert!(
        matches!(err, PlannerError::MissingFillWith(_, _)),
        "single-violation plan must return bare variant, got: {err:?}"
    );
}
