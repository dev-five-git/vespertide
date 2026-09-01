//! Detect `ReplaceConstraint` actions that swap a foreign key with the same
//! column shape but a *different referential action policy*.
//!
//! This is fault **F30** in the data-dependent migration fault taxonomy: the
//! migration SQL succeeds, the data is untouched, the constraint name and
//! columns are unchanged — but the *behaviour* triggered on parent
//! DELETE/UPDATE silently flips. Backend code that assumed the previous
//! policy (cascade auto-cleanup, restrict safety net, set-null bookkeeping)
//! breaks at the first trigger event, which may be hours or weeks after the
//! migration was applied.
//!
//! Both `on_delete` and `on_update` are tracked independently. `None` means
//! "no explicit clause" which the database treats as `NO ACTION`; switching
//! between `None` and `Some(NoAction)` is therefore not flagged, but any
//! transition that changes observable behaviour is.

use vespertide_core::{
    MigrationAction, MigrationPlan, ReferenceAction, TableConstraint,
    schema::names::names_to_strings,
};

/// A single FK constraint whose `on_delete` or `on_update` policy is being
/// changed by a `ReplaceConstraint` action in the migration plan.
///
/// Returned by [`find_fk_policy_changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FkPolicyChangeWarning {
    /// Index of the offending action in the migration plan.
    pub action_index: usize,
    /// Child table that owns the FK constraint.
    pub table: String,
    /// FK constraint name, if explicitly provided.
    pub constraint_name: Option<String>,
    /// Referencing (child) columns in declared order.
    pub columns: Vec<String>,
    /// Parent (referenced) table.
    pub ref_table: String,
    /// Parent (referenced) columns in declared order.
    pub ref_columns: Vec<String>,
    /// `on_delete` policy before and after, when the two differ.
    /// `None` here means "this policy did not change" — *not* "no clause".
    /// Unchanged policies are omitted so the warning surfaces only the deltas.
    pub on_delete_change: Option<PolicyDelta>,
    /// `on_update` policy before and after, when the two differ.
    pub on_update_change: Option<PolicyDelta>,
}

/// A before/after pair for a single referential-action policy.
///
/// `None` inside means "no clause was specified", which the database treats
/// as `NO ACTION`. The `From<Option<ReferenceAction>>` boundary in the
/// detector normalises these so flipping between `None` and `Some(NoAction)`
/// is not reported as a policy change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDelta {
    pub before: Option<ReferenceAction>,
    pub after: Option<ReferenceAction>,
}

/// Scan a migration plan for FK policy changes that will silently alter
/// application behaviour.
///
/// Inspects only `ReplaceConstraint { from: ForeignKey, to: ForeignKey, .. }`
/// pairs. `AddConstraint` / `RemoveConstraint` are not reported here —
/// constraint *introduction* and *removal* belong to F1/F2/F3/F4 (data
/// violation) and F50 (drop without replacement) respectively.
///
/// Static: this performs no data access; it only inspects structural fields
/// of the supplied `MigrationPlan`.
#[must_use]
pub fn find_fk_policy_changes(plan: &MigrationPlan) -> Vec<FkPolicyChangeWarning> {
    plan.actions
        .iter()
        .enumerate()
        .filter_map(|(idx, action)| warning_for_action(idx, action))
        .collect()
}

fn warning_for_action(idx: usize, action: &MigrationAction) -> Option<FkPolicyChangeWarning> {
    let MigrationAction::ReplaceConstraint { table, from, to } = action else {
        return None;
    };
    let (
        TableConstraint::ForeignKey {
            name: from_name,
            columns: from_cols,
            ref_table: from_ref_table,
            ref_columns: from_ref_cols,
            on_delete: from_on_delete,
            on_update: from_on_update,
            ..
        },
        TableConstraint::ForeignKey {
            name: to_name,
            columns: to_cols,
            ref_table: to_ref_table,
            ref_columns: to_ref_cols,
            on_delete: to_on_delete,
            on_update: to_on_update,
            ..
        },
    ) = (from, to)
    else {
        return None;
    };

    let on_delete_change = diff_policy(from_on_delete.as_ref(), to_on_delete.as_ref());
    let on_update_change = diff_policy(from_on_update.as_ref(), to_on_update.as_ref());

    if on_delete_change.is_none() && on_update_change.is_none() {
        return None;
    }

    // Prefer the `to` side's identity fields — that is the target state the
    // user wants. If the constraint name was unchanged, both sides match;
    // if it was renamed, we report the new name (consistent with how the
    // applied migration will leave the database).
    let constraint_name = to_name.clone().or_else(|| from_name.clone());
    let columns = if to_cols.is_empty() {
        from_cols
    } else {
        to_cols
    };
    let ref_table = if to_ref_table.as_str().is_empty() {
        from_ref_table
    } else {
        to_ref_table
    };
    let ref_columns = if to_ref_cols.is_empty() {
        from_ref_cols
    } else {
        to_ref_cols
    };

    Some(FkPolicyChangeWarning {
        action_index: idx,
        table: table.to_string(),
        constraint_name,
        columns: names_to_strings(columns),
        ref_table: ref_table.to_string(),
        ref_columns: names_to_strings(ref_columns),
        on_delete_change,
        on_update_change,
    })
}

/// Compute the policy delta between `before` and `after`. Treats absent
/// clause (`None`) and explicit `NoAction` as equivalent because the
/// database engine implements them identically.
fn diff_policy(
    before: Option<&ReferenceAction>,
    after: Option<&ReferenceAction>,
) -> Option<PolicyDelta> {
    if normalise(before) == normalise(after) {
        None
    } else {
        Some(PolicyDelta {
            before: before.cloned(),
            after: after.cloned(),
        })
    }
}

/// Map `None` and `Some(NoAction)` to a single canonical value so they are
/// not reported as a change against each other.
fn normalise(action: Option<&ReferenceAction>) -> Option<&ReferenceAction> {
    match action {
        Some(ReferenceAction::NoAction) | None => None,
        other => other,
    }
}

/// Render a referential action for display. `None` → `NO ACTION` (the SQL
/// standard default), matching what the database will exhibit at runtime.
#[must_use]
pub fn render_reference_action(action: Option<&ReferenceAction>) -> &'static str {
    match action {
        Some(ReferenceAction::Cascade) => "CASCADE",
        Some(ReferenceAction::Restrict) => "RESTRICT",
        Some(ReferenceAction::SetNull) => "SET NULL",
        Some(ReferenceAction::SetDefault) => "SET DEFAULT",
        Some(ReferenceAction::NoAction) | None => "NO ACTION",
        // reason: unreachable - exhaustive over current ReferenceAction variants; fallback required only for #[non_exhaustive] future variants
        #[cfg(not(tarpaulin_include))]
        Some(_) => "(unknown)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::cascade(Some(ReferenceAction::Cascade), "CASCADE")]
    #[case::restrict(Some(ReferenceAction::Restrict), "RESTRICT")]
    #[case::set_null(Some(ReferenceAction::SetNull), "SET NULL")]
    #[case::set_default(Some(ReferenceAction::SetDefault), "SET DEFAULT")]
    #[case::no_action(Some(ReferenceAction::NoAction), "NO ACTION")]
    #[case::implicit_no_action(None, "NO ACTION")]
    fn render_reference_action_labels_known_policies(
        #[case] action: Option<ReferenceAction>,
        #[case] expected: &str,
    ) {
        assert_eq!(render_reference_action(action.as_ref()), expected);
    }

    fn fk(
        on_delete: Option<ReferenceAction>,
        on_update: Option<ReferenceAction>,
    ) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: Some("fk_posts__user_id".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete,
            on_update,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }
    }

    #[rstest]
    fn replace_constraint_reports_delete_and_update_policy_changes() {
        let plan = MigrationPlan {
            id: "test".into(),
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::ReplaceConstraint {
                table: "posts".into(),
                from: fk(Some(ReferenceAction::Restrict), None),
                to: fk(
                    Some(ReferenceAction::Cascade),
                    Some(ReferenceAction::SetNull),
                ),
            }],
        };

        let warnings = find_fk_policy_changes(&plan);

        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].constraint_name.as_deref(),
            Some("fk_posts__user_id")
        );
        assert_eq!(
            warnings[0]
                .on_delete_change
                .as_ref()
                .map(|d| (&d.before, &d.after)),
            Some((
                &Some(ReferenceAction::Restrict),
                &Some(ReferenceAction::Cascade)
            ))
        );
        assert_eq!(
            warnings[0]
                .on_update_change
                .as_ref()
                .map(|d| (&d.before, &d.after)),
            Some((&None, &Some(ReferenceAction::SetNull)))
        );
    }

    #[rstest]
    fn explicit_no_action_matches_absent_policy() {
        let plan = MigrationPlan {
            id: "test".into(),
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::ReplaceConstraint {
                table: "posts".into(),
                from: fk(None, None),
                to: fk(Some(ReferenceAction::NoAction), None),
            }],
        };
        assert!(find_fk_policy_changes(&plan).is_empty());
    }
}
