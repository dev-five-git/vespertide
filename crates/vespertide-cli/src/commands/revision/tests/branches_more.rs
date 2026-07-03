//! Continuation of `branches.rs` test cases (kept under the 1200-line cap by
//! splitting at the helper boundary). Shares helpers via `super::branches::*`.

use super::branches::*;
use super::*;
use vespertide_core::{
    CheckViolationStrategy, ComplexColumnType, ForeignKeyOrphanStrategy, ReferenceAction,
    schema::foreign_key::ForeignKeySyntax,
};

// Adding a NOT-NULL, no-default, non-FK column to an existing table requires a
// fill_with value, collected interactively. A value-supplying mock provides a
// unique sentinel; the written migration must embed it. Pins
// `if !missing.is_empty()` (mod.rs:492): a `delete !` mutant would skip
// collection entirely, leaving the sentinel out of the migration.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_collects_interactive_fill_with_into_migration() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let mut target = baseline.clone();
    // New NOT-NULL, no-default, non-FK column -> needs fill_with.
    target.columns.push(int_col("age", false));
    write_project_with_tables(&cfg, vec![baseline], vec![target]);

    let fill_with: FillWithFn = |_, _| Ok("424242".to_string());
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        fill_with,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add age".into(), vec![], vec![], prompts)
        .await
        .unwrap();

    // Read the newly written migration (0002_*) and confirm the collected
    // sentinel landed in it.
    let new_migration = std::fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("0002"))
        })
        .expect("a second migration must be written");
    let body = std::fs::read_to_string(&new_migration).unwrap();
    assert!(
        body.contains("424242"),
        "interactively collected fill_with must be embedded in the migration: {body}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_fk_orphan_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let (users, posts) = users_posts_without_fk(true);
    let mut posts_model = posts.clone();
    posts_model.columns[1].foreign_key = Some(ForeignKeySyntax::String("users.id".into()));
    write_project_with_tables(&cfg, vec![users.clone(), posts], vec![users, posts_model]);
    let fk_orphan_addition: FkOrphanAdditionFn = |_| Ok(None);
    let prompts = RevisionPromptFns {
        fk_orphan_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add fk".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_fk_orphan_choice_applies_and_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let (users, posts) = users_posts_without_fk(true);
    let mut posts_model = posts.clone();
    posts_model.columns[1].foreign_key = Some(ForeignKeySyntax::String("users.id".into()));
    write_project_with_tables(&cfg, vec![users.clone(), posts], vec![users, posts_model]);
    let fk_orphan_addition: FkOrphanAdditionFn = |_| Ok(Some(FkOrphanChoice::Nullify));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        fk_orphan_addition,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add fk".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_check_addition_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false), int_col("age", true)],
        vec![pk_constraint("id")],
    );
    let mut target = baseline.clone();
    target.constraints.push(TableConstraint::Check {
        name: "check_age_positive".into(),
        expr: "age > 0".into(),
        strategy: CheckViolationStrategy::default(),
    });
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let check_addition: CheckAdditionFn = |_| Ok(None);
    let prompts = RevisionPromptFns {
        check_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add check".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_check_addition_choice_applies_and_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false), int_col("age", true)],
        vec![pk_constraint("id")],
    );
    let mut target = baseline.clone();
    target.constraints.push(TableConstraint::Check {
        name: "check_age_positive".into(),
        expr: "age > 0".into(),
        strategy: CheckViolationStrategy::default(),
    });
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let check_addition: CheckAdditionFn = |_| {
        Ok(Some(CheckViolationChoice::Nullify {
            column: "age".into(),
        }))
    };
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        check_addition,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add check".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_pk_addition_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false), text_col("email", true)],
        vec![],
    );
    let target = table_def("users", baseline.columns.clone(), vec![pk_constraint("id")]);
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let pk_addition: PkAdditionFn = |_| Ok(None);
    let prompts = RevisionPromptFns {
        pk_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add pk".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_sequence_exhaustion_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![col(
            "id",
            ColumnType::Simple(SimpleColumnType::BigInt),
            false,
        )],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "users",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let type_narrowing: TypeNarrowingFn =
        |_| Ok(Some(vec![vespertide_core::NarrowingStrategy::Delete]));
    let sequence_exhaustion: SequenceExhaustionFn = |_| Ok(None);
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        type_narrowing,
        sequence_exhaustion,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("create risky pk".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_type_narrowing_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false), text_col("name", true)],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "users",
        vec![
            int_col("id", false),
            col(
                "name",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 5 }),
                true,
            ),
        ],
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let type_narrowing: TypeNarrowingFn = |_| Ok(None);
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        type_narrowing,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("narrow name".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_remap_enum_cancel_branch_aborts_generic_plan() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let mut target = base_users();
    target.columns.push(text_col("nickname", true));
    write_project_with_tables(&cfg, vec![base_users()], vec![target]);
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(false);
    let prompts = RevisionPromptFns {
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add nullable".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_fk_policy_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let users = table_def(
        "users",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let baseline_posts = table_def(
        "posts",
        vec![int_col("id", false), int_col("user_id", false)],
        vec![
            pk_constraint("id"),
            TableConstraint::ForeignKey {
                name: Some("fk_posts_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Restrict),
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        ],
    );
    let target_posts = table_def(
        "posts",
        baseline_posts.columns.clone(),
        vec![
            pk_constraint("id"),
            TableConstraint::ForeignKey {
                name: Some("fk_posts_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        ],
    );
    write_project_with_tables(
        &cfg,
        vec![users.clone(), baseline_posts],
        vec![users, target_posts],
    );
    let fk_policy_change: FkPolicyChangeFn = |_| Ok(false);
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        fk_policy_change,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("policy change".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

// ── F12 hard error: PK removal without replacement ──────────────────────

/// Single PK removal → bare `PlannerError::PrimaryKeyRemovedWithoutReplacement`
/// → covers mod.rs:164 (`1 => Some(f12_errors.remove(0))`) + line 167
/// (`return Err(anyhow::anyhow!("{err}"));`).
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_f12_single_pk_removal_hard_errors() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let target = table_def("users", vec![int_col("id", false)], vec![]); // PK dropped, no replacement
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let prompts = panic_guard_prompt_fns();
    let res = cmd_revision_core("drop pk".into(), vec![], vec![], prompts).await;
    let err = res.unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("primary key") || err.to_lowercase().contains("primary"),
        "expected PK removal hard error; got: {err}"
    );
    assert_eq!(migration_count(&cfg), 1, "no v2 must be written");
}

/// Multiple PK removals → `PlannerError::Multiple` → covers mod.rs:165
/// (`_ => Some(PlannerError::Multiple(...))`) + line 167.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_f12_multiple_pk_removals_yield_multiple_error() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline_a = table_def(
        "a_tbl",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let baseline_b = table_def(
        "b_tbl",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let target_a = table_def("a_tbl", vec![int_col("id", false)], vec![]);
    let target_b = table_def("b_tbl", vec![int_col("id", false)], vec![]);
    write_project_with_tables(&cfg, vec![baseline_a, baseline_b], vec![target_a, target_b]);
    let prompts = panic_guard_prompt_fns();
    let res = cmd_revision_core("drop pks".into(), vec![], vec![], prompts).await;
    let err = res.unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("primary"),
        "expected primary key error; got: {err}"
    );
    assert_eq!(migration_count(&cfg), 1);
}

// ── F3 Edge#1 hard error: AddColumn FK requires nullable ────────────────

/// Single Edge#1 violation → covers mod.rs:191 (`1 => Some(... next() ...)`)
/// + line 194 (`return Err(anyhow::anyhow!("{err}"));`).
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_edge1_single_addcolumn_fk_nonnull_with_default_hard_errors() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline_users = table_def(
        "users",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let baseline_posts = table_def(
        "posts",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let target_users = baseline_users.clone();
    let mut target_posts = baseline_posts.clone();
    target_posts.columns.push(ColumnDef {
        name: "user_id".into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: Some(vespertide_core::StringOrBool::String("1".into())),
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: Some(ForeignKeySyntax::String("users.id".into())),
    });
    write_project_with_tables(
        &cfg,
        vec![baseline_users, baseline_posts],
        vec![target_users, target_posts],
    );
    let prompts = panic_guard_prompt_fns();
    let res = cmd_revision_core("add fk col".into(), vec![], vec![], prompts).await;
    let err = res.unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("nullable") || err.to_lowercase().contains("foreign"),
        "expected Edge#1 hard error; got: {err}"
    );
    // A SINGLE violation must be returned BARE, not wrapped in
    // PlannerError::Multiple (whose Display renders a "validation violation(s):"
    // header). Pins the `1 => Some(... next() ...)` match arm: deleting it
    // would fall through to the `_ => Multiple(...)` arm.
    assert!(
        !err.contains("validation violation"),
        "single Edge#1 error must be bare, not a Multiple list: {err}"
    );
    assert_eq!(migration_count(&cfg), 1);
}

/// Two Edge#1 violations across two tables → `PlannerError::Multiple` →
/// covers mod.rs:192 (`_ => Some(PlannerError::Multiple(...))`) + 194.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_edge1_multiple_violations_yield_multiple_error() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline_users = table_def(
        "users",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let baseline_posts = table_def(
        "posts",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let baseline_comments = table_def(
        "comments",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let target_users = baseline_users.clone();
    let mut target_posts = baseline_posts.clone();
    target_posts.columns.push(ColumnDef {
        name: "user_id".into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: Some(vespertide_core::StringOrBool::String("1".into())),
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: Some(ForeignKeySyntax::String("users.id".into())),
    });
    let mut target_comments = baseline_comments.clone();
    target_comments.columns.push(ColumnDef {
        name: "user_id".into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: Some(vespertide_core::StringOrBool::String("1".into())),
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: Some(ForeignKeySyntax::String("users.id".into())),
    });
    write_project_with_tables(
        &cfg,
        vec![baseline_users, baseline_posts, baseline_comments],
        vec![target_users, target_posts, target_comments],
    );
    let prompts = panic_guard_prompt_fns();
    let res = cmd_revision_core("add fk cols".into(), vec![], vec![], prompts).await;
    let err = res.unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("nullable") || err.to_lowercase().contains("foreign"),
        "expected Edge#1 hard error; got: {err}"
    );
    assert_eq!(migration_count(&cfg), 1);
}

// ── F5 PK addition CHOICE apply path (line 239) ─────────────────────────

/// User picks `ContinueWithoutCleanup` → covers mod.rs:239
/// (`prompts::apply_pk_addition_choice(...)`).
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_pk_addition_continue_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "widgets",
        vec![int_col("id", false), text_col("label", true)],
        vec![],
    );
    let target = table_def(
        "widgets",
        baseline.columns.clone(),
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let pk_addition: PkAdditionFn = |_| Ok(Some(PrimaryKeyAdditionChoice::ContinueWithoutCleanup));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        pk_addition,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("add pk".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

// ── F96 cascade-reach Cancel path (lines 249, 250, 251) ─────────────────

/// User cancels the cascade-reach prompt → covers the
/// `if cascade_reach_prompt_fn(warning)?.is_none()` Cancel branch
/// (println! + return Ok(())) at mod.rs:249-251.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_cascade_reach_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    // Baseline: parent + two CASCADE children already present + one bare
    // child without an FK yet.
    let parent = table_def(
        "parent_t",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let child1 = table_def(
        "child1_t",
        vec![int_col("id", false), int_col("parent_id", true)],
        vec![
            pk_constraint("id"),
            TableConstraint::ForeignKey {
                name: Some("fk_child1".into()),
                columns: vec!["parent_id".into()],
                ref_table: "parent_t".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        ],
    );
    let child2 = table_def(
        "child2_t",
        vec![int_col("id", false), int_col("parent_id", true)],
        vec![
            pk_constraint("id"),
            TableConstraint::ForeignKey {
                name: Some("fk_child2".into()),
                columns: vec!["parent_id".into()],
                ref_table: "parent_t".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        ],
    );
    let child3_base = table_def(
        "child3_t",
        vec![int_col("id", false), int_col("parent_id", true)],
        vec![pk_constraint("id")],
    );
    // Model: add a CASCADE FK to child3 → HighFanout (parent has 3 cascade children now).
    let mut child3_target = child3_base.clone();
    child3_target.constraints.push(TableConstraint::ForeignKey {
        name: Some("fk_child3".into()),
        columns: vec!["parent_id".into()],
        ref_table: "parent_t".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(ReferenceAction::Cascade),
        on_update: None,
        orphan_strategy: ForeignKeyOrphanStrategy::default(),
    });
    write_project_with_tables(
        &cfg,
        vec![parent.clone(), child1.clone(), child2.clone(), child3_base],
        vec![parent, child1, child2, child3_target],
    );
    let cascade_reach: CascadeReachFn = |_| Ok(None);
    // fk_orphan_addition fires for AddConstraint(FK) on existing column; pick Nullify to advance.
    let fk_orphan_addition: FkOrphanAdditionFn = |_| Ok(Some(FkOrphanChoice::Nullify));
    let prompts = RevisionPromptFns {
        cascade_reach,
        fk_orphan_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("extend cascade".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1, "Cancel must NOT write v2");
}

// ── F76 sequence_exhaustion CHOICE apply path (line 268) ────────────────

/// User picks `ChangeToBigInt` → covers mod.rs:268
/// (`prompts::apply_sequence_exhaustion_choice(...)`).
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_sequence_exhaustion_change_to_bigint_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    // Narrow BigInt PK → Integer to trigger sequence exhaustion warning on
    // the resulting ModifyColumnType action.
    let baseline = table_def(
        "events",
        vec![col(
            "id",
            ColumnType::Simple(SimpleColumnType::BigInt),
            false,
        )],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "events",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let type_narrowing: TypeNarrowingFn =
        |_| Ok(Some(vec![vespertide_core::NarrowingStrategy::Delete]));
    let sequence_exhaustion: SequenceExhaustionFn =
        |_| Ok(Some(SequenceExhaustionChoice::ChangeToBigInt));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        type_narrowing,
        sequence_exhaustion,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("widen pk".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

// ── F29 check_strengthening Cancel path (lines 283, 284, 285) ───────────

/// CHECK constraint replaced by a stricter predicate (`age > 0` → `age > 10`).
/// User cancels the strengthening prompt → covers the
/// `let Some(_choice) = check_strengthening_prompt_fn(warning)? else { ... }`
/// Cancel branch at mod.rs:283-286.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_check_strengthening_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "members",
        vec![int_col("id", false), int_col("age", true)],
        vec![
            pk_constraint("id"),
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
        ],
    );
    let target = table_def(
        "members",
        baseline.columns.clone(),
        vec![
            pk_constraint("id"),
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 10".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
        ],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let check_strengthening: CheckStrengtheningFn = |_| Ok(None);
    // CHECK addition prompt may fire because the planner emits
    // RemoveConstraint+AddConstraint for the strengthened CHECK.
    let check_addition: CheckAdditionFn = |_| {
        Ok(Some(CheckViolationChoice::Nullify {
            column: "age".into(),
        }))
    };
    let prompts = RevisionPromptFns {
        check_strengthening,
        check_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("strengthen".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1, "Cancel must NOT write v2");
}

// ── F6/F19 type_narrowing apply path (line 358) ─────────────────────────

/// User supplies strategies → covers mod.rs:358
/// (`prompts::apply_narrowing_strategies_to_plan(...)`).
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_type_narrowing_applies_strategies_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "users",
        vec![int_col("id", false), text_col("nickname", true)],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "users",
        vec![
            int_col("id", false),
            col(
                "nickname",
                ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar { length: 5 }),
                true,
            ),
        ],
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let type_narrowing: TypeNarrowingFn =
        |_| Ok(Some(vec![vespertide_core::NarrowingStrategy::Truncate]));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        type_narrowing,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("narrow nickname".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

// ── F20 timezone Cancel path (lines 366-376) ────────────────────────────

/// Timestamp → timestamptz conversion with no explicit timezone, user
/// cancels the prompt → covers the `let Some(choices) = ... else { ... }`
/// Cancel branch and its multi-line `println!` at mod.rs:366-374, plus
/// the `return Ok(())` at 374. Line 376 (`apply_timezone_choices_to_plan`)
/// is covered by the companion accept-path test below.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_timezone_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "logs",
        vec![
            int_col("id", false),
            col("ts", ColumnType::Simple(SimpleColumnType::Timestamp), true),
        ],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "logs",
        vec![
            int_col("id", false),
            col(
                "ts",
                ColumnType::Simple(SimpleColumnType::Timestamptz),
                true,
            ),
        ],
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let timezone_conversion: TimezoneConversionFn = |_| Ok(None);
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        timezone_conversion,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("tz convert".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1, "Cancel must NOT write v2");
}

/// Companion accept-path: user supplies timezone choices → covers
/// mod.rs:376 (`prompts::apply_timezone_choices_to_plan(...)`).
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_timezone_apply_choices_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "logs",
        vec![
            int_col("id", false),
            col("ts", ColumnType::Simple(SimpleColumnType::Timestamp), true),
        ],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "logs",
        vec![
            int_col("id", false),
            col(
                "ts",
                ColumnType::Simple(SimpleColumnType::Timestamptz),
                true,
            ),
        ],
        vec![pk_constraint("id")],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let timezone_conversion: TimezoneConversionFn = |_| Ok(Some(vec!["UTC".to_string()]));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        timezone_conversion,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("tz convert".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

/// CHECK constraint with literal type mismatch (e.g. `int_col = 'abc'`).
/// User cancels the type-mismatch prompt → covers the
/// `check_type_mismatch_prompt_fn(warning)? else { ... return Ok(()) }`
/// Cancel branch.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_check_type_mismatch_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = table_def(
        "products",
        vec![int_col("id", false), int_col("quantity", true)],
        vec![pk_constraint("id")],
    );
    let target = table_def(
        "products",
        baseline.columns.clone(),
        vec![
            pk_constraint("id"),
            TableConstraint::Check {
                name: "chk_qty_type".into(),
                expr: "quantity = 'invalid'".into(),
                strategy: CheckViolationStrategy::default(),
            },
        ],
    );
    write_project_with_tables(&cfg, vec![baseline], vec![target]);
    let check_type_mismatch: CheckTypeMismatchFn = |_| Ok(None);
    // check_addition fires first (F4 cleanup prompt); supply a safe accept
    // so the flow reaches the type-mismatch prompt that we are testing.
    let check_addition: CheckAdditionFn = |_| {
        Ok(Some(CheckViolationChoice::Nullify {
            column: "quantity".into(),
        }))
    };
    let prompts = RevisionPromptFns {
        check_type_mismatch,
        check_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("type mismatch".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1, "Cancel must NOT write v2");
}

/// Model identical to applied migration → plan.actions empty
/// → covers the `if plan.actions.is_empty()` early return path.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_no_changes_detected_returns_early() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let baseline = base_users();
    // Write identical baseline and model → no changes.
    write_project_with_tables(&cfg, vec![baseline.clone()], vec![baseline]);
    let prompts = panic_guard_prompt_fns();
    cmd_revision_core("no changes".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1, "No v2 written when no changes");
}

/// Missing vespertide.json → load_config() returns Err
/// → covers the error propagation at the top of cmd_revision_core.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_load_config_error_propagates() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    // Do NOT write vespertide.json.
    let prompts = panic_guard_prompt_fns();
    let res = cmd_revision_core("test".into(), vec![], vec![], prompts).await;
    assert!(res.is_err(), "Expected error when vespertide.json missing");
}
