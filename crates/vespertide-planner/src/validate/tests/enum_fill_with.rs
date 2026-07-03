use super::*;

// ── find_missing_enum_fill_with tests ──────────────────────────────

fn string_enum(name: &str, values: Vec<&str>) -> ColumnType {
    ColumnType::Complex(ComplexColumnType::Enum {
        name: name.into(),
        values: EnumValues::String(
            values
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ),
    })
}

#[test]
fn find_missing_enum_fill_with_detects_removed_values() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: string_enum("order_status", vec!["pending", "shipped"]),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![table(
        "orders",
        vec![col(
            "status",
            string_enum("order_status", vec!["pending", "shipped", "cancelled"]),
        )],
        vec![],
    )];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].table, "orders");
    assert_eq!(missing[0].column, "status");
    assert_eq!(missing[0].removed_values, vec!["cancelled"]);
    assert_eq!(missing[0].remaining_values, vec!["pending", "shipped"]);
}

#[test]
fn find_missing_enum_fill_with_ignores_additions_only() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: string_enum("order_status", vec!["pending", "shipped", "delivered"]),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![table(
        "orders",
        vec![col(
            "status",
            string_enum("order_status", vec!["pending", "shipped"]),
        )],
        vec![],
    )];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert!(
        missing.is_empty(),
        "Adding values should not trigger fill_with"
    );
}

#[test]
fn find_missing_enum_fill_with_skips_already_covered() {
    let mut fw = std::collections::BTreeMap::new();
    fw.insert("cancelled".to_string(), "pending".to_string());

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: string_enum("order_status", vec!["pending", "shipped"]),
            fill_with: Some(fw),
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![table(
        "orders",
        vec![col(
            "status",
            string_enum("order_status", vec!["pending", "shipped", "cancelled"]),
        )],
        vec![],
    )];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert!(
        missing.is_empty(),
        "All removed values are covered by fill_with"
    );
}

#[test]
fn find_missing_enum_fill_with_reports_partially_covered() {
    let mut fw = std::collections::BTreeMap::new();
    fw.insert("cancelled".to_string(), "pending".to_string());

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: string_enum("order_status", vec!["pending"]),
            fill_with: Some(fw),
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![table(
        "orders",
        vec![col(
            "status",
            string_enum("order_status", vec!["pending", "shipped", "cancelled"]),
        )],
        vec![],
    )];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].removed_values,
        vec!["shipped"],
        "Only uncovered value should be reported"
    );
}

#[test]
fn find_missing_enum_fill_with_ignores_integer_enums() {
    let old_type = ColumnType::Complex(ComplexColumnType::Enum {
        name: "priority".into(),
        values: EnumValues::Integer(vec![
            NumValue {
                name: "low".into(),
                value: 0,
            },
            NumValue {
                name: "high".into(),
                value: 1,
            },
        ]),
    });
    let new_type = ColumnType::Complex(ComplexColumnType::Enum {
        name: "priority".into(),
        values: EnumValues::Integer(vec![NumValue {
            name: "high".into(),
            value: 1,
        }]),
    });

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "tasks".into(),
            column: "priority".into(),
            new_type,
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![table("tasks", vec![col("priority", old_type)], vec![])];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert!(
        missing.is_empty(),
        "Integer enum changes should not trigger fill_with"
    );
}

#[test]
fn find_missing_enum_fill_with_ignores_non_enum_type_change() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "age".into(),
            new_type: ColumnType::Simple(SimpleColumnType::BigInt),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![table(
        "users",
        vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert!(
        missing.is_empty(),
        "Non-enum type changes should not trigger fill_with"
    );
}

#[test]
fn validate_modify_column_type_fill_with_invalid_replacement() {
    let mut fw = std::collections::BTreeMap::new();
    fw.insert("cancelled".to_string(), "nonexistent_value".to_string());

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
            }),
            fill_with: Some(fw),
            narrowing_strategy: None,
            timezone: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_err());
    match result.unwrap_err() {
        PlannerError::InvalidEnumDefault(err) => {
            assert_eq!(err.enum_name, "order_status");
            assert_eq!(err.table_name, "orders");
            assert_eq!(err.column_name, "status");
            assert_eq!(err.value_type, "fill_with");
            assert_eq!(err.value, "nonexistent_value");
        }
        err => panic!("expected InvalidEnumDefault error, got {err:?}"),
    }
}

#[test]
fn validate_modify_column_type_fill_with_valid_replacement() {
    let mut fw = std::collections::BTreeMap::new();
    fw.insert("cancelled".to_string(), "pending".to_string());

    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
            }),
            fill_with: Some(fw),
            narrowing_strategy: None,
            timezone: None,
        }],
    };

    let result = validate_migration_plan(&plan);
    assert!(result.is_ok());
}

#[test]
fn find_missing_enum_fill_with_parallel_path_sorts_by_action_index() {
    let mut actions: Vec<MigrationAction> = (0..10_000)
        .map(|idx| MigrationAction::DeleteColumn {
            table: "noop".into(),
            column: format!("old_{idx}").into(),
        })
        .collect();
    actions.push(MigrationAction::ModifyColumnType {
        table: "orders".into(),
        column: "status".into(),
        new_type: string_enum("order_status", vec!["pending"]),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    });
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions,
    };
    let baseline = vec![table(
        "orders",
        vec![col(
            "status",
            string_enum("order_status", vec!["pending", "cancelled"]),
        )],
        vec![],
    )];

    let missing = find_missing_enum_fill_with(&plan, &baseline);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].action_index, 10_000);
    assert_eq!(missing[0].removed_values, vec!["cancelled"]);
}
