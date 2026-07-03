use super::*;

// Explicit coverage tests for lines that tarpaulin might miss in rstest
mod coverage_explicit {
    use super::*;

    #[test]
    fn delete_column_explicit() {
        // Covers lines 292-294: DeleteColumn action inside modified table loop
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::DeleteColumn { table, column }
            if table == "users" && column == "name"
        ));
    }

    #[test]
    fn add_column_explicit() {
        // Covers lines 359-362: AddColumn action inside modified table loop
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::AddColumn { table, column, .. }
            if table == "users" && column.name == "email"
        ));
    }

    #[test]
    fn remove_constraint_explicit() {
        // Covers lines 370-372: RemoveConstraint action inside modified table loop
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::RemoveConstraint { table, constraint }
            if table == "users" && matches!(constraint, TableConstraint::Index { name: Some(n), .. } if n == "idx_users_id")
        ));
    }

    #[test]
    fn add_constraint_explicit() {
        // Covers lines 378-380: AddConstraint action inside modified table loop
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )];

        let plan = diff_schemas(&from, &to).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::AddConstraint { table, constraint }
            if table == "users" && matches!(constraint, TableConstraint::Index { name: Some(n), .. } if n == "idx_users_id")
        ));
    }
}

#[test]
fn test_sort_enum_default_dependencies_swaps_when_old_default_removed() {
    // Scenario: enum column "status" changes from [active, pending, done] → [active, done]
    // and default changes from 'pending' → 'active'.
    // The ModifyColumnDefault must come BEFORE ModifyColumnType.
    use vespertide_core::{ComplexColumnType, DefaultValue, EnumValues};

    let enum_type_old = ColumnType::Complex(ComplexColumnType::Enum {
        name: "status_enum".into(),
        values: EnumValues::String(vec!["active".into(), "pending".into(), "done".into()]),
    });
    let enum_type_new = ColumnType::Complex(ComplexColumnType::Enum {
        name: "status_enum".into(),
        values: EnumValues::String(vec!["active".into(), "done".into()]),
    });

    let from = vec![table(
        "orders",
        vec![{
            let mut c = col("status", enum_type_old);
            c.default = Some(DefaultValue::String("'pending'".into()));
            c
        }],
        vec![],
    )];
    let to = vec![table(
        "orders",
        vec![{
            let mut c = col("status", enum_type_new);
            c.default = Some(DefaultValue::String("'active'".into()));
            c
        }],
        vec![],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    // Should have both ModifyColumnDefault and ModifyColumnType
    let has_modify_default = plan
        .actions
        .iter()
        .any(|a| matches!(a, MigrationAction::ModifyColumnDefault { .. }));
    let has_modify_type = plan
        .actions
        .iter()
        .any(|a| matches!(a, MigrationAction::ModifyColumnType { .. }));
    assert!(has_modify_default, "Should have ModifyColumnDefault");
    assert!(has_modify_type, "Should have ModifyColumnType");

    // ModifyColumnDefault should come BEFORE ModifyColumnType
    let default_idx = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::ModifyColumnDefault { .. }))
        .unwrap();
    let type_idx = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::ModifyColumnType { .. }))
        .unwrap();
    assert!(
        default_idx < type_idx,
        "ModifyColumnDefault (idx={default_idx}) must come before ModifyColumnType (idx={type_idx})"
    );
}

/// L71-74: drive the rayon parallel branch of `diff_schemas` so the
/// per-table closure `diff_existing_table(name, &from_map, to_tbl)` is
/// invoked. The branch fires when `to_map.len() >= diff_par_table_threshold()`
/// (default 10_000). Force the threshold to 1 via the documented
/// `VESPERTIDE_DIFF_PAR_THRESHOLD` env-var override so a 2-table
/// schema is enough to exercise it.
#[test]
fn diff_existing_table_invoked_via_parallel_branch() {
    use serial_test::serial;
    // serial_test guards the env-var mutation against parallel test
    // races; opt in via the macro on the wrapping closure.
    #[serial(vespertide_diff_par_threshold)]
    #[expect(
        unsafe_code,
        reason = "std::env::set_var/remove_var are unsafe in edition 2024; serial_test gates concurrent races and the prior value is restored below"
    )]
    fn body() {
        // Snapshot prior env so unrelated tests are unaffected.
        let prior = std::env::var("VESPERTIDE_DIFF_PAR_THRESHOLD").ok();
        // SAFETY: env mutation gated by `#[serial]`; restored below.
        unsafe {
            std::env::set_var("VESPERTIDE_DIFF_PAR_THRESHOLD", "1");
        }

        // Two-table schema with one structural change on each existing
        // table → parallel path runs diff_existing_table for each.
        let from = vec![
            table(
                "a",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("name", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            ),
            table(
                "b",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            ),
        ];
        let to = vec![
            table(
                "a",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            ), // drop `name`
            table(
                "b",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            ), // drop `email`
        ];

        let plan = diff_schemas(&from, &to).expect("parallel diff path produces a plan");
        let deleted: BTreeSet<&str> = plan
            .actions
            .iter()
            .filter_map(|a| match a {
                MigrationAction::DeleteColumn { column, .. } => Some(column.as_str()),
                _ => None,
            })
            .collect();
        // diff_existing_table closure ran for both tables → both
        // `DeleteColumn` actions are emitted.
        assert!(
            deleted.contains("name"),
            "missing DeleteColumn for name; got {:?}",
            plan.actions
        );
        assert!(
            deleted.contains("email"),
            "missing DeleteColumn for email; got {:?}",
            plan.actions
        );

        // Restore previous env to avoid leaking into other tests.
        unsafe {
            match prior {
                Some(v) => std::env::set_var("VESPERTIDE_DIFF_PAR_THRESHOLD", v),
                None => std::env::remove_var("VESPERTIDE_DIFF_PAR_THRESHOLD"),
            }
        }
    }
    body();
}

#[test]
fn test_delete_column_from_existing_table() {
    // Simple column deletion to cover diff.rs line 339
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
            col("age", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![],
    )];
    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            // name and age deleted
        ],
        vec![],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    let delete_cols: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| match a {
            MigrationAction::DeleteColumn { column, .. } => Some(column.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(delete_cols.len(), 2);
    assert!(delete_cols.contains(&"name"));
    assert!(delete_cols.contains(&"age"));
}
