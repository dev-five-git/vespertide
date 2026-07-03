//! Fault **F2** — UNIQUE constraint added to a column that may already
//! contain duplicate rows.
//!
//! Adding `UNIQUE` to a freshly-created table or to a brand-new column
//! cannot fail because nothing has been inserted yet. The risky case is
//! `AddConstraint(Unique)` on a column that **already exists in the
//! baseline schema** — its production data may contain duplicates that
//! would cause the migration to fail at apply time (or, with the
//! `DeleteDuplicates` strategy, would cause the planner to silently
//! drop rows). Either way the user must consciously pick a strategy.
//!
//! This module surfaces every such risky addition so the CLI can prompt
//! and stamp the strategy back onto the action.
//!
//! Specifically suppressed (never reported):
//!
//! - `CreateTable` constraints — table is brand new, no rows exist.
//! - `AddColumn` with inline `unique: true` — column is brand new.
//! - `AddConstraint(Unique)` whose every column is **not yet present** in
//!   the baseline. This catches the `AddColumn` + `AddConstraint` pair
//!   that the planner emits when a new column carries an inline unique
//!   constraint that has been normalised into a separate action.
//! - `AddConstraint(Unique)` whose columns include the table's
//!   single-column PRIMARY KEY (the constraint is redundant; nothing to
//!   prompt about).

use std::collections::HashSet;

use vespertide_core::{ColumnName, MigrationAction, MigrationPlan, TableConstraint, TableDef};

/// One risky UNIQUE addition needing user resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueAdditionWarning {
    /// Index of the `AddConstraint(Unique)` action in the plan.
    pub action_index: usize,
    /// Table the constraint is being added to.
    pub table: String,
    /// Constraint name (when declared in the model).
    pub constraint_name: Option<String>,
    /// Columns covered by the new unique constraint.
    pub columns: Vec<String>,
    /// Primary-key shape of the affected table, used by the CLI to decide
    /// which strategy options can be offered.
    pub pk_kind: PkKind,
    /// Foreign keys in the baseline that reference the affected column set.
    /// Informational — the CLI may surface these so the user sees the
    /// downstream impact of a `DeleteDuplicates` strategy.
    pub fk_references: Vec<FkReference>,
}

/// Shape of the baseline table's PRIMARY KEY relative to the unique
/// constraint being added. Drives the CLI's strategy menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PkKind {
    /// Single-column PK that is *not* in the new unique column set. The
    /// SQL generator can emit `DELETE ... NOT IN (SELECT MIN(pk) ...)`
    /// using this PK column.
    SingleAutoCleanupCapable {
        /// Name of the PK column.
        column: String,
    },
    /// Single-column PK that is *part of* the new unique column set.
    /// Auto-cleanup would be a tautology; the user must pre-clean or
    /// pick a different unique column.
    SingleInsideUniqueSet { column: String },
    /// Composite primary key. Auto-cleanup is not implemented in v0.2;
    /// the user must pre-clean manually.
    Composite { columns: Vec<String> },
    /// Table has no primary key. Production schemas reject this via F12
    /// Scenario E; reaching here means the planner is operating on an
    /// invalid schema (defensive case).
    None,
}

/// A foreign-key in the baseline whose `(ref_table, ref_columns)` is
/// affected by this unique addition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FkReference {
    /// Table that owns the foreign key.
    pub child_table: String,
    /// FK constraint name, if any.
    pub constraint_name: Option<String>,
    /// Child columns of the FK.
    pub child_columns: Vec<String>,
}

/// Scan the plan for `AddConstraint(Unique)` on baseline-existing columns.
///
/// Returns warnings in plan-order. Empty when the plan adds no risky
/// UNIQUE constraints.
#[must_use]
pub fn find_unique_additions(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<UniqueAdditionWarning> {
    let mut out = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::Unique { name, columns, .. },
        } = action
        else {
            continue;
        };

        // Only flag when all participating columns exist in the baseline.
        // A unique constraint whose columns include a brand-new column is
        // not at risk — the new column has no rows yet.
        let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str()) else {
            continue;
        };
        let baseline_columns: HashSet<&str> =
            table_def.columns.iter().map(|c| c.name.as_str()).collect();
        let all_existing = columns
            .iter()
            .all(|c| baseline_columns.contains(c.as_str()));
        if !all_existing {
            continue;
        }

        let pk_kind = resolve_pk_kind(table_def, columns);
        let fk_references = collect_fk_references(baseline, table.as_str(), columns);

        out.push(UniqueAdditionWarning {
            action_index: idx,
            table: table.to_string(),
            constraint_name: name.clone(),
            columns: columns.iter().map(ToString::to_string).collect(),
            pk_kind,
            fk_references,
        });
    }
    out
}

fn resolve_pk_kind(table_def: &TableDef, unique_columns: &[ColumnName]) -> PkKind {
    let pk_columns: Vec<String> = table_def
        .constraints
        .iter()
        .find_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.iter().map(ToString::to_string).collect())
            } else {
                None
            }
        })
        .or_else(|| {
            let inline: Vec<String> = table_def
                .columns
                .iter()
                .filter(|col| col.primary_key.is_some())
                .map(|col| col.name.to_string())
                .collect();
            (!inline.is_empty()).then_some(inline)
        })
        .unwrap_or_default();

    match pk_columns.len() {
        0 => PkKind::None,
        1 => {
            let pk = pk_columns.into_iter().next().unwrap_or_default();
            if unique_columns.iter().any(|c| c.as_str() == pk) {
                PkKind::SingleInsideUniqueSet { column: pk }
            } else {
                PkKind::SingleAutoCleanupCapable { column: pk }
            }
        }
        _ => PkKind::Composite {
            columns: pk_columns,
        },
    }
}

fn collect_fk_references(
    baseline: &[TableDef],
    target_table: &str,
    target_columns: &[ColumnName],
) -> Vec<FkReference> {
    let target_set: HashSet<&str> = target_columns.iter().map(ColumnName::as_str).collect();
    let mut out = Vec::new();
    for tbl in baseline {
        for c in &tbl.constraints {
            if let TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                ..
            } = c
            {
                if ref_table.as_str() != target_table {
                    continue;
                }
                let ref_set: HashSet<&str> = ref_columns.iter().map(ColumnName::as_str).collect();
                if ref_set != target_set {
                    continue;
                }
                out.push(FkReference {
                    child_table: tbl.name.to_string(),
                    constraint_name: name.clone(),
                    child_columns: columns.iter().map(ToString::to_string).collect(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, UniqueConstraintStrategy};

    fn col_nn(name: &str) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(SimpleColumnType::Text), false)
    }

    use crate::test_support::{pk, table};

    fn unique(name: Option<&str>, columns: Vec<&str>) -> TableConstraint {
        TableConstraint::Unique {
            name: name.map(ToString::to_string),
            columns: columns.into_iter().map(Into::into).collect(),
            strategy: UniqueConstraintStrategy::default(),
        }
    }

    fn add_unique(t: &str, c: TableConstraint) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: t.into(),
            constraint: c,
        }
    }

    fn plan(actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions,
        }
    }

    /// Case 1: AddConstraint(Unique) on an existing column with a single-
    /// column PK that's not in the unique set → flagged + auto-cleanup capable.
    #[test]
    fn case_01_existing_column_single_pk() {
        let baseline = vec![table(
            "user",
            vec![col_nn("id"), col_nn("email")],
            vec![pk(vec!["id"])],
        )];
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_email"), vec!["email"]),
        )]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].columns, vec!["email"]);
        assert!(matches!(
            w[0].pk_kind,
            PkKind::SingleAutoCleanupCapable { ref column } if column == "id"
        ));
    }

    /// Case 2: AddConstraint(Unique) on a NEW column (not in baseline) →
    /// not flagged (column has no rows yet).
    #[test]
    fn case_02_new_column_not_flagged() {
        let baseline = vec![table("user", vec![col_nn("id")], vec![pk(vec!["id"])])];
        // `email` is being added as a new column elsewhere in the plan; the
        // AddConstraint references it before it exists in baseline.
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_email"), vec!["email"]),
        )]);

        assert!(find_unique_additions(&p, &baseline).is_empty());
    }

    /// Case 3: `CreateTable` internal Unique constraint → not flagged (new table).
    #[test]
    fn case_03_create_table_unique_not_flagged() {
        let baseline: Vec<TableDef> = vec![]; // table does not exist yet
        let p = plan(vec![MigrationAction::CreateTable {
            table: "user".into(),
            columns: vec![col_nn("id"), col_nn("email")],
            constraints: vec![pk(vec!["id"]), unique(Some("uq"), vec!["email"])],
        }]);
        assert!(find_unique_additions(&p, &baseline).is_empty());
    }

    /// Case 4: Composite-PK table → flagged with `Composite` `pk_kind` so
    /// the CLI can warn the user that auto-cleanup is unavailable.
    #[test]
    fn case_04_composite_pk_yields_composite_kind() {
        let baseline = vec![table(
            "membership",
            vec![col_nn("user_id"), col_nn("group_id"), col_nn("email")],
            vec![pk(vec!["user_id", "group_id"])],
        )];
        let p = plan(vec![add_unique(
            "membership",
            unique(Some("uq_m_email"), vec!["email"]),
        )]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert!(matches!(
            w[0].pk_kind,
            PkKind::Composite { ref columns } if columns == &vec!["user_id".to_string(), "group_id".to_string()]
        ));
    }

    /// Case 5: PK column is INSIDE the unique set → `SingleInsideUniqueSet`
    /// kind so the CLI knows auto-cleanup is a tautology.
    #[test]
    fn case_05_pk_inside_unique_set() {
        let baseline = vec![table(
            "user",
            vec![col_nn("id"), col_nn("tenant_id")],
            vec![pk(vec!["id"])],
        )];
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_id_tenant"), vec!["id", "tenant_id"]),
        )]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert!(matches!(
            w[0].pk_kind,
            PkKind::SingleInsideUniqueSet { ref column } if column == "id"
        ));
    }

    /// Case 6: No PK at all → `PkKind::None` (defensive — production
    /// schemas reject this via F12 Scenario E).
    #[test]
    fn case_06_no_pk_defensive() {
        let baseline = vec![table("user", vec![col_nn("email")], vec![])];
        let p = plan(vec![add_unique("user", unique(Some("uq"), vec!["email"]))]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert!(matches!(w[0].pk_kind, PkKind::None));
    }

    /// Case 7: FK references the affected column set → included in
    /// `fk_references` so the CLI can surface downstream impact.
    #[test]
    fn case_07_fk_references_collected() {
        let baseline = vec![
            table(
                "user",
                vec![col_nn("id"), col_nn("email")],
                vec![pk(vec!["id"])],
            ),
            table(
                "session",
                vec![col_nn("id"), col_nn("user_email")],
                vec![
                    pk(vec!["id"]),
                    TableConstraint::ForeignKey {
                        name: Some("fk_session_user".into()),
                        columns: vec!["user_email".into()],
                        ref_table: "user".into(),
                        ref_columns: vec!["email".into()],
                        on_delete: None,
                        on_update: None,
                        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                    },
                ],
            ),
        ];
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_email"), vec!["email"]),
        )]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].fk_references.len(), 1);
        assert_eq!(w[0].fk_references[0].child_table, "session");
        assert_eq!(
            w[0].fk_references[0].constraint_name.as_deref(),
            Some("fk_session_user")
        );
    }

    /// Case 8: `AddColumn` (no `AddConstraint`) → never flagged
    /// (`AddColumn` is not an `AddConstraint` variant).
    #[test]
    fn case_08_add_column_inline_unique_not_flagged() {
        let baseline = vec![table("user", vec![col_nn("id")], vec![pk(vec!["id"])])];
        let p = plan(vec![MigrationAction::AddColumn {
            table: "user".into(),
            column: Box::new(col_nn("email")),
            fill_with: None,
        }]);
        assert!(find_unique_additions(&p, &baseline).is_empty());
    }

    // ── Coverage-closure ──────────────────────────────────────────────

    /// Case 9: `AddConstraint(Unique)` whose target table is not in the
    /// baseline at all (e.g. created later in the plan) — the early
    /// `Some(table_def) else continue` guard fires.
    #[test]
    fn case_09_target_table_not_in_baseline_skipped() {
        let baseline: Vec<TableDef> = vec![];
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_email"), vec!["email"]),
        )]);
        assert!(find_unique_additions(&p, &baseline).is_empty());
    }

    /// Case 10: Table has no table-level PK constraint but has an inline
    /// `primary_key` on a column — exercises the
    /// `or_else(|| { let inline = ...; ... })` fallback in
    /// `resolve_pk_kind` (lines 147-155).
    #[test]
    fn case_10_inline_primary_key_resolves_to_single_auto_cleanup() {
        let mut id = col_nn("id");
        id.primary_key = Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));
        let baseline = vec![table("user", vec![id, col_nn("email")], vec![])];
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_email"), vec!["email"]),
        )]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        // inline PK on `id` ⇒ SingleAutoCleanupCapable { column: "id" }.
        assert!(matches!(
            w[0].pk_kind,
            PkKind::SingleAutoCleanupCapable { ref column } if column == "id"
        ));
    }

    /// Case 11: Table without any PK at all (table-level OR inline) →
    /// `PkKind::None` (the final fallback in `resolve_pk_kind`).
    #[test]
    fn case_11_no_pk_at_all_returns_none_kind() {
        let baseline = vec![table("user", vec![col_nn("email")], vec![])];
        let p = plan(vec![add_unique("user", unique(Some("uq"), vec!["email"]))]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert!(matches!(w[0].pk_kind, PkKind::None));
    }

    #[test]
    fn case_12_fk_references_ignore_other_tables_and_column_sets() {
        let baseline = vec![
            table(
                "user",
                vec![col_nn("id"), col_nn("email")],
                vec![pk(vec!["id"])],
            ),
            table("other", vec![col_nn("id")], vec![pk(vec!["id"])]),
            table(
                "session",
                vec![col_nn("id"), col_nn("other_id"), col_nn("user_id")],
                vec![
                    pk(vec!["id"]),
                    TableConstraint::ForeignKey {
                        name: Some("fk_other".into()),
                        columns: vec!["other_id".into()],
                        ref_table: "other".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                    },
                    TableConstraint::ForeignKey {
                        name: Some("fk_user_id".into()),
                        columns: vec!["user_id".into()],
                        ref_table: "user".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                    },
                ],
            ),
        ];
        let p = plan(vec![add_unique(
            "user",
            unique(Some("uq_user_email"), vec!["email"]),
        )]);

        let w = find_unique_additions(&p, &baseline);
        assert_eq!(w.len(), 1);
        assert!(w[0].fk_references.is_empty());
    }
}
