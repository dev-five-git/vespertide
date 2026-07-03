//! Unit tests for `apply_*_choice` helpers (`pub(in crate::commands::revision)`).
//!
//! `format_*_header` private helpers and pure dispatchers
//! (`warning_is_mutable`, `simple_int_label`) are unit-tested via inline
//! `#[cfg(test)] mod tests` blocks at the bottom of each prompt module.
//! This file covers the *crate-visible* `apply_*_choice` mutators that
//! `cmd_revision_core` invokes after the user resolves a warning.

use super::*;
use vespertide_core::{
    CheckViolationStrategy, ColumnName, ForeignKeyOrphanStrategy, PrimaryKeyAdditionStrategy,
    UniqueConstraintStrategy,
};
use vespertide_planner::{
    CheckAdditionWarning, FkOrphanAdditionWarning, PkAdditionKind, PrimaryKeyAdditionWarning,
    SequenceExhaustionKind, SequenceExhaustionWarning, SequenceRiskLevel, UniqueAdditionWarning,
};

fn plan_of(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    }
}

// ── apply_unique_addition_choice ─────────────────────────────────────────

fn unique_add_plan() -> MigrationPlan {
    plan_of(vec![MigrationAction::AddConstraint {
        table: "users".into(),
        constraint: TableConstraint::Unique {
            name: Some("uq".into()),
            columns: vec!["email".into()],
            strategy: UniqueConstraintStrategy::default(),
        },
    }])
}

fn unique_warning() -> UniqueAdditionWarning {
    UniqueAdditionWarning {
        action_index: 0,
        table: "users".into(),
        constraint_name: Some("uq".into()),
        columns: vec!["email".into()],
        pk_kind: vespertide_planner::PkKind::None,
        fk_references: vec![],
    }
}

#[test]
fn apply_unique_choice_delete_duplicates_sets_strategy() {
    let mut plan = unique_add_plan();
    apply_unique_addition_choice(
        &mut plan,
        &unique_warning(),
        UniqueAdditionChoice::DeleteDuplicates(vespertide_core::KeepPolicy::First),
    );
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::Unique { strategy, .. },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert_eq!(
        *strategy,
        UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First
        }
    );
}

#[test]
fn apply_unique_choice_continue_keeps_default_strategy() {
    let mut plan = unique_add_plan();
    apply_unique_addition_choice(
        &mut plan,
        &unique_warning(),
        UniqueAdditionChoice::ContinueWithoutCleanup,
    );
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::Unique { strategy, .. },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert_eq!(*strategy, UniqueConstraintStrategy::default());
}

#[test]
fn apply_unique_choice_oor_action_index_noop() {
    let mut plan = unique_add_plan();
    let mut w = unique_warning();
    w.action_index = 99;
    apply_unique_addition_choice(&mut plan, &w, UniqueAdditionChoice::ContinueWithoutCleanup);
}

#[test]
fn apply_unique_choice_wrong_action_variant_noop() {
    let mut plan = plan_of(vec![MigrationAction::RawSql {
        sql: "select 1".into(),
    }]);
    apply_unique_addition_choice(
        &mut plan,
        &unique_warning(),
        UniqueAdditionChoice::ContinueWithoutCleanup,
    );
    assert!(matches!(plan.actions[0], MigrationAction::RawSql { .. }));
}

// ── apply_fk_orphan_addition_choice ──────────────────────────────────────

fn fk_add_plan() -> MigrationPlan {
    plan_of(vec![MigrationAction::AddConstraint {
        table: "post".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "user".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: ForeignKeyOrphanStrategy::default(),
        },
    }])
}

fn fk_orphan_warning(nullable: bool) -> FkOrphanAdditionWarning {
    FkOrphanAdditionWarning {
        action_index: 0,
        table: "post".into(),
        constraint_name: None,
        columns: vec!["user_id".into()],
        ref_table: "user".into(),
        ref_columns: vec!["id".into()],
        all_columns_nullable: nullable,
    }
}

#[test]
fn apply_fk_orphan_choice_nullify_sets_strategy() {
    let mut plan = fk_add_plan();
    apply_fk_orphan_addition_choice(&mut plan, &fk_orphan_warning(true), FkOrphanChoice::Nullify);
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::ForeignKey {
            orphan_strategy, ..
        },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert_eq!(*orphan_strategy, ForeignKeyOrphanStrategy::NullifyOrphans);
}

#[test]
fn apply_fk_orphan_choice_delete_sets_strategy() {
    let mut plan = fk_add_plan();
    apply_fk_orphan_addition_choice(&mut plan, &fk_orphan_warning(false), FkOrphanChoice::Delete);
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::ForeignKey {
            orphan_strategy, ..
        },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert_eq!(*orphan_strategy, ForeignKeyOrphanStrategy::DeleteOrphans);
}

#[test]
fn apply_fk_orphan_choice_oor_and_wrong_variant_noop() {
    let mut plan = fk_add_plan();
    let mut w = fk_orphan_warning(true);
    w.action_index = 99;
    apply_fk_orphan_addition_choice(&mut plan, &w, FkOrphanChoice::Delete);

    let mut other = plan_of(vec![MigrationAction::RawSql { sql: "x".into() }]);
    apply_fk_orphan_addition_choice(&mut other, &fk_orphan_warning(true), FkOrphanChoice::Delete);
}

// ── apply_check_addition_choice ──────────────────────────────────────────

fn check_add_plan() -> MigrationPlan {
    plan_of(vec![MigrationAction::AddConstraint {
        table: "products".into(),
        constraint: TableConstraint::Check {
            name: "chk".into(),
            expr: "price > 0".into(),
            strategy: CheckViolationStrategy::default(),
        },
    }])
}

fn check_addition_warning(nullable: bool) -> CheckAdditionWarning {
    CheckAdditionWarning {
        action_index: 0,
        table: "products".into(),
        constraint_name: "chk".into(),
        check_expr: "price > 0".into(),
        target_column: "price".into(),
        target_column_nullable: nullable,
    }
}

#[test]
fn apply_check_addition_choice_nullify_writes_column_into_strategy() {
    let mut plan = check_add_plan();
    apply_check_addition_choice(
        &mut plan,
        &check_addition_warning(true),
        CheckViolationChoice::Nullify {
            column: "price".into(),
        },
    );
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::Check { strategy, .. },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    let CheckViolationStrategy::NullifyViolatingColumn { column } = strategy else {
        panic!("expected NullifyViolatingColumn")
    };
    assert_eq!(column, &ColumnName::from("price"));
}

#[test]
fn apply_check_addition_choice_delete_sets_strategy() {
    let mut plan = check_add_plan();
    apply_check_addition_choice(
        &mut plan,
        &check_addition_warning(false),
        CheckViolationChoice::Delete,
    );
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::Check { strategy, .. },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert!(matches!(
        strategy,
        CheckViolationStrategy::DeleteViolatingRows
    ));
}

#[test]
fn apply_check_addition_choice_oor_and_wrong_variant_noop() {
    let mut plan = check_add_plan();
    let mut w = check_addition_warning(true);
    w.action_index = 99;
    apply_check_addition_choice(&mut plan, &w, CheckViolationChoice::Delete);

    let mut other = plan_of(vec![MigrationAction::RawSql { sql: "x".into() }]);
    apply_check_addition_choice(
        &mut other,
        &check_addition_warning(true),
        CheckViolationChoice::Delete,
    );
}

// ── apply_pk_addition_choice ─────────────────────────────────────────────

fn pk_add_plan() -> MigrationPlan {
    plan_of(vec![MigrationAction::AddConstraint {
        table: "users".into(),
        constraint: TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        },
    }])
}

fn pk_warning() -> PrimaryKeyAdditionWarning {
    PrimaryKeyAdditionWarning {
        action_index: 0,
        table: "users".into(),
        columns: vec!["id".into()],
        kind: PkAdditionKind::ExistingColumns,
        nullable_columns: vec![],
        duplicate_possible: true,
        auto_cleanup_capable: true,
    }
}

#[test]
fn apply_pk_choice_delete_duplicates_sets_strategy() {
    let mut plan = pk_add_plan();
    apply_pk_addition_choice(
        &mut plan,
        &pk_warning(),
        PrimaryKeyAdditionChoice::DeleteDuplicates(vespertide_core::KeepPolicy::Last),
    );
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::PrimaryKey { strategy, .. },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert_eq!(
        *strategy,
        PrimaryKeyAdditionStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::Last
        }
    );
}

#[test]
fn apply_pk_choice_continue_keeps_default() {
    let mut plan = pk_add_plan();
    apply_pk_addition_choice(
        &mut plan,
        &pk_warning(),
        PrimaryKeyAdditionChoice::ContinueWithoutCleanup,
    );
    let MigrationAction::AddConstraint {
        constraint: TableConstraint::PrimaryKey { strategy, .. },
        ..
    } = &plan.actions[0]
    else {
        panic!()
    };
    assert_eq!(*strategy, PrimaryKeyAdditionStrategy::default());
}

#[test]
fn apply_pk_choice_oor_and_wrong_variant_noop() {
    let mut plan = pk_add_plan();
    let mut w = pk_warning();
    w.action_index = 99;
    apply_pk_addition_choice(
        &mut plan,
        &w,
        PrimaryKeyAdditionChoice::ContinueWithoutCleanup,
    );

    let mut other = plan_of(vec![MigrationAction::RawSql { sql: "x".into() }]);
    apply_pk_addition_choice(
        &mut other,
        &pk_warning(),
        PrimaryKeyAdditionChoice::ContinueWithoutCleanup,
    );
}

// ── apply_sequence_exhaustion_choice ─────────────────────────────────────

fn col_int(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

fn seq_warning(kind: SequenceExhaustionKind) -> SequenceExhaustionWarning {
    SequenceExhaustionWarning {
        action_index: 0,
        table: "events".into(),
        column: "id".into(),
        current_type: SimpleColumnType::Integer,
        recommended_type: SimpleColumnType::BigInt,
        risk_level: SequenceRiskLevel::Medium,
        kind,
    }
}

#[test]
fn apply_sequence_choice_change_to_bigint_on_create_table_rewrites_column() {
    let mut plan = plan_of(vec![MigrationAction::CreateTable {
        table: "events".into(),
        columns: vec![col_int("id")],
        constraints: vec![],
    }]);
    apply_sequence_exhaustion_choice(
        &mut plan,
        &seq_warning(SequenceExhaustionKind::Primary),
        SequenceExhaustionChoice::ChangeToBigInt,
    );
    let MigrationAction::CreateTable { columns, .. } = &plan.actions[0] else {
        panic!()
    };
    assert_eq!(
        columns[0].r#type,
        ColumnType::Simple(SimpleColumnType::BigInt)
    );
}

#[test]
fn apply_sequence_choice_change_to_bigint_on_modify_column_type_rewrites_new_type() {
    let mut plan = plan_of(vec![MigrationAction::ModifyColumnType {
        table: "events".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Integer),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    apply_sequence_exhaustion_choice(
        &mut plan,
        &seq_warning(SequenceExhaustionKind::PkTypeNarrowing {
            from: SimpleColumnType::BigInt,
        }),
        SequenceExhaustionChoice::ChangeToBigInt,
    );
    let MigrationAction::ModifyColumnType { new_type, .. } = &plan.actions[0] else {
        panic!()
    };
    assert_eq!(*new_type, ColumnType::Simple(SimpleColumnType::BigInt));
}

#[test]
fn apply_sequence_choice_proceed_is_noop() {
    let mut plan = plan_of(vec![MigrationAction::CreateTable {
        table: "events".into(),
        columns: vec![col_int("id")],
        constraints: vec![],
    }]);
    apply_sequence_exhaustion_choice(
        &mut plan,
        &seq_warning(SequenceExhaustionKind::Primary),
        SequenceExhaustionChoice::Proceed,
    );
    let MigrationAction::CreateTable { columns, .. } = &plan.actions[0] else {
        panic!()
    };
    assert_eq!(
        columns[0].r#type,
        ColumnType::Simple(SimpleColumnType::Integer)
    );
}

#[test]
fn apply_sequence_choice_oor_and_unsupported_variant_noop() {
    // Out-of-range action_index.
    let mut plan = plan_of(vec![MigrationAction::CreateTable {
        table: "events".into(),
        columns: vec![col_int("id")],
        constraints: vec![],
    }]);
    let mut w = seq_warning(SequenceExhaustionKind::Primary);
    w.action_index = 99;
    apply_sequence_exhaustion_choice(&mut plan, &w, SequenceExhaustionChoice::ChangeToBigInt);

    // Wrong action variant (AddConstraint - vespertide doesn't rewrite from here).
    let mut other = plan_of(vec![MigrationAction::AddConstraint {
        table: "events".into(),
        constraint: TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        },
    }]);
    apply_sequence_exhaustion_choice(
        &mut other,
        &seq_warning(SequenceExhaustionKind::Primary),
        SequenceExhaustionChoice::ChangeToBigInt,
    );
}

#[test]
fn apply_sequence_choice_create_table_unmatched_column_unchanged() {
    let mut plan = plan_of(vec![MigrationAction::CreateTable {
        table: "events".into(),
        columns: vec![col_int("other_col")],
        constraints: vec![],
    }]);
    apply_sequence_exhaustion_choice(
        &mut plan,
        &seq_warning(SequenceExhaustionKind::Primary),
        SequenceExhaustionChoice::ChangeToBigInt,
    );
    let MigrationAction::CreateTable { columns, .. } = &plan.actions[0] else {
        panic!()
    };
    assert_eq!(
        columns[0].r#type,
        ColumnType::Simple(SimpleColumnType::Integer)
    );
}
