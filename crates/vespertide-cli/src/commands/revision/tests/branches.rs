//! Coverage closure for `revision/mod.rs` uncovered branches.
//!
//! Covers:
//! - `dangling_drops_to_planner_error` 0/1/N+ contract (top of mod.rs).
//! - `cmd_revision_core` Cancel paths (every interactive prompt that
//!   returns `Ok(None)` aborts the migration without writing).
//! - `cmd_revision_core` happy / hard-error paths that the existing
//!   `integration.rs` tests don't exercise.
//!
//! Pattern: write models + previous migration via the `CwdGuard` +
//! `tempdir()` harness in [`super`]; build a [`RevisionPromptFns`] where
//! every prompt unrelated to the scenario panics; drive `cmd_revision_core`;
//! assert return value + on-disk side effect.

use super::*;
use vespertide_core::{
    ForeignKeyOrphanStrategy, MigrationPlan, PrimaryKeyAdditionStrategy,
    schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax},
};
use vespertide_planner::DanglingFkDrop;

// ── dangling_drops_to_planner_error ──────────────────────────────────────

#[test]
fn dangling_drops_to_planner_error_empty_returns_none() {
    assert!(dangling_drops_to_planner_error(vec![]).is_none());
}

#[test]
fn dangling_drops_to_planner_error_single_returns_bare_variant() {
    let err = dangling_drops_to_planner_error(vec![DanglingFkDrop {
        dropped_table: "users".into(),
        dropped_column: Some("id".into()),
        referencing_table: "posts".into(),
        referencing_constraint: Some("fk_post_user".into()),
    }])
    .unwrap();
    assert!(matches!(
        err,
        vespertide_planner::PlannerError::DanglingForeignKeyAfterDrop { .. }
    ));
}

#[test]
fn dangling_drops_to_planner_error_multiple_returns_multiple_variant() {
    let drops = vec![
        DanglingFkDrop {
            dropped_table: "users".into(),
            dropped_column: Some("id".into()),
            referencing_table: "posts".into(),
            referencing_constraint: None,
        },
        DanglingFkDrop {
            dropped_table: "audit".into(),
            dropped_column: None,
            referencing_table: "log".into(),
            referencing_constraint: Some("fk_log".into()),
        },
    ];
    let err = dangling_drops_to_planner_error(drops).unwrap();
    assert!(matches!(err, vespertide_planner::PlannerError::Multiple(_)));
}

#[test]
fn single_or_multiple_error_single_returns_bare_variant() {
    let err = single_or_multiple_error(vec![PlannerError::TableNotFound("users".into())]);
    assert!(matches!(err, PlannerError::TableNotFound(table) if table == "users"));
}

#[test]
fn single_or_multiple_error_multiple_returns_multiple_variant() {
    let err = single_or_multiple_error(vec![
        PlannerError::TableNotFound("users".into()),
        PlannerError::TableNotFound("posts".into()),
    ]);
    assert!(matches!(err, PlannerError::Multiple(_)));
}

#[test]
fn ensure_no_dangling_fk_drops_returns_error_for_surviving_fk_to_dropped_table() {
    let parent = table_def(
        "parent",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let child = table_def(
        "child",
        vec![int_col("id", false), int_col("parent_id", true)],
        vec![
            pk_constraint("id"),
            TableConstraint::ForeignKey {
                name: Some("fk_child_parent".into()),
                columns: vec!["parent_id".into()],
                ref_table: "parent".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        ],
    );
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::DeleteTable {
            table: "parent".into(),
        }],
    };
    let err = ensure_no_dangling_fk_drops(&plan, &[parent, child])
        .unwrap_err()
        .to_string();
    assert!(
        err.to_lowercase().contains("foreign key") || err.to_lowercase().contains("dangling"),
        "expected dangling FK error; got: {err}"
    );
}

#[test]
fn ensure_no_f12_errors_returns_error_for_pk_removal() {
    let baseline = table_def(
        "widgets",
        vec![int_col("id", false)],
        vec![pk_constraint("id")],
    );
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::RemoveConstraint {
            table: "widgets".into(),
            constraint: pk_constraint("id"),
        }],
    };
    let err = ensure_no_f12_errors(&plan, &[baseline])
        .unwrap_err()
        .to_string();
    assert!(
        err.to_lowercase().contains("primary key"),
        "expected F12 primary-key error; got: {err}"
    );
}

// ── Helpers for cmd_revision_core integration scenarios ─────────────────

// All prompts are typed as `fn(...) -> ...` (function pointers, not generic
// closures) so [`panic_guard_prompt_fns`] returns a *concrete* type that
// downstream tests can extend via `..panic_guard_prompt_fns()` while
// swapping individual fields with non-capturing closures (which coerce to
// the same fn pointer type).
pub(super) type RecreateFn = fn(&[RecreateTableRequired]) -> Result<bool>;
pub(super) type DeleteNullRowsFn = fn(&str, &str) -> Result<bool>;
pub(super) type FillWithFn = fn(&str, &str) -> Result<String>;
pub(super) type EnumQuotedFn = fn(&str, &[String]) -> Result<String>;
pub(super) type EnumBareFn = fn(&str, &[String]) -> Result<String>;
pub(super) type FkPolicyChangeFn = fn(&[vespertide_planner::FkPolicyChangeWarning]) -> Result<bool>;
pub(super) type TypeNarrowingFn = fn(
    &[vespertide_planner::TypeNarrowingWarning],
) -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>>;
pub(super) type TimezoneConversionFn =
    fn(&[vespertide_planner::TimezoneConversionWarning]) -> Result<Option<Vec<String>>>;
pub(super) type RemapEnumValuesFn = fn(&MigrationPlan) -> Result<bool>;
pub(super) type DropResolutionFn =
    fn(&vespertide_planner::DropResolution) -> Result<Option<vespertide_planner::DropChoice>>;
pub(super) type DefaultChangeFn =
    fn(&vespertide_planner::DefaultChangeWarning) -> Result<Option<DefaultChoice>>;
pub(super) type UniqueAdditionFn =
    fn(&vespertide_planner::UniqueAdditionWarning) -> Result<Option<UniqueAdditionChoice>>;
pub(super) type FkOrphanAdditionFn =
    fn(&vespertide_planner::FkOrphanAdditionWarning) -> Result<Option<FkOrphanChoice>>;
pub(super) type CheckAdditionFn =
    fn(&vespertide_planner::CheckAdditionWarning) -> Result<Option<CheckViolationChoice>>;
pub(super) type PkAdditionFn =
    fn(&vespertide_planner::PrimaryKeyAdditionWarning) -> Result<Option<PrimaryKeyAdditionChoice>>;
pub(super) type CascadeReachFn =
    fn(&vespertide_planner::CascadeReachWarning) -> Result<Option<CascadeReachChoice>>;
pub(super) type SequenceExhaustionFn =
    fn(&vespertide_planner::SequenceExhaustionWarning) -> Result<Option<SequenceExhaustionChoice>>;
pub(super) type CheckStrengtheningFn =
    fn(&vespertide_planner::CheckStrengtheningWarning) -> Result<Option<CheckStrengtheningChoice>>;
pub(super) type CheckTypeMismatchFn =
    fn(&vespertide_planner::CheckTypeMismatchWarning) -> Result<Option<CheckTypeMismatchChoice>>;

#[expect(
    clippy::type_complexity,
    reason = "19 fn-pointer generics mirror the production RevisionPromptFns; explicit type aliases would scatter the signature"
)]
pub(super) fn panic_guard_prompt_fns() -> RevisionPromptFns<
    RecreateFn,
    DeleteNullRowsFn,
    FillWithFn,
    EnumQuotedFn,
    EnumBareFn,
    FkPolicyChangeFn,
    TypeNarrowingFn,
    TimezoneConversionFn,
    RemapEnumValuesFn,
    DropResolutionFn,
    DefaultChangeFn,
    UniqueAdditionFn,
    FkOrphanAdditionFn,
    CheckAdditionFn,
    PkAdditionFn,
    CascadeReachFn,
    SequenceExhaustionFn,
    CheckStrengtheningFn,
    CheckTypeMismatchFn,
> {
    RevisionPromptFns {
        recreate: (|_| panic!("recreate prompt should not be called")) as RecreateFn,
        delete_null_rows: (|_, _| panic!("delete_null_rows prompt should not be called"))
            as DeleteNullRowsFn,
        fill_with: (|_, _| panic!("fill_with prompt should not be called")) as FillWithFn,
        enum_quoted: (|_, _| panic!("enum_quoted prompt should not be called")) as EnumQuotedFn,
        enum_bare: (|_, _| panic!("enum_bare prompt should not be called")) as EnumBareFn,
        fk_policy_change: (|_| panic!("fk_policy_change prompt should not be called"))
            as FkPolicyChangeFn,
        type_narrowing: (|_| panic!("type_narrowing prompt should not be called"))
            as TypeNarrowingFn,
        timezone_conversion: (|_| panic!("timezone_conversion prompt should not be called"))
            as TimezoneConversionFn,
        remap_enum_values: (|_| panic!("remap_enum_values prompt should not be called"))
            as RemapEnumValuesFn,
        drop_resolution: (|_| panic!("drop_resolution prompt should not be called"))
            as DropResolutionFn,
        default_change: (|_| panic!("default_change prompt should not be called"))
            as DefaultChangeFn,
        unique_addition: (|_| panic!("unique_addition prompt should not be called"))
            as UniqueAdditionFn,
        fk_orphan_addition: (|_| panic!("fk_orphan_addition prompt should not be called"))
            as FkOrphanAdditionFn,
        check_addition: (|_| panic!("check_addition prompt should not be called"))
            as CheckAdditionFn,
        pk_addition: (|_| panic!("pk_addition prompt should not be called")) as PkAdditionFn,
        cascade_reach: (|_| panic!("cascade_reach prompt should not be called")) as CascadeReachFn,
        sequence_exhaustion: (|_| panic!("sequence_exhaustion prompt should not be called"))
            as SequenceExhaustionFn,
        check_strengthening: (|_| panic!("check_strengthening prompt should not be called"))
            as CheckStrengtheningFn,
        check_type_mismatch: (|_| panic!("check_type_mismatch prompt should not be called"))
            as CheckTypeMismatchFn,
    }
}

// ── Scenario: dangling FK after column drop → hard error ────────────────

/// v1 creates `users(id)` + `posts(user_id FK→users.id)`. New model drops
/// `posts.user_id` without removing the FK, so the F9 "dangling FK after
/// drop" check fires before any prompt runs.
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_dangling_fk_drop_returns_hard_error() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // v1: users + posts with FK posts.user_id → users.id
    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef::new(
                    "id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                )],
                constraints: vec![TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: PrimaryKeyAdditionStrategy::default(),
                }],
            },
            MigrationAction::CreateTable {
                table: "posts".into(),
                columns: vec![
                    ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                    ColumnDef::new(
                        "user_id",
                        ColumnType::Simple(SimpleColumnType::Integer),
                        true,
                    ),
                ],
                constraints: vec![
                    TableConstraint::PrimaryKey {
                        auto_increment: false,
                        columns: vec!["id".into()],
                        strategy: PrimaryKeyAdditionStrategy::default(),
                    },
                    TableConstraint::ForeignKey {
                        name: Some("fk_post_user".into()),
                        columns: vec!["user_id".into()],
                        ref_table: "users".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                        orphan_strategy: ForeignKeyOrphanStrategy::default(),
                    },
                ],
            },
        ],
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();

    // Models: drop users entirely so its column dangles for posts.user_id FK.
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let posts_model = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new(
                "user_id",
                ColumnType::Simple(SimpleColumnType::Integer),
                true,
            )
            .foreign_key(ForeignKeySyntax::Object(ForeignKeyDef {
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            })),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("posts.json"),
        serde_json::to_string_pretty(&posts_model).unwrap(),
    )
    .unwrap();
    // NOTE: no `users.json` — users table is being dropped.

    // Auto-accept the drop_resolution prompt (the planner will pick Drop).
    let drop_resolution: DropResolutionFn = |_| Ok(Some(vespertide_planner::DropChoice::Drop));
    let prompts = RevisionPromptFns {
        drop_resolution,
        ..panic_guard_prompt_fns()
    };

    let res = cmd_revision_core("drop users".into(), vec![], vec![], prompts).await;
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("dangling") || err.contains("foreign key") || err.contains("FK"),
        "expected dangling-FK message; got: {err}"
    );
}

// ── Scenario: drop_resolution cancel aborts without writing migration ────

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_drop_resolution_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // v1: users(id, email)
    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                ColumnDef::new("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        }],
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();

    // Model: drop `email` (only `id` remains).
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef::new(
            "id",
            ColumnType::Simple(SimpleColumnType::Integer),
            false,
        )],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    let drop_resolution: DropResolutionFn = |_| Ok(None);
    let prompts = RevisionPromptFns {
        drop_resolution,
        ..panic_guard_prompt_fns()
    };

    let res = cmd_revision_core("drop email".into(), vec![], vec![], prompts).await;
    assert!(res.is_ok());

    // Only v1 still on disk; no v2 written.
    let count = std_fs::read_dir(cfg.migrations_dir()).unwrap().count();
    assert_eq!(count, 1);
}

// ── Scenario: drop_resolution Drop happy-path writes v2 ─────────────────

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_drop_resolution_accept_drop_writes_migration() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                ColumnDef::new("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        }],
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();

    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef::new(
            "id",
            ColumnType::Simple(SimpleColumnType::Integer),
            false,
        )],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    let drop_resolution: DropResolutionFn = |_| Ok(Some(vespertide_planner::DropChoice::Drop));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        remap_enum_values,
        drop_resolution,
        ..panic_guard_prompt_fns()
    };

    cmd_revision_core("drop email".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    let count = std_fs::read_dir(cfg.migrations_dir()).unwrap().count();
    assert_eq!(count, 2);
}

// ── Scenario: default_change Backfill rewrites action.backfill ──────────

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_default_change_backfill_path() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // v1: users(id, status text default 'pending')
    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                ColumnDef::new("status", ColumnType::Simple(SimpleColumnType::Text), false)
                    .default("'pending'".into()),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        }],
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();

    // Model: change default 'pending' → 'active'.
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new("status", ColumnType::Simple(SimpleColumnType::Text), false)
                .default("'active'".into()),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    let default_change: DefaultChangeFn = |_| Ok(Some(DefaultChoice::Backfill));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        remap_enum_values,
        default_change,
        ..panic_guard_prompt_fns()
    };

    cmd_revision_core("default change".into(), vec![], vec![], prompts)
        .await
        .unwrap();

    // Read v2 and assert ModifyColumnDefault.backfill is set.
    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .collect();
    let v2 = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy().contains("0002"))
        .expect("v2 not found");
    let content = std_fs::read_to_string(v2.path()).unwrap();
    assert!(
        content.contains("backfill"),
        "Expected backfill in v2 migration; got: {content}"
    );
}

// ── Scenario: default_change Cancel aborts without writing ──────────────

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_default_change_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                ColumnDef::new("status", ColumnType::Simple(SimpleColumnType::Text), false)
                    .default("'pending'".into()),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        }],
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();

    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new("status", ColumnType::Simple(SimpleColumnType::Text), false)
                .default("'active'".into()),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    let default_change: DefaultChangeFn = |_| Ok(None);
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        remap_enum_values,
        default_change,
        ..panic_guard_prompt_fns()
    };

    cmd_revision_core("default change".into(), vec![], vec![], prompts)
        .await
        .unwrap();

    let count = std_fs::read_dir(cfg.migrations_dir()).unwrap().count();
    assert_eq!(count, 1, "Cancel must not write v2");
}

// ── Scenario: default_change Skip leaves backfill None and writes ───────

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_default_change_skip_writes_v2_without_backfill() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                ColumnDef::new("status", ColumnType::Simple(SimpleColumnType::Text), false)
                    .default("'pending'".into()),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        }],
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();

    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new("status", ColumnType::Simple(SimpleColumnType::Text), false)
                .default("'active'".into()),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    let default_change: DefaultChangeFn = |_| Ok(Some(DefaultChoice::Skip));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        remap_enum_values,
        default_change,
        ..panic_guard_prompt_fns()
    };

    cmd_revision_core("default change".into(), vec![], vec![], prompts)
        .await
        .unwrap();

    let count = std_fs::read_dir(cfg.migrations_dir()).unwrap().count();
    assert_eq!(count, 2);
}

pub(super) fn col(name: &str, ty: ColumnType, nullable: bool) -> ColumnDef {
    ColumnDef::new(name, ty, nullable)
}

pub(super) fn int_col(name: &str, nullable: bool) -> ColumnDef {
    col(
        name,
        ColumnType::Simple(SimpleColumnType::Integer),
        nullable,
    )
}

pub(super) fn text_col(name: &str, nullable: bool) -> ColumnDef {
    col(name, ColumnType::Simple(SimpleColumnType::Text), nullable)
}

pub(super) fn pk_constraint(column: &str) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: vec![column.into()],
        strategy: PrimaryKeyAdditionStrategy::default(),
    }
}

pub(super) fn table_def(
    name: &str,
    columns: Vec<ColumnDef>,
    constraints: Vec<TableConstraint>,
) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

pub(super) fn write_project_with_tables(
    cfg: &VespertideConfig,
    baseline: Vec<TableDef>,
    models: Vec<TableDef>,
) {
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();
    let actions = baseline
        .into_iter()
        .map(|table| MigrationAction::CreateTable {
            table: table.name,
            columns: table.columns,
            constraints: table.constraints,
        })
        .collect();
    let v1 = MigrationPlan {
        id: "v1".into(),
        comment: Some("init".into()),
        created_at: None,
        version: 1,
        actions,
    };
    std_fs::write(
        cfg.migrations_dir().join("0001_init.vespertide.json"),
        serde_json::to_string_pretty(&v1).unwrap(),
    )
    .unwrap();
    std_fs::create_dir_all("models").unwrap();
    for model in models {
        std_fs::write(
            PathBuf::from("models").join(format!("{}.json", model.name)),
            serde_json::to_string_pretty(&model).unwrap(),
        )
        .unwrap();
    }
}

pub(super) fn migration_count(cfg: &VespertideConfig) -> usize {
    std_fs::read_dir(cfg.migrations_dir()).unwrap().count()
}

pub(super) fn base_users() -> TableDef {
    table_def(
        "users",
        vec![int_col("id", false), text_col("email", true)],
        vec![pk_constraint("id")],
    )
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_unique_addition_cancel_aborts() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let mut target = base_users();
    target.columns[1].unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));
    write_project_with_tables(&cfg, vec![base_users()], vec![target]);
    let unique_addition: UniqueAdditionFn = |_| Ok(None);
    let prompts = RevisionPromptFns {
        unique_addition,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("unique email".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_unique_addition_continue_writes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_config();
    let mut target = base_users();
    target.columns[1].unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));
    write_project_with_tables(&cfg, vec![base_users()], vec![target]);
    let unique_addition: UniqueAdditionFn =
        |_| Ok(Some(UniqueAdditionChoice::ContinueWithoutCleanup));
    let remap_enum_values: RemapEnumValuesFn = |_| Ok(true);
    let prompts = RevisionPromptFns {
        unique_addition,
        remap_enum_values,
        ..panic_guard_prompt_fns()
    };
    cmd_revision_core("unique email".into(), vec![], vec![], prompts)
        .await
        .unwrap();
    assert_eq!(migration_count(&cfg), 2);
}

pub(super) fn users_posts_without_fk(nullable: bool) -> (TableDef, TableDef) {
    (
        table_def(
            "users",
            vec![int_col("id", false)],
            vec![pk_constraint("id")],
        ),
        table_def(
            "posts",
            vec![int_col("id", false), int_col("user_id", nullable)],
            vec![pk_constraint("id")],
        ),
    )
}
