use super::*;
use crate::test_support::CwdGuard;
use colored::Colorize;
use rstest::rstest;
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use vespertide_config::VespertideConfig;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationPlan, ReferenceAction, SimpleColumnType, TableConstraint,
    TableDef,
};
use vespertide_planner::{
    NarrowingKind, PolicyDelta, TimezoneConversionDirection, TimezoneConversionWarning,
};

fn write_config() {
    let cfg = VespertideConfig::default();
    let text = serde_json::to_string_pretty(&cfg).unwrap();
    fs::write("vespertide.json", text).unwrap();
}

fn write_model(name: &str) {
    let models_dir = PathBuf::from("models");
    fs::create_dir_all(&models_dir).unwrap();
    let table = TableDef {
        name: name.into(),
        description: None,
        columns: vec![ColumnDef::new(
            "id",
            ColumnType::Simple(SimpleColumnType::Integer),
            false,
        )],
        constraints: vec![pk_id()],
    };
    let path = models_dir.join(format!("{name}.json"));
    fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
}

fn idx(name: Option<&str>, cols: &[&str]) -> TableConstraint {
    TableConstraint::Index {
        name: name.map(Into::into),
        columns: cols.iter().map(|c| (*c).into()).collect(),
    }
}
fn pk_id() -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: vec!["id".into()],
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}
fn uq_email(name: Option<&str>) -> TableConstraint {
    TableConstraint::Unique {
        name: name.map(Into::into),
        columns: vec!["email".into()],
        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First,
        },
    }
}
fn fk_user(name: Option<&str>, on_delete: Option<ReferenceAction>) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: name.map(Into::into),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}
fn chk_age() -> TableConstraint {
    TableConstraint::Check {
        name: "check_age".into(),
        expr: "age > 0".into(),
        strategy: vespertide_core::CheckViolationStrategy::default(),
    }
}

#[rstest]
#[case(
    MigrationAction::CreateTable { table: "users".into(), columns: vec![], constraints: vec![] },
    format!("{} {}", "Create table:".bright_green(), "users".bright_cyan().bold())
)]
#[case(
    MigrationAction::DeleteTable { table: "users".into() },
    format!("{} {}", "Delete table:".bright_red(), "users".bright_cyan().bold())
)]
#[case(
    MigrationAction::AddColumn { table: "users".into(), column: Box::new(ColumnDef::new("name", ColumnType::Simple(SimpleColumnType::Text), true)), fill_with: None },
    format!("{} {}.{}", "Add column:".bright_green(), "users".bright_cyan(), "name".bright_cyan().bold())
)]
#[case(
    MigrationAction::RenameColumn { table: "users".into(), from: "old".into(), to: "new".into() },
    format!("{} {}.{} {} {}", "Rename column:".bright_yellow(), "users".bright_cyan(), "old".bright_white(), "->".bright_white(), "new".bright_cyan().bold())
)]
#[case(
    MigrationAction::DeleteColumn { table: "users".into(), column: "name".into() },
    format!("{} {}.{}", "Delete column:".bright_red(), "users".bright_cyan(), "name".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnType { table: "users".into(), column: "id".into(), new_type: ColumnType::Simple(SimpleColumnType::Integer), fill_with: None, narrowing_strategy: None, timezone: None },
    format!("{} {}.{} {} {}", "Modify column type:".bright_yellow(), "users".bright_cyan(), "id".bright_cyan().bold(), "->".bright_white(), "integer".bright_cyan().bold())
)]
#[case(
    MigrationAction::AddConstraint { table: "users".into(), constraint: idx(Some("idx"), &["id"]) },
    format!("{} {} {} {}", "Add constraint:".bright_green(), "idx INDEX (id)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::RemoveConstraint { table: "users".into(), constraint: idx(Some("idx"), &["id"]) },
    format!("{} {} {} {}", "Remove constraint:".bright_red(), "idx INDEX (id)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::RenameTable { from: "users".into(), to: "accounts".into() },
    format!("{} {} {} {}", "Rename table:".bright_yellow(), "users".bright_cyan(), "->".bright_white(), "accounts".bright_cyan().bold())
)]
#[case(
    MigrationAction::RawSql { sql: "SELECT 1".into() },
    format!("{} {}", "Execute raw SQL:".bright_yellow(), "SELECT 1".bright_cyan())
)]
#[case(
    MigrationAction::AddConstraint { table: "users".into(), constraint: pk_id() },
    format!("{} {} {} {}", "Add constraint:".bright_green(), "PRIMARY KEY (id)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::AddConstraint { table: "users".into(), constraint: uq_email(Some("unique_email")) },
    format!("{} {} {} {}", "Add constraint:".bright_green(), "unique_email UNIQUE (email)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::AddConstraint { table: "posts".into(), constraint: fk_user(Some("fk_user"), None) },
    format!("{} {} {} {}", "Add constraint:".bright_green(), "fk_user FK (user_id) -> users".bright_cyan().bold(), "on".bright_white(), "posts".bright_cyan())
)]
#[case(
    MigrationAction::AddConstraint { table: "users".into(), constraint: chk_age() },
    format!("{} {} {} {}", "Add constraint:".bright_green(), "check_age CHECK (age > 0)".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::RemoveConstraint { table: "users".into(), constraint: pk_id() },
    format!("{} {} {} {}", "Remove constraint:".bright_red(), "PRIMARY KEY (id)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::RemoveConstraint { table: "users".into(), constraint: uq_email(None) },
    format!("{} {} {} {}", "Remove constraint:".bright_red(), "UNIQUE (email)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::RemoveConstraint { table: "posts".into(), constraint: fk_user(None, None) },
    format!("{} {} {} {}", "Remove constraint:".bright_red(), "FK (user_id) -> users".bright_cyan().bold(), "from".bright_white(), "posts".bright_cyan())
)]
#[case(
    MigrationAction::RemoveConstraint { table: "users".into(), constraint: chk_age() },
    format!("{} {} {} {}", "Remove constraint:".bright_red(), "check_age CHECK (age > 0)".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
)]
#[case(
    MigrationAction::ModifyColumnNullable { table: "users".into(), column: "email".into(), nullable: false, fill_with: None, delete_null_rows: None },
    format!("{} {}.{} {} {}", "Modify column nullability:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "NOT NULL".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnNullable { table: "users".into(), column: "email".into(), nullable: true, fill_with: None, delete_null_rows: None },
    format!("{} {}.{} {} {}", "Modify column nullability:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "NULL".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnDefault { table: "users".into(), column: "status".into(), new_default: Some("'active'".into()), backfill: None },
    format!("{} {}.{} {} {}", "Modify column default:".bright_yellow(), "users".bright_cyan(), "status".bright_cyan().bold(), "->".bright_white(), "'active'".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnDefault { table: "users".into(), column: "status".into(), new_default: None, backfill: None },
    format!("{} {}.{} {} {}", "Modify column default:".bright_yellow(), "users".bright_cyan(), "status".bright_cyan().bold(), "->".bright_white(), "(none)".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnComment { table: "users".into(), column: "email".into(), new_comment: Some("User email address".into()) },
    format!("{} {}.{} {} '{}'", "Modify column comment:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "User email address".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnComment { table: "users".into(), column: "email".into(), new_comment: None },
    format!("{} {}.{} {} '{}'", "Modify column comment:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "(none)".bright_cyan().bold())
)]
#[case(
    MigrationAction::ModifyColumnComment { table: "users".into(), column: "email".into(), new_comment: Some("This is a very long comment that exceeds thirty characters and should be truncated".into()) },
    format!("{} {}.{} {} '{}'", "Modify column comment:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "This is a very long comment...".bright_cyan().bold())
)]
// Boundary: EXACTLY 30 chars → NOT truncated (kills `> 30` → `>= 30` mutant).
#[case(
    MigrationAction::ModifyColumnComment { table: "users".into(), column: "email".into(), new_comment: Some("012345678901234567890123456789".into()) },
    format!("{} {}.{} {} '{}'", "Modify column comment:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "012345678901234567890123456789".bright_cyan().bold())
)]
// Boundary: 31 chars → truncated to first 27 chars + "..." (kills `> 30` → `>= 30` mutant).
#[case(
    MigrationAction::ModifyColumnComment { table: "users".into(), column: "email".into(), new_comment: Some("0123456789012345678901234567890".into()) },
    format!("{} {}.{} {} '{}'", "Modify column comment:".bright_yellow(), "users".bright_cyan(), "email".bright_cyan().bold(), "->".bright_white(), "012345678901234567890123456...".bright_cyan().bold())
)]
#[case(
    MigrationAction::ReplaceConstraint { table: "posts".into(), from: fk_user(Some("fk_user"), None), to: fk_user(Some("fk_user"), Some(ReferenceAction::Cascade)) },
    format!("{} {} {} {} {} {}", "Replace constraint:".bright_yellow(), "fk_user FK (user_id) -> users".bright_cyan().bold(), "->".bright_white(), "fk_user FK (user_id) -> users".bright_cyan().bold(), "on".bright_white(), "posts".bright_cyan())
)]
#[case(
    MigrationAction::RemapEnumValues { table: "users".into(), column: "status".into(), mapping: { let mut m = std::collections::BTreeMap::new(); m.insert(0, 10); m.insert(1, 20); m } },
    format!("{} {}.{} [{}]", "Remap enum values:".bright_yellow(), "users".bright_cyan(), "status".bright_cyan().bold(), "0->10, 1->20".bright_white())
)]
#[serial]
fn format_action_cases(#[case] action: MigrationAction, #[case] expected: String) {
    assert_eq!(format_action(&action), expected);
}

#[rstest]
#[serial]
#[tokio::test]
async fn cmd_diff_with_model_and_no_migrations() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    write_config();
    write_model("users");
    fs::create_dir_all("migrations").unwrap();

    let result = cmd_diff().await;
    assert!(result.is_ok());
}

#[rstest]
#[serial]
#[tokio::test]
async fn cmd_diff_when_no_changes() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    write_config();
    // No models, no migrations -> planner should report no actions.
    fs::create_dir_all("models").unwrap();
    fs::create_dir_all("migrations").unwrap();

    let result = cmd_diff().await;
    assert!(result.is_ok());
}

#[test]
fn test_constraint_display_unnamed_index() {
    let constraint = TableConstraint::Index {
        name: None,
        columns: vec!["email".into(), "username".into()],
    };
    let display = format_constraint_type(&constraint);
    assert_eq!(display, "INDEX (email, username)");
}

#[test]
fn test_constraint_display_named_index() {
    let constraint = TableConstraint::Index {
        name: Some("ix_users_email".into()),
        columns: vec!["email".into()],
    };
    let display = format_constraint_type(&constraint);
    assert_eq!(display, "ix_users_email INDEX (email)");
}

#[test]
fn format_missing_fk_warning_named_fk_produces_4_lines() {
    let m = MissingFkSupportingIndex {
        table: "orders".to_string(),
        constraint_name: Some("fk_orders__user".to_string()),
        columns: vec!["user_id".to_string()],
        ref_table: "users".to_string(),
        ref_columns: vec!["id".to_string()],
        suggested_index_name: "ix_orders__user_id".to_string(),
    };
    let out = format_missing_fk_warning(&m);

    assert_eq!(
        out.lines().count(),
        4,
        "4 indented lines: fk / ref / why / fix"
    );
    // The four labels must each appear exactly once.
    for label in ["fk:", "ref:", "why:", "fix:"] {
        assert_eq!(
            out.matches(label).count(),
            1,
            "label `{label}` should appear exactly once in:\n{out}"
        );
    }
    // The user-facing identifiers must surface unescaped.
    assert!(out.contains("fk_orders__user"));
    assert!(out.contains("orders(user_id)"));
    assert!(out.contains("users(id)"));
    assert!(out.contains("ix_orders__user_id"));
}

#[test]
fn format_missing_fk_warning_unnamed_fk_falls_back_to_placeholder() {
    let m = MissingFkSupportingIndex {
        table: "orders".to_string(),
        constraint_name: None,
        columns: vec!["user_id".to_string()],
        ref_table: "users".to_string(),
        ref_columns: vec!["id".to_string()],
        suggested_index_name: "ix_orders__user_id".to_string(),
    };
    let out = format_missing_fk_warning(&m);
    assert!(out.contains("(unnamed)"));
    assert!(out.contains("ix_orders__user_id"));
}

#[test]
fn format_missing_fk_warning_composite_fk_lists_all_columns() {
    let m = MissingFkSupportingIndex {
        table: "audit".to_string(),
        constraint_name: Some("fk_audit__tenant_user".to_string()),
        columns: vec!["tenant_id".to_string(), "user_id".to_string()],
        ref_table: "membership".to_string(),
        ref_columns: vec!["tenant_id".to_string(), "user_id".to_string()],
        suggested_index_name: "ix_audit__tenant_id_user_id".to_string(),
    };
    let out = format_missing_fk_warning(&m);
    assert!(out.contains("audit(tenant_id, user_id)"));
    assert!(out.contains("membership(tenant_id, user_id)"));
    assert!(out.contains("ix_audit__tenant_id_user_id"));
}

// F50: constraint-drop warnings
fn drop_warning(
    kind: vespertide_core::ConstraintKind,
    label: &str,
    table: &str,
    columns: Vec<&str>,
) -> ConstraintDropWarning {
    ConstraintDropWarning {
        action_index: 0,
        table: table.to_string(),
        kind,
        label: label.to_string(),
        columns: columns.into_iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn format_constraint_drop_warning_primary_key_produces_4_lines() {
    let w = drop_warning(
        vespertide_core::ConstraintKind::PrimaryKey,
        "PRIMARY KEY (id)",
        "users",
        vec!["id"],
    );
    let out = format_constraint_drop_warning(&w);

    assert_eq!(
        out.lines().count(),
        4,
        "4 indented lines: on / drop / why / fix"
    );
    for label in ["on:", "drop:", "why:", "fix:"] {
        assert_eq!(
            out.matches(label).count(),
            1,
            "label `{label}` should appear exactly once in:\n{out}"
        );
    }
    assert!(out.contains("users"));
    assert!(out.contains("PRIMARY KEY"));
    assert!(out.contains("PRIMARY KEY (id)"));
    // The label "PRIMARY KEY (id)" already contains "PRIMARY KEY", so only the
    // "KIND — " prefix distinguishes the match arm. Asserting it kills the
    // delete-match-arm mutant on PrimaryKey.
    assert!(
        out.contains("PRIMARY KEY — "),
        "kind_label prefix missing (arm deleted?):\n{out}"
    );
}

#[test]
fn format_constraint_drop_warning_unique_uses_unique_kind_label() {
    let w = drop_warning(
        vespertide_core::ConstraintKind::Unique,
        "uq_users__email UNIQUE (email)",
        "users",
        vec!["email"],
    );
    let out = format_constraint_drop_warning(&w);
    assert!(out.contains("UNIQUE"));
    assert!(out.contains("uq_users__email"));
    // Distinguishes the Unique arm from the label substring (kills
    // delete-match-arm mutant on Unique).
    assert!(
        out.contains("UNIQUE — "),
        "UNIQUE kind_label prefix missing:\n{out}"
    );
}

#[test]
fn format_constraint_drop_warning_foreign_key_uses_fk_kind_label() {
    let w = drop_warning(
        vespertide_core::ConstraintKind::ForeignKey,
        "fk_orders__user FK (user_id) -> users",
        "orders",
        vec!["user_id"],
    );
    let out = format_constraint_drop_warning(&w);
    assert!(out.contains("FOREIGN KEY"));
    assert!(out.contains("fk_orders__user"));
    assert!(out.contains("-> users"));
}

#[test]
fn format_constraint_drop_warning_check_uses_check_kind_label() {
    let w = drop_warning(
        vespertide_core::ConstraintKind::Check,
        "chk_positive_total CHECK (total > 0)",
        "orders",
        vec![],
    );
    let out = format_constraint_drop_warning(&w);
    assert!(out.contains("CHECK"));
    assert!(out.contains("total > 0"));
    // Distinguishes the Check arm from the label substring (kills
    // delete-match-arm mutant on Check).
    assert!(
        out.contains("CHECK — "),
        "CHECK kind_label prefix missing:\n{out}"
    );
}

fn policy_warning(
    on_delete: Option<(Option<ReferenceAction>, Option<ReferenceAction>)>,
    on_update: Option<(Option<ReferenceAction>, Option<ReferenceAction>)>,
) -> FkPolicyChangeWarning {
    FkPolicyChangeWarning {
        action_index: 0,
        table: "orders".to_string(),
        constraint_name: Some("fk_orders__user".to_string()),
        columns: vec!["user_id".to_string()],
        ref_table: "users".to_string(),
        ref_columns: vec!["id".to_string()],
        on_delete_change: on_delete.map(|(before, after)| PolicyDelta { before, after }),
        on_update_change: on_update.map(|(before, after)| PolicyDelta { before, after }),
    }
}

#[test]
fn format_fk_policy_warning_on_delete_only_renders_single_delta_line() {
    let w = policy_warning(
        Some((
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::Restrict),
        )),
        None,
    );
    let out = format_fk_policy_change_warning(&w);
    assert!(out.contains("ON DELETE:"), "missing ON DELETE row: {out}");
    assert!(out.contains("CASCADE"));
    assert!(out.contains("RESTRICT"));
    assert!(
        !out.contains("ON UPDATE:"),
        "ON UPDATE row should be suppressed when unchanged"
    );
    assert!(out.contains("fk_orders__user"));
    assert!(out.contains("orders(user_id)"));
    assert!(out.contains("users(id)"));
}

#[test]
fn format_fk_policy_warning_on_update_only_renders_single_delta_line() {
    let w = policy_warning(None, Some((None, Some(ReferenceAction::Cascade))));
    let out = format_fk_policy_change_warning(&w);
    assert!(!out.contains("ON DELETE:"));
    assert!(out.contains("ON UPDATE:"));
    // None policy renders as the SQL-standard default.
    assert!(out.contains("NO ACTION"));
    assert!(out.contains("CASCADE"));
}

#[test]
fn format_fk_policy_warning_both_changes_render_two_delta_lines() {
    let w = policy_warning(
        Some((
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::SetNull),
        )),
        Some((
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::Restrict),
        )),
    );
    let out = format_fk_policy_change_warning(&w);
    assert!(out.contains("ON DELETE:"));
    assert!(out.contains("SET NULL"));
    assert!(out.contains("ON UPDATE:"));
    assert!(out.contains("RESTRICT"));
    // why + fix advisory must always appear regardless of which delta hit.
    assert!(out.contains("why:"));
    assert!(out.contains("fix:"));
}

#[test]
fn format_fk_policy_warning_unnamed_fk_falls_back_to_placeholder() {
    let mut w = policy_warning(
        Some((
            Some(ReferenceAction::Cascade),
            Some(ReferenceAction::Restrict),
        )),
        None,
    );
    w.constraint_name = None;
    let out = format_fk_policy_change_warning(&w);
    assert!(out.contains("(unnamed)"));
}

fn narrowing(
    table: &str,
    column: &str,
    from_display: &str,
    to_display: &str,
    kind: NarrowingKind,
) -> TypeNarrowingWarning {
    TypeNarrowingWarning {
        action_index: 0,
        table: table.to_string(),
        column: column.to_string(),
        kind,
        from_display: from_display.to_string(),
        to_display: to_display.to_string(),
    }
}

#[test]
fn format_type_narrowing_warning_varchar_renders_all_three_backends() {
    let w = narrowing(
        "users",
        "email",
        "varchar(40)",
        "varchar(30)",
        NarrowingKind::VarcharLength { from: 40, to: 30 },
    );
    let out = format_type_narrowing_warning(&w);
    // Identity line
    assert!(out.contains("users.email"));
    assert!(out.contains("varchar(40)"));
    assert!(out.contains("varchar(30)"));

    // Each backend line must be present and distinct.
    assert!(out.contains("postgres:"));
    assert!(out.contains("mysql:"));
    assert!(out.contains("sqlite:"));

    // Backend behavior must come through.
    assert!(out.to_lowercase().contains("rejects"), "PG should reject");
    assert!(
        out.to_lowercase().contains("silently truncates"),
        "MySQL should silently truncate"
    );
    assert!(
        out.to_lowercase().contains("advisory"),
        "SQLite should show advisory-only"
    );

    // Fix must mention all 3 strategies the user can pick (no `reject`).
    assert!(out.contains("truncate"));
    assert!(out.contains("delete"));
    assert!(out.contains("set_to_value"));
}

#[test]
fn format_type_narrowing_warning_integer_size_uses_integer_impacts() {
    let w = narrowing(
        "events",
        "offset_id",
        "bigint",
        "integer",
        NarrowingKind::IntegerSize {
            from: "bigint",
            to: "integer",
        },
    );
    let out = format_type_narrowing_warning(&w);
    assert!(out.contains("events.offset_id"));
    assert!(out.to_lowercase().contains("out of range"));
    assert!(out.to_lowercase().contains("sql_mode"));
}

#[test]
fn format_type_narrowing_warning_numeric_scale_uses_decimal_impacts() {
    let w = narrowing(
        "accounts",
        "balance",
        "numeric(10,4)",
        "numeric(10,2)",
        NarrowingKind::NumericScale {
            from_scale: 4,
            to_scale: 2,
        },
    );
    let out = format_type_narrowing_warning(&w);
    assert!(out.contains("accounts.balance"));
    assert!(out.contains("numeric(10,4)"));
    assert!(out.contains("numeric(10,2)"));
    assert!(out.to_lowercase().contains("decimal"));
}

// === F20: timezone conversion warnings ===

fn timezone_warning(
    direction: TimezoneConversionDirection,
    current_timezone: Option<String>,
) -> TimezoneConversionWarning {
    TimezoneConversionWarning {
        action_index: 0,
        table: "events".to_string(),
        column: "occurred_at".to_string(),
        direction,
        current_timezone,
    }
}

#[test]
fn format_timezone_conversion_warning_naive_to_aware_without_tz_shows_fix_hint() {
    let w = timezone_warning(TimezoneConversionDirection::NaiveToAware, None);
    let out = format_timezone_conversion_warning(&w);
    assert!(out.contains("events.occurred_at"));
    assert!(out.contains("direction:"));
    assert!(out.contains("why:"));
    assert!(out.contains("AS IF"));
    // Without a current timezone, the fix branch surfaces.
    assert!(out.contains("fix:"));
    assert!(out.contains("vespertide revision"));
    assert!(!out.contains("currently:"));
}

#[test]
fn format_timezone_conversion_warning_aware_to_naive_with_tz_shows_skip_note() {
    let w = timezone_warning(
        TimezoneConversionDirection::AwareToNaive,
        Some("Asia/Seoul".to_string()),
    );
    let out = format_timezone_conversion_warning(&w);
    assert!(out.contains("events.occurred_at"));
    assert!(out.contains("INTO <tz>"));
    // With a current timezone, the `currently:` branch surfaces.
    assert!(out.contains("currently:"));
    assert!(out.contains("Asia/Seoul"));
    assert!(out.contains("skip the prompt"));
    assert!(!out.contains("vespertide revision"));
}

// === emit_* function integration tests (exercise the println!/loop bodies) ===

#[rstest]
#[serial]
#[tokio::test]
async fn cmd_diff_with_actual_change_runs_format_action_loop() {
    // Schema with a pending CreateTable so cmd_diff iterates over `plan.actions`
    // and exercises the format_action println! branch.
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    write_config();
    write_model("books");
    fs::create_dir_all("migrations").unwrap();

    cmd_diff().await.unwrap();
}

#[rstest]
#[serial]
#[tokio::test]
async fn cmd_diff_emits_fk_supporting_index_warning() {
    // A FK column with no supporting index triggers F51's warning emitter,
    // covering `emit_fk_supporting_index_warnings` + `format_missing_fk_warning`
    // print branches end-to-end.
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());

    write_config();
    fs::create_dir_all("migrations").unwrap();
    let models_dir = PathBuf::from("models");
    fs::create_dir_all(&models_dir).unwrap();

    // users.json: simple PK (table name matches fk_user's ref_table "users")
    let user = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef::new(
            "id",
            ColumnType::Simple(SimpleColumnType::Integer),
            false,
        )],
        constraints: vec![pk_id()],
    };
    fs::write(
        models_dir.join("users.json"),
        serde_json::to_string_pretty(&user).unwrap(),
    )
    .unwrap();

    // post.json: FK to user.id with NO index -> triggers warning
    let post = TableDef {
        name: "post".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new(
                "user_id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            ),
        ],
        constraints: vec![pk_id(), fk_user(Some("fk_post__user"), None)],
    };
    fs::write(
        models_dir.join("post.json"),
        serde_json::to_string_pretty(&post).unwrap(),
    )
    .unwrap();

    cmd_diff().await.unwrap();
}

#[test]
fn emit_constraint_drop_warnings_prints_each_warning() {
    // The emitter just println!s; we exercise both the header + per-warning
    // loop branches.
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: pk_id(),
        }],
    };
    emit_constraint_drop_warnings(&plan);
}

#[test]
fn emit_constraint_drop_warnings_empty_returns_early() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![],
    };
    emit_constraint_drop_warnings(&plan);
}

#[test]
fn emit_fk_policy_change_warnings_prints_each_warning() {
    // ReplaceConstraint with a different on_delete triggers FK policy delta
    // detection inside find_fk_policy_changes.
    let from = fk_user(Some("fk_post__user"), Some(ReferenceAction::Cascade));
    let to = fk_user(Some("fk_post__user"), Some(ReferenceAction::Restrict));
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ReplaceConstraint {
            table: "post".into(),
            from,
            to,
        }],
    };
    emit_fk_policy_change_warnings(&plan);
}

#[test]
fn emit_fk_policy_change_warnings_empty_returns_early() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![],
    };
    emit_fk_policy_change_warnings(&plan);
}

#[test]
fn emit_type_narrowing_warnings_prints_each_warning() {
    // varchar(40) -> varchar(20) is a narrowing the planner detects.
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "email".into(),
            new_type: ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar {
                length: 20,
            }),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new(
                "email",
                ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar { length: 40 }),
                false,
            ),
        ],
        constraints: vec![pk_id()],
    }];
    emit_type_narrowing_warnings(&plan, &baseline);
}

#[test]
fn emit_type_narrowing_warnings_empty_returns_early() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![],
    };
    emit_type_narrowing_warnings(&plan, &[]);
}

#[test]
fn emit_timezone_conversion_warnings_prints_each_warning() {
    // timestamp -> timestamptz triggers a timezone conversion warning.
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "events".into(),
            column: "occurred_at".into(),
            new_type: ColumnType::Simple(SimpleColumnType::Timestamptz),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };
    let baseline = vec![TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false),
            ColumnDef::new(
                "occurred_at",
                ColumnType::Simple(SimpleColumnType::Timestamp),
                false,
            ),
        ],
        constraints: vec![pk_id()],
    }];
    emit_timezone_conversion_warnings(&plan, &baseline);
}

#[test]
fn emit_timezone_conversion_warnings_empty_returns_early() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![],
    };
    emit_timezone_conversion_warnings(&plan, &[]);
}

#[test]
fn emit_fk_supporting_index_warnings_empty_returns_early() {
    // No tables -> no missing index -> early return.
    emit_fk_supporting_index_warnings(&[]);
}

// Wildcard arm of format_constraint_drop_warning for the `#[non_exhaustive]`
// ConstraintKind. Index is filtered upstream so it would never reach this
// formatter in production, but constructing the warning directly proves the
// `(unknown)` fallback is reachable for new variants.
#[test]
fn format_constraint_drop_warning_unknown_kind_arm() {
    let w = vespertide_planner::ConstraintDropWarning {
        action_index: 0,
        table: "users".into(),
        kind: vespertide_core::ConstraintKind::Index,
        label: "ix_users__email".into(),
        columns: vec!["email".into()],
    };
    let out = format_constraint_drop_warning(&w);
    assert!(out.contains("(unknown)"));
    assert!(out.contains("users"));
    assert!(out.contains("ix_users__email"));
}
