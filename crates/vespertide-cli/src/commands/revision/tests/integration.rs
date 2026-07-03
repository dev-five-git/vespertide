use super::*;

/// Integration test: FK column nullable→not-null triggers `handle_delete_null_rows` (line 489)
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_handles_delete_null_rows_for_fk_column() {
    use vespertide_core::MigrationPlan;
    use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};

    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // Write v1 migration: create "orders" table with nullable user_id
    let v1 = MigrationPlan {
        id: "v1-id".to_string(),
        comment: Some("init".to_string()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "orders".into(),
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
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true, // nullable in v1
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_orders__user_id".into()),
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            ],
        }],
    };
    let v1_path = cfg.migrations_dir().join("0001_init.vespertide.json");
    std_fs::write(&v1_path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    // Write updated model: user_id is now NOT NULL
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let users_model = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&users_model).unwrap(),
    )
    .unwrap();

    let model = TableDef {
        name: "orders".into(),
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
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false, // NOT NULL now
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("orders.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    // Mock prompts
    let recreate_prompt = |_: &[RecreateTableRequired]| -> Result<bool> { Ok(true) };
    let delete_prompt = |_table: &str, _col: &str| -> Result<bool> { Ok(true) };
    let fill_prompt = |_p: &str, _d: &str| -> Result<String> {
        panic!("fill prompt should not be called — FK handled by delete_null_rows");
    };
    let enum_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum prompt should not be called");
    };
    let enum_bare_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum bare prompt should not be called");
    };

    let result = cmd_revision_core(
        "make user_id required".into(),
        vec![],
        vec![],
        RevisionPromptFns {
            recreate: recreate_prompt,
            delete_null_rows: delete_prompt,
            fill_with: fill_prompt,
            enum_quoted: enum_prompt,
            enum_bare: enum_bare_prompt,
            // F30 / FK policy change is irrelevant to these scenarios:
            // assert via panic so any unexpected detection breaks the test.
            fk_policy_change: |_: &[vespertide_planner::FkPolicyChangeWarning]| -> Result<bool> { panic!("fk_policy_change prompt should not be called") },
            // F6 / type narrowing is irrelevant to these scenarios: assert
            // via panic so any unexpected detection breaks the test.
            type_narrowing: |_: &[vespertide_planner::TypeNarrowingWarning]| -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>> { panic!("type_narrowing prompt should not be called") },
            // F20 / timezone conversion likewise must not fire here.
            timezone_conversion: |_: &[vespertide_planner::TimezoneConversionWarning]| -> Result<Option<Vec<String>>> { panic!("timezone_conversion prompt should not be called") },
            // F7-(b) / RemapEnumValues likewise: integer enum value drift
            // is not in scope for these scenarios. Auto-approve so the
            // existing flow proceeds unchanged when no remap action exists.
            remap_enum_values: |_: &vespertide_core::MigrationPlan| -> Result<bool> { Ok(true) },
            // F10/F8/F22 drop resolution: these scenarios add columns only,
            // so no DeleteColumn / DeleteTable actions exist and the prompt
            // should never fire. Panic guards against silent flow drift.
            drop_resolution: |_: &vespertide_planner::DropResolution| -> Result<Option<vespertide_planner::DropChoice>> { panic!("drop_resolution prompt should not be called") },
            // F15 default-change resolution: these scenarios touch new
            // columns only, never `ModifyColumnDefault`, so the prompt
            // should never fire. Panic guards against silent flow drift.
            default_change: |_: &vespertide_planner::DefaultChangeWarning| -> Result<Option<crate::commands::revision::prompts::DefaultChoice>> { panic!("default_change prompt should not be called") },
            // F2 unique-addition resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(Unique)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            unique_addition: |_: &vespertide_planner::UniqueAdditionWarning| -> Result<Option<crate::commands::revision::prompts::UniqueAdditionChoice>> { panic!("unique_addition prompt should not be called") },
            // F3 fk-orphan resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(ForeignKey)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            fk_orphan_addition: |_: &vespertide_planner::FkOrphanAdditionWarning| -> Result<Option<crate::commands::revision::prompts::FkOrphanChoice>> { panic!("fk_orphan_addition prompt should not be called") },
            // F4 check-addition resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(Check)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            check_addition: |_: &vespertide_planner::CheckAdditionWarning| -> Result<Option<crate::commands::revision::prompts::CheckViolationChoice>> { panic!("check_addition prompt should not be called") },
            // F5 pk-addition resolution: same scope guarantee.
            pk_addition: |_: &vespertide_planner::PrimaryKeyAdditionWarning| -> Result<Option<crate::commands::revision::prompts::PrimaryKeyAdditionChoice>> { panic!("pk_addition prompt should not be called") },
            // F96 cascade-reach analysis: these scenarios do not add
            // new CASCADE foreign keys, so the prompt should never
            // fire. Panic guards against silent flow drift.
            cascade_reach: |_: &vespertide_planner::CascadeReachWarning| -> Result<Option<crate::commands::revision::prompts::CascadeReachChoice>> { panic!("cascade_reach prompt should not be called") },
            // F76 sequence-exhaustion: same scope guarantee.
            sequence_exhaustion: |_: &vespertide_planner::SequenceExhaustionWarning| -> Result<Option<crate::commands::revision::prompts::SequenceExhaustionChoice>> { panic!("sequence_exhaustion prompt should not be called") },
            // F29 check-strengthening: same scope guarantee.
            check_strengthening: |_: &vespertide_planner::CheckStrengtheningWarning| -> Result<Option<crate::commands::revision::prompts::CheckStrengtheningChoice>> { panic!("check_strengthening prompt should not be called") },
            // F-novel-4 check-type-mismatch: same scope guarantee.
            check_type_mismatch: |_: &vespertide_planner::CheckTypeMismatchWarning| -> Result<Option<crate::commands::revision::prompts::CheckTypeMismatchChoice>> { panic!("check_type_mismatch prompt should not be called") },
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "cmd_revision_core failed: {:?}",
        result.err()
    );

    // Verify migration was created
    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .collect();
    // Should have 2 files: v1 + new v2
    assert_eq!(entries.len(), 2);
}

/// Integration test: non-FK column nullable→not-null triggers `collect_fill_with_values` (lines 494-495)
#[tokio::test]
#[serial_test::serial]
async fn cmd_revision_core_handles_fill_with_for_non_fk_column() {
    use vespertide_core::MigrationPlan;

    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    // Write v1 migration: create "users" table with nullable email
    let v1 = MigrationPlan {
        id: "v1-id".to_string(),
        comment: Some("init".to_string()),
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
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
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true, // nullable in v1
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        }],
    };
    let v1_path = cfg.migrations_dir().join("0001_init.vespertide.json");
    std_fs::write(&v1_path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    // Write updated model: email is now NOT NULL (no default)
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "users".into(),
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
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false, // NOT NULL now
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();

    // Mock prompts
    let recreate_prompt = |_: &[RecreateTableRequired]| -> Result<bool> { Ok(true) };
    let delete_prompt = |_table: &str, _col: &str| -> Result<bool> { Ok(false) };
    let fill_prompt = |_p: &str, _d: &str| -> Result<String> { Ok("'unknown'".to_string()) };
    let enum_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum prompt should not be called");
    };
    let enum_bare_prompt = |_p: &str, _v: &[String]| -> Result<String> {
        panic!("enum bare prompt should not be called");
    };

    let result = cmd_revision_core(
        "make email required".into(),
        vec![],
        vec![],
        RevisionPromptFns {
            recreate: recreate_prompt,
            delete_null_rows: delete_prompt,
            fill_with: fill_prompt,
            enum_quoted: enum_prompt,
            enum_bare: enum_bare_prompt,
            // F30 / FK policy change is irrelevant to these scenarios:
            // assert via panic so any unexpected detection breaks the test.
            fk_policy_change: |_: &[vespertide_planner::FkPolicyChangeWarning]| -> Result<bool> { panic!("fk_policy_change prompt should not be called") },
            // F6 / type narrowing is irrelevant to these scenarios: assert
            // via panic so any unexpected detection breaks the test.
            type_narrowing: |_: &[vespertide_planner::TypeNarrowingWarning]| -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>> { panic!("type_narrowing prompt should not be called") },
            // F20 / timezone conversion likewise must not fire here.
            timezone_conversion: |_: &[vespertide_planner::TimezoneConversionWarning]| -> Result<Option<Vec<String>>> { panic!("timezone_conversion prompt should not be called") },
            // F7-(b) / RemapEnumValues likewise: integer enum value drift
            // is not in scope for these scenarios. Auto-approve so the
            // existing flow proceeds unchanged when no remap action exists.
            remap_enum_values: |_: &vespertide_core::MigrationPlan| -> Result<bool> { Ok(true) },
            // F10/F8/F22 drop resolution: these scenarios add columns only,
            // so no DeleteColumn / DeleteTable actions exist and the prompt
            // should never fire. Panic guards against silent flow drift.
            drop_resolution: |_: &vespertide_planner::DropResolution| -> Result<Option<vespertide_planner::DropChoice>> { panic!("drop_resolution prompt should not be called") },
            // F15 default-change resolution: these scenarios touch new
            // columns only, never `ModifyColumnDefault`, so the prompt
            // should never fire. Panic guards against silent flow drift.
            default_change: |_: &vespertide_planner::DefaultChangeWarning| -> Result<Option<crate::commands::revision::prompts::DefaultChoice>> { panic!("default_change prompt should not be called") },
            // F2 unique-addition resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(Unique)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            unique_addition: |_: &vespertide_planner::UniqueAdditionWarning| -> Result<Option<crate::commands::revision::prompts::UniqueAdditionChoice>> { panic!("unique_addition prompt should not be called") },
            // F3 fk-orphan resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(ForeignKey)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            fk_orphan_addition: |_: &vespertide_planner::FkOrphanAdditionWarning| -> Result<Option<crate::commands::revision::prompts::FkOrphanChoice>> { panic!("fk_orphan_addition prompt should not be called") },
            // F4 check-addition resolution: these scenarios add columns or
            // create tables only, never `AddConstraint(Check)` on an
            // existing column, so the prompt should never fire. Panic
            // guards against silent flow drift.
            check_addition: |_: &vespertide_planner::CheckAdditionWarning| -> Result<Option<crate::commands::revision::prompts::CheckViolationChoice>> { panic!("check_addition prompt should not be called") },
            // F5 pk-addition resolution: same scope guarantee.
            pk_addition: |_: &vespertide_planner::PrimaryKeyAdditionWarning| -> Result<Option<crate::commands::revision::prompts::PrimaryKeyAdditionChoice>> { panic!("pk_addition prompt should not be called") },
            // F96 cascade-reach analysis: these scenarios do not add
            // new CASCADE foreign keys, so the prompt should never
            // fire. Panic guards against silent flow drift.
            cascade_reach: |_: &vespertide_planner::CascadeReachWarning| -> Result<Option<crate::commands::revision::prompts::CascadeReachChoice>> { panic!("cascade_reach prompt should not be called") },
            // F76 sequence-exhaustion: same scope guarantee.
            sequence_exhaustion: |_: &vespertide_planner::SequenceExhaustionWarning| -> Result<Option<crate::commands::revision::prompts::SequenceExhaustionChoice>> { panic!("sequence_exhaustion prompt should not be called") },
            // F29 check-strengthening: same scope guarantee.
            check_strengthening: |_: &vespertide_planner::CheckStrengtheningWarning| -> Result<Option<crate::commands::revision::prompts::CheckStrengtheningChoice>> { panic!("check_strengthening prompt should not be called") },
            // F-novel-4 check-type-mismatch: same scope guarantee.
            check_type_mismatch: |_: &vespertide_planner::CheckTypeMismatchWarning| -> Result<Option<crate::commands::revision::prompts::CheckTypeMismatchChoice>> { panic!("check_type_mismatch prompt should not be called") },
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "cmd_revision_core failed: {:?}",
        result.err()
    );

    // Verify migration was written with fill_with
    let entries: Vec<_> = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .collect();
    assert_eq!(entries.len(), 2);

    // Read the v2 migration and verify fill_with was applied
    let v2_path = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy().contains("0002"))
        .expect("v2 migration not found");
    let v2_content = std_fs::read_to_string(v2_path.path()).unwrap();
    assert!(
        v2_content.contains("fill_with"),
        "Expected fill_with in migration, got: {v2_content}"
    );
}

// -- F-novel-4 CHECK type-mismatch hook (TM-S1/S2/S3) ---------------------
//
// A CreateTable carrying a CHECK that compares an integer column to a
// string literal (`age = 'abc'`) must route through the
// `check_type_mismatch` prompt during `vespertide revision`. The three
// scenarios prove the wiring:
//   TM-S1: the hook FIRES with the right warning payload (table/column/
//          literal) when a type-mismatch exists.
//   TM-S2: choosing Proceed writes the migration.
//   TM-S3: choosing Cancel aborts WITHOUT writing the migration.

/// Build a model dir + config with a single `events` table whose CHECK
/// compares the integer `age` column to a string literal. Returns the
/// config so the caller can inspect the migrations dir.
fn write_check_type_mismatch_model() -> vespertide_config::VespertideConfig {
    let cfg = write_config();
    std_fs::create_dir_all(cfg.migrations_dir()).unwrap();

    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let model = TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(
                    vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true),
                ),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "age".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        // CHECK compares an integer column to a STRING literal — the
        // F-novel-4 mismatch.
        constraints: vec![TableConstraint::Check {
            name: "chk_age_bad".into(),
            expr: "age = 'abc'".to_string(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }],
    };
    std_fs::write(
        models_dir.join("events.json"),
        serde_json::to_string_pretty(&model).unwrap(),
    )
    .unwrap();
    cfg
}

#[tokio::test]
#[serial_test::serial]
async fn tm_s1_s2_check_type_mismatch_proceed_writes_migration() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_check_type_mismatch_model();

    // Capture the warning the hook receives to prove TM-S1 payload.
    let seen: std::rc::Rc<std::cell::RefCell<Vec<(String, String, String)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen_clone = std::rc::Rc::clone(&seen);

    let result = cmd_revision_core(
        "create events".into(),
        vec![],
        vec![],
        make_prompt_fns_for_type_mismatch(
            move |w: &vespertide_planner::CheckTypeMismatchWarning| {
                seen_clone.borrow_mut().push((
                    w.table.clone(),
                    w.column.clone(),
                    w.literal_text.clone(),
                ));
                // Proceed.
                Ok(Some(
                    crate::commands::revision::prompts::CheckTypeMismatchChoice::Proceed,
                ))
            },
        ),
    )
    .await;

    assert!(
        result.is_ok(),
        "cmd_revision_core failed: {:?}",
        result.err()
    );

    // TM-S1: hook fired exactly once with the right payload.
    let captured = seen.borrow();
    assert_eq!(captured.len(), 1, "type-mismatch hook should fire once");
    assert_eq!(captured[0].0, "events");
    assert_eq!(captured[0].1, "age");
    assert_eq!(captured[0].2, "'abc'");

    // TM-S2: migration was written.
    let count = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .count();
    assert_eq!(count, 1, "Proceed must write exactly one migration");
}

#[tokio::test]
#[serial_test::serial]
async fn tm_s3_check_type_mismatch_cancel_aborts_without_writing() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let cfg = write_check_type_mismatch_model();

    let result = cmd_revision_core(
        "create events".into(),
        vec![],
        vec![],
        make_prompt_fns_for_type_mismatch(|_w: &vespertide_planner::CheckTypeMismatchWarning| {
            // Cancel.
            Ok(None)
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "cmd_revision_core should return Ok on user cancel, got: {:?}",
        result.err()
    );

    // TM-S3: no migration written on Cancel.
    let count = std_fs::read_dir(cfg.migrations_dir())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .count();
    assert_eq!(count, 0, "Cancel must NOT write a migration");
}

/// Construct a `RevisionPromptFns` where every prompt unrelated to
/// F-novel-4 is a panic-guard (proving they never fire for a plain
/// `CreateTable` + CHECK type-mismatch), and `check_type_mismatch` is the
/// caller-supplied closure under test.
#[expect(
    clippy::type_complexity,
    reason = "mirrors RevisionPromptFns' 19 closure generics; this test builder fixes 18 panic-guards and threads only the CTM closure under test — extracting type aliases would duplicate the production signature"
)]
fn make_prompt_fns_for_type_mismatch<CTM>(
    check_type_mismatch: CTM,
) -> RevisionPromptFns<
    impl Fn(&[RecreateTableRequired]) -> Result<bool>,
    impl Fn(&str, &str) -> Result<bool>,
    impl Fn(&str, &str) -> Result<String>,
    impl Fn(&str, &[String]) -> Result<String>,
    impl Fn(&str, &[String]) -> Result<String>,
    impl Fn(&[vespertide_planner::FkPolicyChangeWarning]) -> Result<bool>,
    impl Fn(
        &[vespertide_planner::TypeNarrowingWarning],
    ) -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>>,
    impl Fn(&[vespertide_planner::TimezoneConversionWarning]) -> Result<Option<Vec<String>>>,
    impl Fn(&vespertide_core::MigrationPlan) -> Result<bool>,
    impl Fn(&vespertide_planner::DropResolution) -> Result<Option<vespertide_planner::DropChoice>>,
    impl Fn(
        &vespertide_planner::DefaultChangeWarning,
    ) -> Result<Option<crate::commands::revision::prompts::DefaultChoice>>,
    impl Fn(
        &vespertide_planner::UniqueAdditionWarning,
    ) -> Result<Option<crate::commands::revision::prompts::UniqueAdditionChoice>>,
    impl Fn(
        &vespertide_planner::FkOrphanAdditionWarning,
    ) -> Result<Option<crate::commands::revision::prompts::FkOrphanChoice>>,
    impl Fn(
        &vespertide_planner::CheckAdditionWarning,
    ) -> Result<Option<crate::commands::revision::prompts::CheckViolationChoice>>,
    impl Fn(
        &vespertide_planner::PrimaryKeyAdditionWarning,
    ) -> Result<Option<crate::commands::revision::prompts::PrimaryKeyAdditionChoice>>,
    impl Fn(
        &vespertide_planner::CascadeReachWarning,
    ) -> Result<Option<crate::commands::revision::prompts::CascadeReachChoice>>,
    impl Fn(
        &vespertide_planner::SequenceExhaustionWarning,
    ) -> Result<Option<crate::commands::revision::prompts::SequenceExhaustionChoice>>,
    impl Fn(
        &vespertide_planner::CheckStrengtheningWarning,
    ) -> Result<Option<crate::commands::revision::prompts::CheckStrengtheningChoice>>,
    CTM,
>
where
    CTM: Fn(
        &vespertide_planner::CheckTypeMismatchWarning,
    ) -> Result<Option<crate::commands::revision::prompts::CheckTypeMismatchChoice>>,
{
    RevisionPromptFns { recreate: |_: &[RecreateTableRequired]| -> Result<bool> { Ok(true) }, delete_null_rows: |_: &str, _: &str| -> Result<bool> { Ok(false) }, fill_with: |_: &str, _: &str| -> Result<String> { panic!("fill_with prompt should not be called") }, enum_quoted: |_: &str, _: &[String]| -> Result<String> { panic!("enum prompt should not be called") }, enum_bare: |_: &str, _: &[String]| -> Result<String> { panic!("enum bare prompt should not be called") }, fk_policy_change: |_: &[vespertide_planner::FkPolicyChangeWarning]| -> Result<bool> { panic!("fk_policy_change prompt should not be called") }, type_narrowing: |_: &[vespertide_planner::TypeNarrowingWarning]| -> Result<Option<Vec<vespertide_core::NarrowingStrategy>>> { panic!("type_narrowing prompt should not be called") }, timezone_conversion: |_: &[vespertide_planner::TimezoneConversionWarning]| -> Result<Option<Vec<String>>> { panic!("timezone_conversion prompt should not be called") }, remap_enum_values: |_: &vespertide_core::MigrationPlan| -> Result<bool> { Ok(true) }, drop_resolution: |_: &vespertide_planner::DropResolution| -> Result<Option<vespertide_planner::DropChoice>> { panic!("drop_resolution prompt should not be called") }, default_change: |_: &vespertide_planner::DefaultChangeWarning| -> Result<Option<crate::commands::revision::prompts::DefaultChoice>> { panic!("default_change prompt should not be called") }, unique_addition: |_: &vespertide_planner::UniqueAdditionWarning| -> Result<Option<crate::commands::revision::prompts::UniqueAdditionChoice>> { panic!("unique_addition prompt should not be called") }, fk_orphan_addition: |_: &vespertide_planner::FkOrphanAdditionWarning| -> Result<Option<crate::commands::revision::prompts::FkOrphanChoice>> { panic!("fk_orphan_addition prompt should not be called") }, check_addition: |_: &vespertide_planner::CheckAdditionWarning| -> Result<Option<crate::commands::revision::prompts::CheckViolationChoice>> { panic!("check_addition prompt should not be called") }, pk_addition: |_: &vespertide_planner::PrimaryKeyAdditionWarning| -> Result<Option<crate::commands::revision::prompts::PrimaryKeyAdditionChoice>> { panic!("pk_addition prompt should not be called") }, cascade_reach: |_: &vespertide_planner::CascadeReachWarning| -> Result<Option<crate::commands::revision::prompts::CascadeReachChoice>> { panic!("cascade_reach prompt should not be called") }, sequence_exhaustion: |_: &vespertide_planner::SequenceExhaustionWarning| -> Result<Option<crate::commands::revision::prompts::SequenceExhaustionChoice>> { panic!("sequence_exhaustion prompt should not be called") }, check_strengthening: |_: &vespertide_planner::CheckStrengtheningWarning| -> Result<Option<crate::commands::revision::prompts::CheckStrengtheningChoice>> { panic!("check_strengthening prompt should not be called") }, check_type_mismatch }
}
