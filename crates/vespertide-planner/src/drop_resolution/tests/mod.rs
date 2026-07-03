use super::*;
use vespertide_core::{ColumnType, DefaultValue, SimpleColumnType, TableConstraint};

// ── helpers ──────────────────────────────────────────────────────────

fn col(name: &str, ty: SimpleColumnType) -> ColumnDef {
    ColumnDef::new(name, ColumnType::Simple(ty), true)
}

fn col_not_null(name: &str, ty: SimpleColumnType) -> ColumnDef {
    ColumnDef::new(name, ColumnType::Simple(ty), false)
}

fn pk(columns: Vec<&str>) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: columns.into_iter().map(Into::into).collect(),
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}

fn table(name: &str, columns: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

fn plan_with(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    }
}

fn delete_column(table: &str, column: &str) -> MigrationAction {
    MigrationAction::DeleteColumn {
        table: table.into(),
        column: column.into(),
    }
}

fn add_column(table: &str, c: ColumnDef) -> MigrationAction {
    MigrationAction::AddColumn {
        table: table.into(),
        column: Box::new(c),
        fill_with: None,
    }
}

// ── candidate discovery ──────────────────────────────────────────────

/// Case 1: column drop with one same-type `AddColumn` → `Match::Exact`.
#[test]
fn case_01_column_rename_same_type() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let plan = plan_with(vec![
        delete_column("user", "email"),
        add_column(
            "user",
            col_not_null("email_address", SimpleColumnType::Text),
        ),
    ]);

    let resolutions = find_drop_resolutions(&plan, &baseline);
    assert_eq!(resolutions.len(), 1);
    let r = &resolutions[0];
    assert_eq!(r.candidates.len(), 1);
    assert_eq!(r.candidates[0].target_name, "email_address");
    assert_eq!(r.candidates[0].match_quality, Match::Exact);
    assert!(r.candidates[0].differences.is_empty());
}

/// A column drop on `user` must only consider rename candidates that are
/// `AddColumn`s on the SAME table. An AddColumn on a DIFFERENT table must not
/// surface as a candidate. Pins the `add_table.as_str() == table` match guard
/// in resolve_column_drop (a `-> true` mutant would gather cross-table adds).
#[test]
fn rename_candidates_are_scoped_to_the_dropped_columns_table() {
    let baseline = vec![
        table(
            "user",
            vec![col_not_null("email", SimpleColumnType::Text)],
            vec![pk(vec!["email"])],
        ),
        table(
            "post",
            vec![col_not_null("id", SimpleColumnType::Integer)],
            vec![pk(vec!["id"])],
        ),
    ];
    let plan = plan_with(vec![
        delete_column("user", "email"),
        // AddColumn on a DIFFERENT table `post` — must NOT be a candidate.
        add_column("post", col_not_null("headline", SimpleColumnType::Text)),
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    assert!(
        r.candidates.is_empty(),
        "AddColumn on a different table must not be a rename candidate: {:?}",
        r.candidates
    );
}

/// Case 2: column drop with type-different `AddColumn` → `Match::Different`
/// (type mismatch dominates the grade).
#[test]
fn case_02_column_rename_type_different() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let plan = plan_with(vec![
        delete_column("user", "email"),
        add_column(
            "user",
            col_not_null("email_address", SimpleColumnType::Integer),
        ),
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    assert_eq!(r.candidates[0].match_quality, Match::Different);
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("type:"))
    );
}

/// Case 3: type same, nullable different → `Match::SameType`.
#[test]
fn case_03_column_rename_nullable_diff() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let plan = plan_with(vec![
        delete_column("user", "email"),
        // nullable: true vs baseline false
        add_column("user", col("email_address", SimpleColumnType::Text)),
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    assert_eq!(r.candidates[0].match_quality, Match::SameType);
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("nullable"))
    );
}

/// Case 4: column drop with no candidates → empty list, prompt collapses
/// to confirm/cancel.
#[test]
fn case_04_column_drop_no_candidates() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let plan = plan_with(vec![delete_column("user", "email")]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    assert!(r.candidates.is_empty());
}

/// Case 5: multiple candidates → sorted by `Match` (`Exact` < `SameType` <
/// Different), then by name.
#[test]
fn case_05_column_drop_multiple_candidates_sorted() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let plan = plan_with(vec![
        delete_column("user", "email"),
        // Different (Integer != Text)
        add_column(
            "user",
            col_not_null("legacy_email", SimpleColumnType::Integer),
        ),
        // Exact
        add_column(
            "user",
            col_not_null("renamed_email", SimpleColumnType::Text),
        ),
        // SameType (nullable differs)
        add_column("user", col("alt_email", SimpleColumnType::Text)),
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    let grades: Vec<_> = r.candidates.iter().map(|c| c.match_quality).collect();
    assert_eq!(
        grades,
        vec![Match::Exact, Match::SameType, Match::Different]
    );
    assert_eq!(r.candidates[0].target_name, "renamed_email");
}

/// Case 6: table drop with column-set identical `CreateTable` → `Match::Exact`.
#[test]
fn case_06_table_rename_same_columns() {
    let baseline = vec![table(
        "old_user",
        vec![
            col_not_null("id", SimpleColumnType::Integer),
            col_not_null("name", SimpleColumnType::Text),
        ],
        vec![pk(vec!["id"])],
    )];
    let plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old_user".into(),
        },
        MigrationAction::CreateTable {
            table: "new_user".into(),
            columns: vec![
                col_not_null("id", SimpleColumnType::Integer),
                col_not_null("name", SimpleColumnType::Text),
            ],
            constraints: vec![pk(vec!["id"])],
        },
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    assert_eq!(r.candidates[0].match_quality, Match::Exact);
    assert_eq!(r.candidates[0].target_name, "new_user");
}

/// Case 7: table drop with column-set differing → `Match::Different` + diff list.
#[test]
fn case_07_table_rename_column_diff() {
    let baseline = vec![table(
        "old_user",
        vec![
            col_not_null("id", SimpleColumnType::Integer),
            col_not_null("name", SimpleColumnType::Text),
        ],
        vec![pk(vec!["id"])],
    )];
    let plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old_user".into(),
        },
        MigrationAction::CreateTable {
            table: "new_user".into(),
            columns: vec![
                col_not_null("id", SimpleColumnType::Integer),
                col_not_null("email", SimpleColumnType::Text),
            ],
            constraints: vec![pk(vec!["id"])],
        },
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    assert_eq!(r.candidates[0].match_quality, Match::Different);
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("removed columns: name"))
    );
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("added columns: email"))
    );
}

// ── apply_drop_resolution ────────────────────────────────────────────

/// Case 8: `Drop` choice is a no-op — original plan unchanged.
#[test]
fn case_08_apply_drop_choice_is_noop() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let mut plan = plan_with(vec![delete_column("user", "email")]);
    let resolutions = find_drop_resolutions(&plan, &baseline);
    let before = plan.clone();

    apply_drop_resolution(&mut plan, &baseline, &resolutions[0], &DropChoice::Drop).unwrap();

    assert_eq!(plan, before);
}

/// Case 9: `RenameTo` for a same-type column → `DeleteColumn` + `AddColumn`
/// collapse into a single `RenameColumn` (no follow-up modifications).
#[test]
fn case_09_apply_rename_same_type_emits_rename_only() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let mut plan = plan_with(vec![
        delete_column("user", "email"),
        add_column(
            "user",
            col_not_null("email_address", SimpleColumnType::Text),
        ),
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("email_address".to_string()),
    )
    .unwrap();

    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RenameColumn { table, from, to }
            if table.as_str() == "user"
                && from.as_str() == "email"
                && to.as_str() == "email_address"
    ));
}

/// Case 10: `RenameTo` with type difference emits `RenameColumn` +
/// `ModifyColumnType` in deterministic order.
#[test]
fn case_10_apply_rename_with_type_diff_emits_modify() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let mut plan = plan_with(vec![
        delete_column("user", "email"),
        add_column(
            "user",
            col_not_null("email_address", SimpleColumnType::Integer),
        ),
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("email_address".to_string()),
    )
    .unwrap();

    assert_eq!(plan.actions.len(), 2);
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RenameColumn { .. }
    ));
    assert!(matches!(
        &plan.actions[1],
        MigrationAction::ModifyColumnType { .. }
    ));
}

/// Case 11: `RenameTo` with nullable + default differences emits the
/// matching `ModifyColumn*` actions after `RenameColumn`.
#[test]
fn case_11_apply_rename_with_nullable_and_default_diff() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let mut new_col = col("email_address", SimpleColumnType::Text);
    new_col.default = Some(DefaultValue::from("''"));
    let mut plan = plan_with(vec![
        delete_column("user", "email"),
        add_column("user", new_col),
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("email_address".to_string()),
    )
    .unwrap();

    // Expected: RenameColumn + ModifyColumnNullable + ModifyColumnDefault.
    // ModifyColumnType is absent because the type did not change.
    assert_eq!(plan.actions.len(), 3);
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RenameColumn { .. }
    ));
    assert!(matches!(
        &plan.actions[1],
        MigrationAction::ModifyColumnNullable { .. }
    ));
    assert!(matches!(
        &plan.actions[2],
        MigrationAction::ModifyColumnDefault { .. }
    ));
}

/// Case 12: Table `RenameTo` with column-set identical → only
/// `RenameTable` emitted.
#[test]
fn case_12_apply_table_rename_same_columns() {
    let baseline = vec![table(
        "old_user",
        vec![
            col_not_null("id", SimpleColumnType::Integer),
            col_not_null("name", SimpleColumnType::Text),
        ],
        vec![pk(vec!["id"])],
    )];
    let mut plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old_user".into(),
        },
        MigrationAction::CreateTable {
            table: "new_user".into(),
            columns: vec![
                col_not_null("id", SimpleColumnType::Integer),
                col_not_null("name", SimpleColumnType::Text),
            ],
            constraints: vec![pk(vec!["id"])],
        },
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("new_user".to_string()),
    )
    .unwrap();

    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RenameTable { from, to }
            if from.as_str() == "old_user" && to.as_str() == "new_user"
    ));
}

/// Case 13: Table `RenameTo` with column-set differing → `RenameTable`
/// followed by the column-level diff actions.
#[test]
fn case_13_apply_table_rename_with_column_diff() {
    let baseline = vec![table(
        "old_user",
        vec![
            col_not_null("id", SimpleColumnType::Integer),
            col_not_null("name", SimpleColumnType::Text),
        ],
        vec![pk(vec!["id"])],
    )];
    let mut plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old_user".into(),
        },
        MigrationAction::CreateTable {
            table: "new_user".into(),
            columns: vec![
                col_not_null("id", SimpleColumnType::Integer),
                col_not_null("email", SimpleColumnType::Text),
            ],
            constraints: vec![pk(vec!["id"])],
        },
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("new_user".to_string()),
    )
    .unwrap();

    // Expected: RenameTable, DeleteColumn(name), AddColumn(email).
    // Constraints unchanged (both have PK on id) so no constraint actions.
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RenameTable { .. }
    ));
    assert!(plan.actions.iter().any(|a| matches!(
        a,
        MigrationAction::DeleteColumn { column, .. } if column.as_str() == "name"
    )));
    assert!(plan.actions.iter().any(|a| matches!(
        a,
        MigrationAction::AddColumn { column, .. } if column.name.as_str() == "email"
    )));
}

/// Case 14: `RenameTo` with a target that does not exist in the plan
/// → returns `TargetActionMissing` (programmer error guard).
#[test]
fn case_14_apply_rename_missing_target_errors() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let mut plan = plan_with(vec![delete_column("user", "email")]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    let err = apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("nonexistent".to_string()),
    )
    .unwrap_err();

    assert!(matches!(err, DropResolutionError::TargetActionMissing(_)));
}

// ── Coverage-closure: dropped-column-not-in-baseline / table-not-in-
// baseline / apply_*_rename DropActionMissing / table constraint diff /
// DropTarget::label Table / render_default None+Some arms ──

/// Column drop targeting a column the baseline does not know about
/// (e.g. baseline has the table but the column was never declared) →
/// `dropped` is `None`. `column_candidate` returns
/// `Match::Different` with "dropped column not found in baseline"
/// (lines 241-247) and `render_column_type` falls back to
/// `"(unknown)"` (line 170).
#[test]
fn case_15_column_drop_baseline_missing_column_uses_unknown_label() {
    let baseline = vec![table(
        "user",
        // baseline has only `id`, not `email`
        vec![col_not_null("id", SimpleColumnType::Integer)],
        vec![pk(vec!["id"])],
    )];
    let plan = plan_with(vec![
        delete_column("user", "email"),
        add_column(
            "user",
            col_not_null("email_address", SimpleColumnType::Text),
        ),
    ]);

    let r = &find_drop_resolutions(&plan, &baseline)[0];
    // Column type rendered as "(unknown)" because baseline lookup failed.
    let DropTarget::Column { column_type, .. } = &r.target else {
        panic!("expected Column target");
    };
    assert_eq!(column_type, "(unknown)");
    // The candidate carries the "not found in baseline" hint.
    assert_eq!(r.candidates.len(), 1);
    assert_eq!(r.candidates[0].match_quality, Match::Different);
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("not found in baseline"))
    );
}

/// Table drop where the baseline doesn't contain the table at all →
/// `baseline_columns` becomes empty (line 207-208 `unwrap_or_default`)
/// and the candidate grades to `Different` because every new column
/// looks "added".
#[test]
fn case_16_table_drop_baseline_missing_table_uses_empty_columns() {
    let baseline: Vec<TableDef> = vec![];
    let plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old".into(),
        },
        MigrationAction::CreateTable {
            table: "new".into(),
            columns: vec![col_not_null("id", SimpleColumnType::Integer)],
            constraints: vec![pk(vec!["id"])],
        },
    ]);
    let r = &find_drop_resolutions(&plan, &baseline)[0];
    // Single candidate, Different grade (only_in_new = ["id"]).
    assert_eq!(r.candidates.len(), 1);
    assert_eq!(r.candidates[0].match_quality, Match::Different);
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("added columns"))
    );
}

/// `apply_drop_resolution` for a column whose `DeleteColumn` action was
/// already removed from the plan → `DropActionMissing` (lines 422-430).
#[test]
fn case_17_apply_column_rename_no_delete_action_errors() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    // Build a resolution manually for `email → email_address` but
    // hand `apply_drop_resolution` a plan that contains ONLY the
    // AddColumn, no DeleteColumn.
    let resolution = DropResolution {
        action_index: 0,
        target: DropTarget::Column {
            table: "user".into(),
            column: "email".into(),
            column_type: "text NOT NULL".into(),
        },
        candidates: vec![],
    };
    let mut plan = plan_with(vec![add_column(
        "user",
        col_not_null("email_address", SimpleColumnType::Text),
    )]);

    let err = apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolution,
        &DropChoice::RenameTo("email_address".to_string()),
    )
    .unwrap_err();
    assert!(matches!(err, DropResolutionError::DropActionMissing(_)));
}

/// `apply_drop_resolution` for a table whose `DeleteTable` action was
/// already removed from the plan → `DropActionMissing`
/// (lines 521-528).
#[test]
fn case_18_apply_table_rename_no_delete_action_errors() {
    let baseline = vec![table(
        "old",
        vec![col_not_null("id", SimpleColumnType::Integer)],
        vec![pk(vec!["id"])],
    )];
    let resolution = DropResolution {
        action_index: 0,
        target: DropTarget::Table { name: "old".into() },
        candidates: vec![],
    };
    let mut plan = plan_with(vec![MigrationAction::CreateTable {
        table: "new".into(),
        columns: vec![col_not_null("id", SimpleColumnType::Integer)],
        constraints: vec![pk(vec!["id"])],
    }]);

    let err = apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolution,
        &DropChoice::RenameTo("new".to_string()),
    )
    .unwrap_err();
    assert!(matches!(err, DropResolutionError::DropActionMissing(_)));
}

/// `apply_drop_resolution` for a table rename where the new declaration
/// adds a NEW constraint absent from the baseline → emits a
/// `RemoveConstraint` for the old PK + `AddConstraint` for the new
/// PK as part of the rename follow-ups (lines 656-679, both arms).
#[test]
fn case_19_apply_table_rename_with_constraint_changes_emits_diff() {
    let baseline = vec![table(
        "old",
        vec![
            col_not_null("a", SimpleColumnType::Integer),
            col_not_null("b", SimpleColumnType::Integer),
        ],
        vec![pk(vec!["a"])],
    )];
    let mut plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old".into(),
        },
        MigrationAction::CreateTable {
            table: "new".into(),
            columns: vec![
                col_not_null("a", SimpleColumnType::Integer),
                col_not_null("b", SimpleColumnType::Integer),
            ],
            // PK shifts a → b.
            constraints: vec![pk(vec!["b"])],
        },
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("new".to_string()),
    )
    .unwrap();

    // RenameTable + RemoveConstraint(PK on a) + AddConstraint(PK on b).
    assert!(matches!(
        &plan.actions[0],
        MigrationAction::RenameTable { .. }
    ));
    assert!(
        plan.actions
            .iter()
            .any(|a| matches!(a, MigrationAction::RemoveConstraint { .. }))
    );
    assert!(
        plan.actions
            .iter()
            .any(|a| matches!(a, MigrationAction::AddConstraint { .. }))
    );
}

/// `DropTarget::label` for `Table` variant (lines 73-75 `Self::Table`).
/// The Column variant is reached by every column-drop test, but the
/// Table arm needs an explicit check.
#[test]
fn case_20_drop_target_label_table_returns_name() {
    let t = DropTarget::Table {
        name: "users".into(),
    };
    assert_eq!(t.label(), "users");
    let c = DropTarget::Column {
        table: "t".into(),
        column: "c".into(),
        column_type: "text".into(),
    };
    assert_eq!(c.label(), "t.c");
}

/// `render_default` `None` arm (line 348: `"(none)"`). Reached when a
/// column with NO default is compared against an added column WITH a
/// default — the `default: (none) → '...'` diff line surfaces.
#[test]
fn case_21_column_diff_default_none_to_some_renders_none_label() {
    let baseline = vec![table(
        "user",
        vec![col_not_null("email", SimpleColumnType::Text)],
        vec![pk(vec!["email"])],
    )];
    let mut new_col = col_not_null("email_address", SimpleColumnType::Text);
    new_col.default = Some(DefaultValue::from("'placeholder'"));
    let plan = plan_with(vec![
        delete_column("user", "email"),
        add_column("user", new_col),
    ]);
    let r = &find_drop_resolutions(&plan, &baseline)[0];
    // Default changed → SameType grade with a "default:" diff line.
    assert_eq!(r.candidates[0].match_quality, Match::SameType);
    assert!(
        r.candidates[0]
            .differences
            .iter()
            .any(|d| d.contains("default") && d.contains("(none)"))
    );
}

/// Bare `DropResolutionError` Display impl: both variants format with
/// the inner string. Locks the user-facing text.
#[test]
fn case_22_drop_resolution_error_display() {
    let e = DropResolutionError::DropActionMissing("DeleteColumn user.email".into());
    assert!(e.to_string().contains("drop action not found in plan"));
    let e = DropResolutionError::TargetActionMissing("AddColumn user.email_address".into());
    assert!(e.to_string().contains("rename target action not found"));
}

#[test]
fn case_23_apply_table_rename_with_same_column_property_changes() {
    let mut old_count = col_not_null("count", SimpleColumnType::Integer);
    old_count.default = Some(DefaultValue::from("0"));
    let baseline = vec![table(
        "old",
        vec![col_not_null("id", SimpleColumnType::Integer), old_count],
        vec![pk(vec!["id"])],
    )];

    let mut new_count = col("count", SimpleColumnType::BigInt);
    new_count.default = Some(DefaultValue::from("1"));
    let mut plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "old".into(),
        },
        MigrationAction::CreateTable {
            table: "new".into(),
            columns: vec![col_not_null("id", SimpleColumnType::Integer), new_count],
            constraints: vec![pk(vec!["id"])],
        },
    ]);
    let resolutions = find_drop_resolutions(&plan, &baseline);

    apply_drop_resolution(
        &mut plan,
        &baseline,
        &resolutions[0],
        &DropChoice::RenameTo("new".to_string()),
    )
    .unwrap();

    assert!(plan.actions.iter().any(|a| matches!(a, MigrationAction::ModifyColumnType { column, .. } if column.as_str() == "count")));
    assert!(plan.actions.iter().any(|a| matches!(a, MigrationAction::ModifyColumnNullable { column, nullable, .. } if column.as_str() == "count" && *nullable)));
    assert!(plan.actions.iter().any(|a| matches!(a, MigrationAction::ModifyColumnDefault { column, new_default, .. } if column.as_str() == "count" && new_default.as_deref() == Some("1"))));
}
