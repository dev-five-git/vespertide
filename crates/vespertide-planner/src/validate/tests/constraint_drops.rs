use super::*;
use vespertide_core::ConstraintKind;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn plan_with(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    }
}

fn pk_constraint(columns: Vec<&str>) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: columns.into_iter().map(Into::into).collect(),
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}

fn unique_constraint(name: Option<&str>, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Unique {
        name: name.map(ToString::to_string),
        columns: columns.into_iter().map(Into::into).collect(),
        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First,
        },
    }
}

fn fk_constraint(
    name: Option<&str>,
    columns: Vec<&str>,
    ref_table: &str,
    ref_columns: Vec<&str>,
) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: name.map(ToString::to_string),
        columns: columns.into_iter().map(Into::into).collect(),
        ref_table: ref_table.into(),
        ref_columns: ref_columns.into_iter().map(Into::into).collect(),
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}

fn check_constraint(name: &str, expr: &str) -> TableConstraint {
    TableConstraint::Check {
        name: name.to_string(),
        expr: expr.to_string(),
        strategy: vespertide_core::CheckViolationStrategy::default(),
    }
}

fn index_constraint(name: &str, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Index {
        name: Some(name.to_string()),
        columns: columns.into_iter().map(Into::into).collect(),
    }
}

fn remove(table: &str, constraint: TableConstraint) -> MigrationAction {
    MigrationAction::RemoveConstraint {
        table: table.into(),
        constraint,
    }
}

fn replace(table: &str, from: TableConstraint, to: TableConstraint) -> MigrationAction {
    MigrationAction::ReplaceConstraint {
        table: table.into(),
        from,
        to,
    }
}

fn add(table: &str, constraint: TableConstraint) -> MigrationAction {
    MigrationAction::AddConstraint {
        table: table.into(),
        constraint,
    }
}

// ---------------------------------------------------------------------------
// Warned: PrimaryKey / Unique / ForeignKey / Check
// ---------------------------------------------------------------------------

#[test]
fn primary_key_drop_is_warned() {
    let plan = plan_with(vec![remove("users", pk_constraint(vec!["id"]))]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];
    assert_eq!(w.action_index, 0);
    assert_eq!(w.table, "users");
    assert_eq!(w.kind, ConstraintKind::PrimaryKey);
    assert_eq!(w.label, "PRIMARY KEY (id)");
    assert_eq!(w.columns, vec!["id"]);
}

#[test]
fn unique_drop_is_warned_with_name_in_label() {
    let plan = plan_with(vec![remove(
        "users",
        unique_constraint(Some("uq_users__email"), vec!["email"]),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, ConstraintKind::Unique);
    assert_eq!(warnings[0].label, "uq_users__email UNIQUE (email)");
    assert_eq!(warnings[0].columns, vec!["email"]);
}

#[test]
fn unique_drop_without_name_uses_anonymous_label() {
    let plan = plan_with(vec![remove(
        "users",
        unique_constraint(None, vec!["email"]),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].label, "UNIQUE (email)");
}

#[test]
fn foreign_key_drop_is_warned_with_ref_table_in_label() {
    let plan = plan_with(vec![remove(
        "orders",
        fk_constraint(
            Some("fk_orders__user"),
            vec!["user_id"],
            "users",
            vec!["id"],
        ),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, ConstraintKind::ForeignKey);
    assert_eq!(warnings[0].label, "fk_orders__user FK (user_id) -> users");
}

#[test]
fn check_drop_is_warned_with_expression_in_label() {
    let plan = plan_with(vec![remove(
        "orders",
        check_constraint("chk_positive_total", "total > 0"),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, ConstraintKind::Check);
    assert_eq!(warnings[0].label, "chk_positive_total CHECK (total > 0)");
    // CHECK is expression-based: columns slice is empty by design.
    assert!(warnings[0].columns.is_empty());
}

// ---------------------------------------------------------------------------
// Safe: Index drop / ReplaceConstraint / AddConstraint
// ---------------------------------------------------------------------------

#[test]
fn index_drop_is_not_warned() {
    let plan = plan_with(vec![remove(
        "users",
        index_constraint("ix_users__created_at", vec!["created_at"]),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);
    assert!(warnings.is_empty());
}

#[test]
fn replace_constraint_is_not_warned() {
    let plan = plan_with(vec![replace(
        "orders",
        fk_constraint(
            Some("fk_orders__user"),
            vec!["user_id"],
            "users",
            vec!["id"],
        ),
        fk_constraint(
            Some("fk_orders__user"),
            vec!["user_id"],
            "users",
            vec!["id"],
        ),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);
    assert!(warnings.is_empty());
}

#[test]
fn add_constraint_is_not_warned() {
    let plan = plan_with(vec![add(
        "users",
        unique_constraint(Some("uq_users__email"), vec!["email"]),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);
    assert!(warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

#[test]
fn composite_primary_key_drop_preserves_column_order() {
    let plan = plan_with(vec![remove(
        "user_role",
        pk_constraint(vec!["user_id", "role_id"]),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].columns, vec!["user_id", "role_id"]);
    assert_eq!(warnings[0].label, "PRIMARY KEY (user_id, role_id)");
}

#[test]
fn mixed_plan_returns_only_warned_drops_with_correct_indices() {
    let plan = plan_with(vec![
        add("users", unique_constraint(Some("uq_a"), vec!["a"])), // 0  safe
        remove("users", index_constraint("ix_a", vec!["a"])),     // 1  safe (Index)
        remove(
            "orders",
            fk_constraint(Some("fk_o"), vec!["uid"], "users", vec!["id"]),
        ), // 2  WARN
        replace(
            "users",
            unique_constraint(Some("uq_b"), vec!["b"]),
            unique_constraint(Some("uq_b"), vec!["b"]),
        ), // 3  safe
        remove("users", pk_constraint(vec!["id"])),               // 4  WARN
    ]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].action_index, 2);
    assert_eq!(warnings[0].kind, ConstraintKind::ForeignKey);
    assert_eq!(warnings[1].action_index, 4);
    assert_eq!(warnings[1].kind, ConstraintKind::PrimaryKey);
}

#[test]
fn empty_plan_returns_empty_warnings() {
    let plan = plan_with(vec![]);
    let warnings = find_constraint_drops_without_replacement(&plan);
    assert!(warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Coverage-closure: anonymous FK, anonymous PK label paths
// ---------------------------------------------------------------------------

/// FK without a name → `constraint_label` falls through to the unnamed
/// arm (`format!("FK ({}) -> {ref_table}", ...)`).
#[test]
fn foreign_key_drop_without_name_uses_anonymous_label() {
    let plan = plan_with(vec![remove(
        "orders",
        fk_constraint(None, vec!["user_id"], "users", vec!["id"]),
    )]);
    let warnings = find_constraint_drops_without_replacement(&plan);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, ConstraintKind::ForeignKey);
    assert_eq!(warnings[0].label, "FK (user_id) -> users");
    assert_eq!(warnings[0].columns, vec!["user_id"]);
}

/// Index drops are filtered before `constraint_label` runs — exercise the
/// `kind == Index` early-return alongside a real PK drop in the same plan.
#[test]
fn index_drop_alongside_pk_drop_only_emits_pk_warning() {
    let plan = plan_with(vec![
        remove(
            "users",
            index_constraint("ix_users__created", vec!["created"]),
        ),
        remove("users", pk_constraint(vec!["id"])),
    ]);
    let warnings = find_constraint_drops_without_replacement(&plan);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, ConstraintKind::PrimaryKey);
}
