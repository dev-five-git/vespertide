//! Detect `RemoveConstraint` actions that weaken referential or value
//! integrity without an explicit replacement.
//!
//! This is fault **F50** in the data-dependent migration fault taxonomy:
//! dropping a `PrimaryKey`, `Unique`, `ForeignKey`, or `Check` constraint
//! is *not* a SQL error — the migration succeeds — but every subsequent
//! write that would have been rejected by the dropped constraint is now
//! silently accepted. The damage is invisible until a downstream consumer
//! reads bad data.
//!
//! `Index` removals are deliberately **not** reported here: dropping an
//! index can only regress query performance, never data integrity, and is
//! already covered by F100 (index bloat after migration) which lives in
//! a separate detector.
//!
//! `ReplaceConstraint` is the explicit "I am swapping this constraint for
//! another" action and is therefore considered safe.

use vespertide_core::{
    ConstraintKind, MigrationAction, MigrationPlan, TableConstraint,
    schema::names::join_column_names,
};

/// One `RemoveConstraint` action that drops an integrity-preserving
/// constraint with no `ReplaceConstraint` counterpart in the same plan.
///
/// Returned by [`find_constraint_drops_without_replacement`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintDropWarning {
    /// Index of the offending action in the migration plan.
    pub action_index: usize,
    /// Table the constraint was attached to.
    pub table: String,
    /// High-level kind of the dropped constraint (`PrimaryKey`, `Unique`, `ForeignKey`, `Check`).
    pub kind: ConstraintKind,
    /// Display label of the dropped constraint (constraint name when available,
    /// otherwise its column list / expression).
    pub label: String,
    /// Columns covered by the dropped constraint, in declared order.
    /// Empty for `Check` constraints (which are expression-based).
    pub columns: Vec<String>,
}

/// Scan a migration plan for `RemoveConstraint` actions that silently weaken
/// data integrity.
///
/// Reported kinds: `PrimaryKey`, `Unique`, `ForeignKey`, `Check`.
/// Ignored kinds: `Index` (performance-only, never an integrity guarantee).
///
/// `ReplaceConstraint` actions are not reported because they carry an
/// explicit replacement. If a caller mixes a `RemoveConstraint` followed by
/// an `AddConstraint` of the same shape into the same plan, they should
/// emit `ReplaceConstraint` instead — this detector intentionally does **not**
/// silently pair the two, because the lone `RemoveConstraint` arm gives no
/// transactional guarantee that the replacement was added atomically.
///
/// Static: this performs no data access; it only inspects the structure of
/// the supplied `MigrationPlan`.
#[must_use]
pub fn find_constraint_drops_without_replacement(
    plan: &MigrationPlan,
) -> Vec<ConstraintDropWarning> {
    plan.actions
        .iter()
        .enumerate()
        .filter_map(|(idx, action)| warning_for_action(idx, action))
        .collect()
}

fn warning_for_action(idx: usize, action: &MigrationAction) -> Option<ConstraintDropWarning> {
    let MigrationAction::RemoveConstraint { table, constraint } = action else {
        return None;
    };
    let kind = constraint.kind();
    if matches!(kind, ConstraintKind::Index) {
        return None;
    }
    Some(ConstraintDropWarning {
        action_index: idx,
        table: table.to_string(),
        kind,
        label: constraint_label(constraint),
        columns: constraint
            .columns()
            .iter()
            .map(ToString::to_string)
            .collect(),
    })
}

/// Build a short human-readable label for a constraint. Mirrors the spirit
/// of `vespertide-cli`'s `format_constraint_type` but lives here so the
/// planner crate has no dependency on the CLI's formatter.
fn constraint_label(constraint: &TableConstraint) -> String {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            format!("PRIMARY KEY ({})", join_column_names(columns, ", "))
        }
        TableConstraint::Unique { name, columns, .. } => match name {
            Some(n) => format!("{n} UNIQUE ({})", join_column_names(columns, ", ")),
            None => format!("UNIQUE ({})", join_column_names(columns, ", ")),
        },
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ..
        } => match name {
            Some(n) => format!(
                "{n} FK ({}) -> {ref_table}",
                join_column_names(columns, ", ")
            ),
            None => format!("FK ({}) -> {ref_table}", join_column_names(columns, ", ")),
        },
        TableConstraint::Check { name, expr, .. } => format!("{name} CHECK ({expr})"),
        TableConstraint::Index { name, columns } => match name {
            Some(n) => format!("{n} INDEX ({})", join_column_names(columns, ", ")),
            None => format!("INDEX ({})", join_column_names(columns, ", ")),
        },
        // reason: unreachable - exhaustive over current TableConstraint variants; fallback required only for #[non_exhaustive] future variants
        #[cfg(not(tarpaulin_include))]
        _ => format!("{:?}", constraint.kind()),
    }
}

#[cfg(test)]
mod private_helpers {
    //! Inline tests for the private `constraint_label` helper. The public
    //! `find_constraint_drops_without_replacement` filters Index drops out
    //! before `constraint_label` runs, so the Index arms in
    //! `constraint_label` are unreachable through the integration tests
    //! in `validate/tests/constraint_drops.rs`. They are still part of
    //! the public-facing label contract for any future caller, so we lock
    //! them here directly.
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn index_with_name_label() {
        let c = TableConstraint::Index {
            name: Some("ix_users__email".to_string()),
            columns: vec!["email".into()],
        };
        assert_eq!(constraint_label(&c), "ix_users__email INDEX (email)");
    }

    #[rstest]
    fn index_without_name_label() {
        let c = TableConstraint::Index {
            name: None,
            columns: vec!["email".into(), "tenant_id".into()],
        };
        assert_eq!(constraint_label(&c), "INDEX (email, tenant_id)");
    }

    #[rstest]
    #[case::primary_key(TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() }, "PRIMARY KEY (id)")]
    #[case::unique_named(TableConstraint::Unique { name: Some("uq_users__email".into()), columns: vec!["email".into()], strategy: vespertide_core::UniqueConstraintStrategy::default() }, "uq_users__email UNIQUE (email)")]
    #[case::unique_unnamed(TableConstraint::Unique { name: None, columns: vec!["email".into()], strategy: vespertide_core::UniqueConstraintStrategy::default() }, "UNIQUE (email)")]
    #[case::foreign_key_named(TableConstraint::ForeignKey { name: Some("fk_posts__user_id".into()), columns: vec!["user_id".into()], ref_table: "users".into(), ref_columns: vec!["id".into()], on_delete: None, on_update: None, orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default() }, "fk_posts__user_id FK (user_id) -> users")]
    #[case::foreign_key_unnamed(TableConstraint::ForeignKey { name: None, columns: vec!["user_id".into()], ref_table: "users".into(), ref_columns: vec!["id".into()], on_delete: None, on_update: None, orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default() }, "FK (user_id) -> users")]
    #[case::check(TableConstraint::Check { name: "chk_age".into(), expr: "age > 0".into(), strategy: vespertide_core::CheckViolationStrategy::default() }, "chk_age CHECK (age > 0)")]
    fn constraint_label_formats_integrity_constraints(
        #[case] constraint: TableConstraint,
        #[case] expected: &str,
    ) {
        assert_eq!(constraint_label(&constraint), expected);
    }

    #[rstest]
    fn index_drop_is_filtered_before_warning() {
        let plan = MigrationPlan {
            id: "test".into(),
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::RemoveConstraint {
                table: "users".into(),
                constraint: TableConstraint::Index {
                    name: Some("ix_users__email".into()),
                    columns: vec!["email".into()],
                },
            }],
        };
        assert!(find_constraint_drops_without_replacement(&plan).is_empty());
    }
}
