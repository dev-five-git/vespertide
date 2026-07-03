//! Fault **F5** — PRIMARY KEY added to a column set that may already
//! contain duplicate values or NULLs.
//!
//! `AddConstraint(PrimaryKey)` against a populated table fails at apply
//! time when the chosen column set:
//!
//! 1. Already contains **duplicate rows** (PK ⇒ UNIQUE), or
//! 2. Contains any **NULL** value (PK ⇒ NOT NULL).
//!
//! v4 catalog describes F5 as "the F1+F2 combination" — this detector
//! identifies the *PK addition* event and surfaces a warning so the CLI
//! can:
//!
//! - Prompt for a [`PrimaryKeyAdditionStrategy`] (handles **duplicates**
//!   via the same `DeleteDuplicates { keep }` mechanism as F2), and
//! - Trigger the existing F1 `fill_with` mechanism for any nullable PK
//!   column (so NULL values are backfilled before the constraint is
//!   added).
//!
//! Specifically suppressed (never reported):
//!
//! - `CreateTable` constraints — table is brand new, no rows exist.
//! - `AddConstraint(PrimaryKey)` whose every column is **not yet
//!   present** in the baseline (kind = `NewColumns`).
//! - Mixed new-and-existing column sets (kind = `Mixed`) — conservative
//!   skip; the new-column path is hard for the user to reason about and
//!   the planner cannot infer intent.
//! - Tables where an existing baseline UNIQUE constraint already covers
//!   the new PK column set — duplicate prevention is already guaranteed.
//!
//! [`PrimaryKeyAdditionStrategy`]: vespertide_core::PrimaryKeyAdditionStrategy

use std::collections::HashSet;

use vespertide_core::{ColumnName, MigrationAction, MigrationPlan, TableConstraint, TableDef};

/// One risky PRIMARY KEY addition needing user resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryKeyAdditionWarning {
    /// Index of the `AddConstraint(PrimaryKey)` action in the plan.
    pub action_index: usize,
    /// Table the constraint is being added to.
    pub table: String,
    /// Columns covered by the new PRIMARY KEY, in declared order.
    pub columns: Vec<String>,
    /// Composition shape of the column set — determines whether the
    /// CLI should prompt at all.
    pub kind: PkAdditionKind,
    /// Baseline columns participating in the PK that are nullable.
    /// The CLI uses this list to trigger the existing F1 `fill_with`
    /// mechanism for each one (NULL values must be backfilled before
    /// the PK is added, since PK ⇒ NOT NULL).
    pub nullable_columns: Vec<String>,
    /// `true` when the baseline has no UNIQUE constraint covering the
    /// new PK column set — duplicate cleanup may be necessary.
    pub duplicate_possible: bool,
    /// `true` when a single-column PK is being added and that column
    /// is not already covered by a baseline UNIQUE constraint —
    /// auto-cleanup (`DELETE` ... `NOT IN (SELECT MIN(rowid) ...)`)
    /// is feasible.
    pub auto_cleanup_capable: bool,
}

/// Composition shape of the new PK column set relative to the
/// baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PkAdditionKind {
    /// All PK columns exist in the baseline — data violations are
    /// possible and the warning surfaces.
    ExistingColumns,
    /// All PK columns are being added in this plan — no baseline
    /// data, skip.
    NewColumns,
    /// Mixed new and existing columns — conservative skip (see module
    /// doc).
    Mixed,
}

/// Scan the plan for `AddConstraint(PrimaryKey)` against tables that
/// already exist in the baseline schema with overlapping columns.
///
/// Returns warnings in plan-order. Empty when the plan adds no risky
/// PRIMARY KEY constraints (no PK additions at all, or every PK
/// addition targets brand-new columns / a brand-new table / a
/// column set already covered by a baseline UNIQUE).
#[must_use]
pub fn find_primary_key_additions(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<PrimaryKeyAdditionWarning> {
    let mut out = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::PrimaryKey { columns, .. },
        } = action
        else {
            continue;
        };

        let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str()) else {
            // Table is being created in this plan — no rows yet.
            continue;
        };

        let baseline_columns: HashSet<&str> =
            table_def.columns.iter().map(|c| c.name.as_str()).collect();
        let pk_column_names: Vec<&str> = columns.iter().map(ColumnName::as_str).collect();

        let n_existing = pk_column_names
            .iter()
            .filter(|c| baseline_columns.contains(*c))
            .count();
        let kind = match n_existing {
            0 => PkAdditionKind::NewColumns,
            n if n == pk_column_names.len() => PkAdditionKind::ExistingColumns,
            _ => PkAdditionKind::Mixed,
        };

        if !matches!(kind, PkAdditionKind::ExistingColumns) {
            continue;
        }

        // Collect nullable baseline columns participating in the PK.
        let nullable_columns: Vec<String> = pk_column_names
            .iter()
            .filter_map(|c| {
                table_def
                    .columns
                    .iter()
                    .find(|cd| cd.name.as_str() == *c && cd.nullable)
                    .map(|cd| cd.name.to_string())
            })
            .collect();

        // Check whether any baseline UNIQUE constraint already covers
        // the new PK column set. If yes, duplicates are already
        // prevented and we skip the warning.
        let pk_set: HashSet<&str> = pk_column_names.iter().copied().collect();
        let covered_by_unique = table_def.constraints.iter().any(|c| {
            if let TableConstraint::Unique { columns: uc, .. } = c {
                let unique_set: HashSet<&str> = uc.iter().map(ColumnName::as_str).collect();
                unique_set == pk_set
            } else {
                false
            }
        });
        if covered_by_unique && nullable_columns.is_empty() {
            // Both NULL and duplicate violations are already prevented
            // by baseline constraints.
            continue;
        }

        let duplicate_possible = !covered_by_unique;
        let auto_cleanup_capable = duplicate_possible && pk_column_names.len() == 1;

        out.push(PrimaryKeyAdditionWarning {
            action_index: idx,
            table: table.to_string(),
            columns: pk_column_names.iter().map(|s| (*s).to_string()).collect(),
            kind,
            nullable_columns,
            duplicate_possible,
            auto_cleanup_capable,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, PrimaryKeyAdditionStrategy,
        SimpleColumnType, TableConstraint, TableDef, TableName,
    };

    fn col(name: &str, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(name: &str, cols: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: cols,
            constraints,
        }
    }

    fn add_pk(table: &str, columns: &[&str]) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: TableName::from(table),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: columns.iter().map(|c| (*c).into()).collect(),
                strategy: PrimaryKeyAdditionStrategy::default(),
            },
        }
    }

    fn plan(actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: "test".into(),
            version: 1,
            comment: None,
            created_at: None,
            actions,
        }
    }

    #[rstest]
    fn case_01_single_existing_not_null_no_unique_flagged() {
        let baseline = vec![table(
            "users",
            vec![col("id", false), col("email", false)],
            vec![],
        )];
        let p = plan(vec![add_pk("users", &["email"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].kind, PkAdditionKind::ExistingColumns);
        assert_eq!(ws[0].columns, vec!["email".to_string()]);
        assert!(ws[0].duplicate_possible);
        assert!(ws[0].auto_cleanup_capable);
        assert!(ws[0].nullable_columns.is_empty());
    }

    #[rstest]
    fn case_02_single_existing_nullable_flagged_with_nullable_list() {
        let baseline = vec![table(
            "users",
            vec![col("id", false), col("email", true)],
            vec![],
        )];
        let p = plan(vec![add_pk("users", &["email"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].nullable_columns, vec!["email".to_string()]);
        assert!(ws[0].duplicate_possible);
    }

    #[rstest]
    fn case_03_new_column_skipped() {
        let baseline = vec![table("users", vec![col("id", false)], vec![])];
        let p = plan(vec![add_pk("users", &["new_col"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_04_mixed_new_existing_skipped() {
        let baseline = vec![table("users", vec![col("id", false)], vec![])];
        let p = plan(vec![add_pk("users", &["id", "new_col"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_05_table_not_in_baseline_skipped() {
        let baseline: Vec<TableDef> = vec![];
        let p = plan(vec![add_pk("users", &["id"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_06_composite_pk_existing_columns_flagged() {
        let baseline = vec![table(
            "audit",
            vec![col("team_id", true), col("member_id", false)],
            vec![],
        )];
        let p = plan(vec![add_pk("audit", &["team_id", "member_id"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(
            ws[0].columns,
            vec!["team_id".to_string(), "member_id".to_string()]
        );
        assert_eq!(ws[0].nullable_columns, vec!["team_id".to_string()]);
        // composite ⇒ no auto cleanup (single-column only)
        assert!(!ws[0].auto_cleanup_capable);
    }

    #[rstest]
    fn case_07_existing_unique_covers_pk_skipped() {
        let baseline = vec![table(
            "users",
            vec![col("id", false), col("email", false)],
            vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::default(),
            }],
        )];
        let p = plan(vec![add_pk("users", &["email"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        // baseline UNIQUE on email already prevents duplicates, and column
        // is NOT NULL — fully safe, no warning.
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_08_unique_covers_but_nullable_still_flagged() {
        // baseline UNIQUE prevents duplicates BUT column is nullable —
        // F5 should still flag for NULL handling.
        let baseline = vec![table(
            "users",
            vec![col("id", false), col("email", true)],
            vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::default(),
            }],
        )];
        let p = plan(vec![add_pk("users", &["email"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].nullable_columns, vec!["email".to_string()]);
        // covered by unique ⇒ duplicate prevention OK, but the warning
        // still surfaces because of the nullable column.
        assert!(!ws[0].duplicate_possible);
        assert!(!ws[0].auto_cleanup_capable);
    }

    #[rstest]
    fn case_09_inline_pk_baseline_is_detected_as_single_pk() {
        let mut id = col("id", false);
        id.primary_key = Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));
        let baseline = vec![table("users", vec![id, col("email", false)], vec![])];
        let p = plan(vec![add_pk("users", &["email"])]);

        let ws = find_primary_key_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert!(ws[0].duplicate_possible);
    }

    /// L125: the `else { false }` arm inside the `iter().any(...)`
    /// closure that searches for a baseline UNIQUE constraint matching
    /// the new PK column set. Existing cases only exercise the
    /// Unique-matching path or empty constraint lists; this test
    /// drops a non-Unique (Check / Index) constraint BEFORE any
    /// matching one so the closure visits the `_ => false` branch.
    #[rstest]
    fn case_10_non_unique_constraints_skipped_in_any_closure() {
        let baseline = vec![table(
            "users",
            vec![col("id", false), col("email", false)],
            vec![
                // L125 wildcard arm: Check returns `false` from the
                // closure so `iter().any(...)` keeps scanning.
                TableConstraint::Check {
                    name: "chk_email".into(),
                    expr: "email <> ''".into(),
                    strategy: vespertide_core::CheckViolationStrategy::default(),
                },
                // Same wildcard for an Index constraint.
                TableConstraint::Index {
                    name: Some("ix_email".into()),
                    columns: vec!["email".into()],
                },
                // Finally a matching Unique → covered_by_unique = true
                // → with NOT-NULL column, the warning is fully
                // suppressed (case_07 pattern).
                TableConstraint::Unique {
                    name: Some("uq_email".into()),
                    columns: vec!["email".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::default(),
                },
            ],
        )];
        let p = plan(vec![add_pk("users", &["email"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        // Unique constraint covers the PK set AND email is NOT NULL → no warning.
        assert!(
            ws.is_empty(),
            "fully-covered single-column PK should not warn, got: {ws:?}"
        );
    }

    /// L125 also hits when the baseline has ONLY non-Unique
    /// constraints (e.g. a sole Check). The `any(...)` closure
    /// returns `false` for every constraint via the `_ => false`
    /// arm, so `covered_by_unique` ends up false and the warning
    /// is emitted (because duplicates remain possible).
    #[rstest]
    fn case_11_only_non_unique_constraints_still_warns_via_wildcard() {
        let baseline = vec![table(
            "users",
            vec![col("id", false), col("email", false)],
            vec![
                // Only non-Unique constraint — every iter.any() step
                // takes L125's `_ => false` branch.
                TableConstraint::Check {
                    name: "chk_email".into(),
                    expr: "email <> ''".into(),
                    strategy: vespertide_core::CheckViolationStrategy::default(),
                },
            ],
        )];
        let p = plan(vec![add_pk("users", &["email"])]);
        let ws = find_primary_key_additions(&p, &baseline);
        // No Unique → covered_by_unique = false → warning fires.
        assert_eq!(ws.len(), 1);
        assert!(ws[0].duplicate_possible);
    }
}
