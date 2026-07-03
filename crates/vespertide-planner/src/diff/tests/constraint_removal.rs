use super::*;

mod constraint_removal_on_deleted_columns {
    use super::*;

    fn fk(columns: Vec<&str>, ref_table: &str, ref_columns: Vec<&str>) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: None,
            columns: columns.into_iter().map(Into::into).collect(),
            ref_table: ref_table.into(),
            ref_columns: ref_columns.into_iter().map(Into::into).collect(),
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }
    }

    #[test]
    fn skip_remove_constraint_when_all_columns_deleted() {
        // When a column with FK and index is deleted, the constraints should NOT
        // generate separate RemoveConstraint actions (they are dropped with the column)
        let from = vec![table(
            "project",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("template_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            vec![
                fk(vec!["template_id"], "book_template", vec!["id"]),
                idx("ix_project__template_id", vec!["template_id"]),
            ],
        )];

        let to = vec![table(
            "project",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should only have DeleteColumn, NO RemoveConstraint actions
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::DeleteColumn { table, column }
            if table == "project" && column == "template_id"
        ));

        // Explicitly verify no RemoveConstraint
        let has_remove_constraint = plan
            .actions
            .iter()
            .any(|a| matches!(a, MigrationAction::RemoveConstraint { .. }));
        assert!(
            !has_remove_constraint,
            "Should NOT have RemoveConstraint when column is deleted"
        );
    }

    #[test]
    fn keep_remove_constraint_when_only_some_columns_deleted() {
        // If a composite constraint has some columns remaining, RemoveConstraint is needed
        let from = vec![table(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            vec![idx(
                "ix_orders__user_product",
                vec!["user_id", "product_id"],
            )],
        )];

        let to = vec![table(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                // product_id is deleted, but user_id remains
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should have both DeleteColumn AND RemoveConstraint
        // (because user_id is still there, the composite index needs explicit removal)
        let has_delete_column = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::DeleteColumn { table, column }
                if table == "orders" && column == "product_id"
            )
        });
        assert!(has_delete_column, "Should have DeleteColumn for product_id");

        let has_remove_constraint = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::RemoveConstraint { table, .. }
                if table == "orders"
            )
        });
        assert!(
            has_remove_constraint,
            "Should have RemoveConstraint for composite index when only some columns deleted"
        );
    }

    #[test]
    fn skip_remove_constraint_when_all_composite_columns_deleted() {
        // If ALL columns of a composite constraint are deleted, skip RemoveConstraint
        let from = vec![table(
            "orders",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            vec![idx(
                "ix_orders__user_product",
                vec!["user_id", "product_id"],
            )],
        )];

        let to = vec![table(
            "orders",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should only have DeleteColumn actions, no RemoveConstraint
        let delete_columns: Vec<_> = plan
            .actions
            .iter()
            .filter(|a| matches!(a, MigrationAction::DeleteColumn { .. }))
            .collect();
        assert_eq!(
            delete_columns.len(),
            2,
            "Should have 2 DeleteColumn actions"
        );

        let has_remove_constraint = plan
            .actions
            .iter()
            .any(|a| matches!(a, MigrationAction::RemoveConstraint { .. }));
        assert!(
            !has_remove_constraint,
            "Should NOT have RemoveConstraint when all composite columns deleted"
        );
    }

    #[test]
    fn keep_remove_constraint_when_no_columns_deleted() {
        // Normal case: constraint removed but columns remain
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![idx("ix_users__email", vec!["email"])],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![], // Index removed but column remains
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::RemoveConstraint { table, .. }
            if table == "users"
        ));
    }
}

#[test]
fn diff_detects_replace_fk_on_delete() {
    // Changing FK on_delete should produce a ReplaceConstraint, not Remove+Add
    let from = vec![table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    )];
    let to = vec![table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(vespertide_core::ReferenceAction::Cascade),
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    )];
    let plan = diff_schemas(&from, &to).unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert!(
        matches!(&plan.actions[0], MigrationAction::ReplaceConstraint { table, .. } if table == "posts")
    );
}

#[test]
fn diff_detects_replace_unique_constraint() {
    // Unique identity is by columns; changing name on same columns = replace
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![TableConstraint::Unique {
            name: Some("uq_old".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];
    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![TableConstraint::Unique {
            name: Some("uq_new".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];
    let plan = diff_schemas(&from, &to).unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert!(
        matches!(&plan.actions[0], MigrationAction::ReplaceConstraint { table, .. } if table == "users")
    );
}

#[test]
fn diff_detects_replace_check_constraint() {
    // Changing Check expr with same name → ReplaceConstraint
    let from = vec![table(
        "users",
        vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }],
    )];
    let to = vec![table(
        "users",
        vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![TableConstraint::Check {
            name: "chk_age".into(),
            expr: "age > 18".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }],
    )];
    let plan = diff_schemas(&from, &to).unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert!(
        matches!(&plan.actions[0], MigrationAction::ReplaceConstraint { table, .. } if table == "users")
    );
}

#[test]
fn diff_detects_replace_index_constraint() {
    // Changing Index name but same columns → ReplaceConstraint
    let from = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![idx("ix_old", vec!["email"])],
    )];
    let to = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![TableConstraint::Index {
            name: Some("ix_new".into()),
            columns: vec!["email".into()],
        }],
    )];
    let plan = diff_schemas(&from, &to).unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert!(
        matches!(&plan.actions[0], MigrationAction::ReplaceConstraint { table, .. } if table == "users")
    );
}

#[test]
fn diff_already_paired_constraint_not_double_matched() {
    // Two unique constraints on different columns both getting renamed.
    // The "already paired" check ensures the second from finds the second to
    // rather than pairing with the already-matched first to.
    let from = vec![table(
        "users",
        vec![
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![
            TableConstraint::Unique {
                name: Some("uq_email_old".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
            TableConstraint::Unique {
                name: Some("uq_name_old".into()),
                columns: vec!["name".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    )];
    let to = vec![table(
        "users",
        vec![
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![
            TableConstraint::Unique {
                name: Some("uq_email_new".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
            TableConstraint::Unique {
                name: Some("uq_name_new".into()),
                columns: vec!["name".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    )];
    let plan = diff_schemas(&from, &to).unwrap();
    // Should get exactly 2 ReplaceConstraint actions, not 1
    let replace_count = plan
        .actions
        .iter()
        .filter(|a| matches!(a, MigrationAction::ReplaceConstraint { .. }))
        .count();
    assert_eq!(
        replace_count, 2,
        "Expected 2 ReplaceConstraint actions, got: {:?}",
        plan.actions
    );
}

#[test]
fn diff_mismatched_constraint_types_not_paired() {
    // Removing a Unique and adding an Index on the same columns should NOT produce
    // ReplaceConstraint — they are different constraint types (hits _ => false branch).
    let from = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];
    let to = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![idx("ix_email", vec!["email"])],
    )];
    let plan = diff_schemas(&from, &to).unwrap();
    // Should be Remove + Add, not ReplaceConstraint
    let replace_count = plan
        .actions
        .iter()
        .filter(|a| matches!(a, MigrationAction::ReplaceConstraint { .. }))
        .count();
    assert_eq!(
        replace_count, 0,
        "Mismatched types should not produce ReplaceConstraint, got: {:?}",
        plan.actions
    );
    assert_eq!(plan.actions.len(), 2);
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RemoveConstraint { .. }
    ));
    assert!(matches!(
        &plan.actions[1],
        MigrationAction::AddConstraint { .. }
    ));
}
