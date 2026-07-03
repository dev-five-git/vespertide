use super::*;

// Tests for integer enum handling
mod integer_enum {
    use super::*;
    use vespertide_core::{ComplexColumnType, EnumValues, NumValue};

    #[test]
    fn integer_enum_values_changed_no_migration() {
        // Integer enum values changed - should NOT generate ModifyColumnType
        let from = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Shipped".into(),
                            value: 1,
                        },
                    ]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Shipped".into(),
                            value: 1,
                        },
                        NumValue {
                            name: "Delivered".into(),
                            value: 2,
                        },
                        NumValue {
                            name: "Cancelled".into(),
                            value: 100,
                        },
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert!(
            plan.actions.is_empty(),
            "Expected no actions, got: {:?}",
            plan.actions
        );
    }

    #[test]
    fn integer_enum_name_changed_no_migration() {
        // Integer enum name changed - should NOT generate migration
        // because integer enums use INTEGER column type, not a named PG type.
        let from = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Shipped".into(),
                            value: 1,
                        },
                    ]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Shipped".into(),
                            value: 1,
                        },
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert!(
            plan.actions.is_empty(),
            "Expected no actions for integer enum name change, got: {:?}",
            plan.actions
        );
    }
    #[test]
    fn string_enum_values_changed_requires_migration() {
        // String enum values changed - SHOULD generate ModifyColumnType
        let from = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "shipped".into(),
                        "delivered".into(),
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnType { table, column, .. }
            if table == "orders" && column == "status"
        ));
    }

    #[test]
    fn string_enum_name_changed_same_values_requires_migration() {
        // String enum name changed but values identical - SHOULD generate migration
        // because the PostgreSQL enum type name is derived from the enum name.
        let from = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                }),
            )],
            vec![],
        )];
        let to = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                }),
            )],
            vec![],
        )];
        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(
            plan.actions.len(),
            1,
            "Expected 1 action for enum name change, got: {:?}",
            plan.actions
        );
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnType { table, column, new_type, .. }
            if table == "orders" && column == "status"
                && matches!(new_type, ColumnType::Complex(ComplexColumnType::Enum { name, .. }) if name == "status")
        ));
    }

    #[test]
    fn string_enum_name_and_values_changed_requires_migration() {
        // String enum name AND values changed - SHOULD generate ModifyColumnType
        let from = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "shipped".into(),
                        "delivered".into(),
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnType { table, column, .. }
            if table == "orders" && column == "status"
        ));
    }
}

// Tests for detecting enum name changes
mod enum_name_change {
    use super::*;
    use vespertide_core::{ComplexColumnType, EnumValues, NumValue};

    #[test]
    fn same_enum_name_and_values_no_migration() {
        // Identical enum - no migration needed
        let schema = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&schema, &schema).unwrap();
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn string_enum_name_only_changed_detects_rename() {
        // Only enum name changed, values identical - MUST detect as rename
        let from = vec![table(
            "users",
            vec![col(
                "role",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_role".into(),
                    values: EnumValues::String(vec![
                        "admin".into(),
                        "member".into(),
                        "guest".into(),
                    ]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![col(
                "role",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "role".into(),
                    values: EnumValues::String(vec![
                        "admin".into(),
                        "member".into(),
                        "guest".into(),
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(
            plan.actions.len(),
            1,
            "Expected 1 action, got: {:?}",
            plan.actions
        );
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnType { table, column, new_type, .. }
            if table == "users" && column == "role"
                && matches!(new_type, ColumnType::Complex(ComplexColumnType::Enum { name, .. }) if name == "role")
        ));
    }

    #[test]
    fn multiple_columns_enum_name_changed() {
        // Multiple columns with enum name changes in same table
        let from = vec![table(
            "orders",
            vec![
                col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                    }),
                ),
                col(
                    "priority",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_priority".into(),
                        values: EnumValues::String(vec!["low".into(), "high".into()]),
                    }),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![
                col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "status".into(),
                        values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                    }),
                ),
                col(
                    "priority",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority".into(),
                        values: EnumValues::String(vec!["low".into(), "high".into()]),
                    }),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(
            plan.actions.len(),
            2,
            "Expected 2 actions, got: {:?}",
            plan.actions
        );
        // Both should be ModifyColumnType for enum renames
        assert!(
            plan.actions
                .iter()
                .all(|a| matches!(a, MigrationAction::ModifyColumnType { .. }))
        );
    }

    #[test]
    fn integer_enum_name_changed_ignored() {
        // Integer enum name change - should be ignored (DB type is always INTEGER)
        let from = vec![table(
            "orders",
            vec![col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "old_priority".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 10,
                        },
                    ]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "new_priority".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "High".into(),
                            value: 10,
                        },
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert!(
            plan.actions.is_empty(),
            "Expected no actions for integer enum rename, got: {:?}",
            plan.actions
        );
    }

    #[test]
    fn enum_name_changed_across_tables() {
        // Enum name changes detected across different tables
        let from = vec![
            table(
                "users",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec!["active".into(), "banned".into()]),
                    }),
                )],
                vec![],
            ),
            table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec!["pending".into(), "done".into()]),
                    }),
                )],
                vec![],
            ),
        ];

        let to = vec![
            table(
                "users",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "status".into(),
                        values: EnumValues::String(vec!["active".into(), "banned".into()]),
                    }),
                )],
                vec![],
            ),
            table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "status".into(),
                        values: EnumValues::String(vec!["pending".into(), "done".into()]),
                    }),
                )],
                vec![],
            ),
        ];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(
            plan.actions.len(),
            2,
            "Expected 2 actions, got: {:?}",
            plan.actions
        );
        assert!(
            plan.actions
                .iter()
                .all(|a| matches!(a, MigrationAction::ModifyColumnType { .. }))
        );
    }

    #[test]
    fn enum_name_changed_with_value_change_single_action() {
        // Both name AND values changed - should produce only ONE ModifyColumnType
        // (the value change already triggers it; enum rename doesn't duplicate)
        let from = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                }),
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "shipped".into(),
                        "delivered".into(),
                    ]),
                }),
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        // Should be exactly 1 action, not 2 (no duplicate for name + values)
        assert_eq!(
            plan.actions.len(),
            1,
            "Expected exactly 1 action, got: {:?}",
            plan.actions
        );
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnType { .. }
        ));
    }
}
// Tests for enum + default value ordering
mod enum_default_ordering {
    use super::*;
    use vespertide_core::{ComplexColumnType, EnumValues};

    fn col_enum_with_default(
        name: &str,
        enum_name: &str,
        values: Vec<&str>,
        default: &str,
    ) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Complex(ComplexColumnType::Enum {
                name: enum_name.to_string(),
                values: EnumValues::String(values.into_iter().map(String::from).collect()),
            }),
            nullable: false,
            default: Some(default.into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn enum_add_value_with_new_default() {
        // Case 1: Add new enum value and change default to that new value
        // Expected order: ModifyColumnType FIRST (add value), then ModifyColumnDefault
        let from = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped"],
                "'pending'",
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped", "delivered"],
                "'delivered'", // new default uses newly added value
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate both actions
        assert_eq!(
            plan.actions.len(),
            2,
            "Expected 2 actions, got: {:?}",
            plan.actions
        );

        // ModifyColumnType should come FIRST (to add the new enum value)
        assert!(
            matches!(&plan.actions[0], MigrationAction::ModifyColumnType { table, column, .. }
                if table == "orders" && column == "status"),
            "First action should be ModifyColumnType, got: {:?}",
            plan.actions[0]
        );

        // ModifyColumnDefault should come SECOND
        assert!(
            matches!(&plan.actions[1], MigrationAction::ModifyColumnDefault { table, column, .. }
                if table == "orders" && column == "status"),
            "Second action should be ModifyColumnDefault, got: {:?}",
            plan.actions[1]
        );
    }

    #[test]
    fn enum_remove_value_that_was_default() {
        // Case 2: Remove enum value that was the default
        // Expected order: ModifyColumnDefault FIRST (change away from removed value),
        //                 then ModifyColumnType (remove the value)
        let from = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped", "cancelled"],
                "'cancelled'", // default is 'cancelled' which will be removed
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped"], // 'cancelled' removed
                "'pending'",                // default changed to existing value
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate both actions
        assert_eq!(
            plan.actions.len(),
            2,
            "Expected 2 actions, got: {:?}",
            plan.actions
        );

        // ModifyColumnDefault should come FIRST (change default before removing enum value)
        assert!(
            matches!(&plan.actions[0], MigrationAction::ModifyColumnDefault { table, column, .. }
                if table == "orders" && column == "status"),
            "First action should be ModifyColumnDefault, got: {:?}",
            plan.actions[0]
        );

        // ModifyColumnType should come SECOND (now safe to remove enum value)
        assert!(
            matches!(&plan.actions[1], MigrationAction::ModifyColumnType { table, column, .. }
                if table == "orders" && column == "status"),
            "Second action should be ModifyColumnType, got: {:?}",
            plan.actions[1]
        );
    }

    #[test]
    fn enum_remove_value_default_unchanged() {
        // Remove enum value, but default was NOT that value (no reordering needed)
        let from = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped", "cancelled"],
                "'pending'", // default is 'pending', not the removed 'cancelled'
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped"], // 'cancelled' removed
                "'pending'",                // default unchanged
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate only ModifyColumnType (default unchanged)
        assert_eq!(
            plan.actions.len(),
            1,
            "Expected 1 action, got: {:?}",
            plan.actions
        );
        assert!(
            matches!(&plan.actions[0], MigrationAction::ModifyColumnType { table, column, .. }
                if table == "orders" && column == "status"),
            "Action should be ModifyColumnType, got: {:?}",
            plan.actions[0]
        );
    }

    #[test]
    fn enum_remove_value_with_default_change_to_remaining() {
        // Remove multiple enum values, old default was one of them, new default is a remaining value
        let from = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["draft", "pending", "shipped", "delivered", "cancelled"],
                "'cancelled'",
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped", "delivered"], // removed 'draft' and 'cancelled'
                "'pending'",
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(
            plan.actions.len(),
            2,
            "Expected 2 actions, got: {:?}",
            plan.actions
        );

        // ModifyColumnDefault MUST come first because old default 'cancelled' is being removed
        assert!(
            matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnDefault { .. }
            ),
            "First action should be ModifyColumnDefault, got: {:?}",
            plan.actions[0]
        );
        assert!(
            matches!(&plan.actions[1], MigrationAction::ModifyColumnType { .. }),
            "Second action should be ModifyColumnType, got: {:?}",
            plan.actions[1]
        );
    }

    #[test]
    fn enum_remove_value_with_unquoted_default() {
        // Test coverage for extract_unquoted_default else branch (line 335)
        // When default value doesn't have quotes, it should still be compared correctly
        let from = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped", "cancelled"],
                "cancelled", // unquoted default (no single quotes)
            )],
            vec![],
        )];

        let to = vec![table(
            "orders",
            vec![col_enum_with_default(
                "status",
                "order_status",
                vec!["pending", "shipped"], // 'cancelled' removed
                "pending",                  // unquoted default
            )],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate both actions
        assert_eq!(
            plan.actions.len(),
            2,
            "Expected 2 actions, got: {:?}",
            plan.actions
        );

        // ModifyColumnDefault should come FIRST because unquoted 'cancelled' matches removed value
        assert!(
            matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnDefault { .. }
            ),
            "First action should be ModifyColumnDefault, got: {:?}",
            plan.actions[0]
        );
        assert!(
            matches!(&plan.actions[1], MigrationAction::ModifyColumnType { .. }),
            "Second action should be ModifyColumnType, got: {:?}",
            plan.actions[1]
        );
    }
}
