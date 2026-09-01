//! Fault **F9**: dangling foreign-key references after a column or table drop.
//!
//! The planner already emits `DeleteColumn` / `DeleteTable` actions when the
//! user removes a column or model file. By default these emit verbatim
//! `ALTER TABLE ... DROP COLUMN` / `DROP TABLE` SQL — but if some *other*
//! table has a foreign key pointing at the dropped target, that FK is now
//! "dangling": it references a column or table that no longer exists.
//!
//! This module performs a **purely static** scan over the planned actions and
//! the baseline schema to detect every such dangling reference. It is data-
//! independent (no DB connection, no row inspection); only the structure of
//! the migration plan and the prior schema are needed.
//!
//! ## Acceptance rule
//!
//! A drop is **not** dangling when the offending FK is removed by the *same*
//! plan, via any of these three escape hatches:
//!
//! 1. The FK constraint is explicitly removed (`RemoveConstraint`).
//! 2. The table that *owns* the FK is dropped (`DeleteTable`) — the FK
//!    disappears with its table.
//! 3. The child column that *participates* in the FK is dropped
//!    (`DeleteColumn`) — every backend treats that as an implicit FK drop.
//!
//! ## Algorithm
//!
//! ```text
//! Step 1. drop_set  = { (table, Some(col)) | DeleteColumn { table, col } in plan }
//!                   ∪ { (table, None)      | DeleteTable  { table } in plan }
//!
//! Step 2. surviving_fks
//!   = baseline FKs
//!     − FKs explicitly removed by RemoveConstraint
//!     − FKs whose owning table is in drop_set (DeleteTable cascade)
//!     − FKs whose owning column is in drop_set (DeleteColumn cascade)
//!     + FKs added by CreateTable / AddConstraint
//!
//! Step 3. for each surviving fk in surviving_fks:
//!           for each (ref_table, ref_column) referenced by fk:
//!             if ref_table ∈ drop_set (whole table)
//!                or (ref_table, ref_column) ∈ drop_set:
//!               emit DanglingFkDrop { ... }
//! ```
//!
//! The output is sorted by `(dropped_table, dropped_column, referencing_table,
//! referencing_constraint)` so it is deterministic across runs and stable
//! under parallel scanning.

use std::collections::{BTreeSet, HashSet};

use vespertide_core::{
    ColumnName, MigrationAction, MigrationPlan, TableConstraint, TableDef,
    schema::names::names_to_strings,
};

/// A single dangling FK reference: dropping `(dropped_table, dropped_column)`
/// would leave `referencing_table` with a foreign key that points at nothing.
///
/// `dropped_column = None` means the whole table is being dropped. Callers
/// converting this to [`crate::error::PlannerError::DanglingForeignKeyAfterDrop`]
/// should preserve the `Option` semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DanglingFkDrop {
    /// Table whose column or whole row is being removed.
    pub dropped_table: String,
    /// Column being removed, or `None` when the whole `dropped_table` is going away.
    pub dropped_column: Option<String>,
    /// Referencing (child) table that still owns the dangling FK.
    pub referencing_table: String,
    /// FK constraint name as declared in the model, or `None` when inlined
    /// without an explicit name.
    pub referencing_constraint: Option<String>,
}

/// Scan a plan against its baseline schema for dangling foreign-key drops.
///
/// Returned violations are sorted lexicographically by
/// `(dropped_table, dropped_column, referencing_table, referencing_constraint)`
/// so output ordering is deterministic for snapshots and batch error rendering.
///
/// Pass the **baseline** schema (the state *before* this plan is applied), not
/// the current model state. Callers usually have this from
/// `vespertide_planner::schema_from_plans` over the previously applied
/// migrations.
#[must_use]
pub fn find_dangling_fk_drops(plan: &MigrationPlan, baseline: &[TableDef]) -> Vec<DanglingFkDrop> {
    let drop_set = collect_drop_set(plan);
    if drop_set.is_empty() {
        // No drops in this plan → nothing can dangle.
        return Vec::new();
    }

    let surviving = collect_surviving_fks(plan, baseline, &drop_set);

    let mut out: BTreeSet<DanglingFkDrop> = BTreeSet::new();
    for fk in &surviving {
        // Composite FK: any single (ref_table, ref_col) pointing into the
        // drop set makes the whole FK dangling. We emit one diagnostic per
        // distinct dropped target so the user sees which one to fix.
        for ref_col in &fk.ref_columns {
            if let Some(dropped) = ref_target_dropped(&drop_set, &fk.ref_table, ref_col) {
                out.insert(DanglingFkDrop {
                    dropped_table: fk.ref_table.clone(),
                    dropped_column: match dropped {
                        DroppedTarget::WholeTable => None,
                        DroppedTarget::Column(c) => Some(c),
                    },
                    referencing_table: fk.owner_table.clone(),
                    referencing_constraint: fk.name.clone(),
                });
            }
        }
    }

    out.into_iter().collect()
}

/// Why a `(ref_table, ref_col)` target is no longer present after the plan.
/// Carries the column name back to the diagnostic when the kill came from
/// `DeleteColumn`; the `WholeTable` case has no column to report.
enum DroppedTarget {
    WholeTable,
    Column(String),
}

/// Set of `(table, Option<column>)` entries being dropped by this plan.
/// `None` column = whole table dropped.
type DropSet = HashSet<(String, Option<String>)>;

fn collect_drop_set(plan: &MigrationPlan) -> DropSet {
    let mut set = HashSet::new();
    for action in &plan.actions {
        match action {
            MigrationAction::DeleteTable { table } => {
                set.insert((table.to_string(), None));
            }
            MigrationAction::DeleteColumn { table, column } => {
                set.insert((table.to_string(), Some(column.to_string())));
            }
            _ => {}
        }
    }
    set
}

/// Decide whether a `(ref_table, ref_col)` target is removed by `drop_set`.
/// Returns `None` when the target survives the plan; otherwise the
/// [`DroppedTarget`] variant explaining how the target was killed.
fn ref_target_dropped(
    drop_set: &DropSet,
    ref_table: &str,
    ref_column: &ColumnName,
) -> Option<DroppedTarget> {
    if drop_set.contains(&(ref_table.to_string(), None)) {
        return Some(DroppedTarget::WholeTable);
    }
    if drop_set.contains(&(ref_table.to_string(), Some(ref_column.to_string()))) {
        return Some(DroppedTarget::Column(ref_column.to_string()));
    }
    None
}

/// Flattened view of a surviving foreign key. Captures everything the
/// dangling check needs without keeping borrows into the baseline.
#[derive(Debug, Clone)]
struct SurvivingFk {
    owner_table: String,
    name: Option<String>,
    ref_table: String,
    ref_columns: Vec<ColumnName>,
}

fn collect_surviving_fks(
    plan: &MigrationPlan,
    baseline: &[TableDef],
    drop_set: &DropSet,
) -> Vec<SurvivingFk> {
    // 1) Start with every FK in the baseline that survives the drop set and
    //    is not explicitly removed by RemoveConstraint.
    let explicit_removed = collect_explicitly_removed_fks(plan);

    let mut surviving: Vec<SurvivingFk> = Vec::new();
    for table in baseline {
        let owner = table.name.as_str();
        // Owner table itself dropped → all its FKs disappear.
        if drop_set.contains(&(owner.to_string(), None)) {
            continue;
        }
        for c in &table.constraints {
            if let TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                ..
            } = c
            {
                // Owner column participating in the FK dropped → FK
                // disappears via column cascade.
                if columns
                    .iter()
                    .any(|col| drop_set.contains(&(owner.to_string(), Some(col.to_string()))))
                {
                    continue;
                }
                // Explicit RemoveConstraint matches this FK.
                if fk_in_removed_set(
                    &explicit_removed,
                    owner,
                    name.as_ref(),
                    columns,
                    ref_table.as_str(),
                ) {
                    continue;
                }
                surviving.push(SurvivingFk {
                    owner_table: owner.to_string(),
                    name: name.clone(),
                    ref_table: ref_table.to_string(),
                    ref_columns: ref_columns.clone(),
                });
            }
        }
    }

    // 2) Add FKs introduced by the plan (CreateTable + AddConstraint).
    //    These can also be dangling against the drop set when a new FK
    //    points at a column the same plan removes — rare but legal.
    for action in &plan.actions {
        match action {
            MigrationAction::CreateTable {
                table, constraints, ..
            } => {
                // If the same plan also drops this newly created table, skip.
                if drop_set.contains(&(table.to_string(), None)) {
                    continue;
                }
                for c in constraints {
                    if let TableConstraint::ForeignKey {
                        name,
                        ref_table,
                        ref_columns,
                        ..
                    } = c
                    {
                        surviving.push(SurvivingFk {
                            owner_table: table.to_string(),
                            name: name.clone(),
                            ref_table: ref_table.to_string(),
                            ref_columns: ref_columns.clone(),
                        });
                    }
                }
            }
            MigrationAction::AddConstraint {
                table,
                constraint:
                    TableConstraint::ForeignKey {
                        name,
                        ref_table,
                        ref_columns,
                        ..
                    },
            } => {
                if drop_set.contains(&(table.to_string(), None)) {
                    continue;
                }
                surviving.push(SurvivingFk {
                    owner_table: table.to_string(),
                    name: name.clone(),
                    ref_table: ref_table.to_string(),
                    ref_columns: ref_columns.clone(),
                });
            }
            _ => {}
        }
    }

    surviving
}

/// Compact key for an explicitly-removed FK. Matches a baseline FK iff
/// `(owner, columns, ref_table)` align; constraint name is used when present.
type RemovedFkKey = (String, Option<String>, Vec<String>, String);

fn collect_explicitly_removed_fks(plan: &MigrationPlan) -> HashSet<RemovedFkKey> {
    let mut set = HashSet::new();
    for action in &plan.actions {
        if let MigrationAction::RemoveConstraint {
            table,
            constraint:
                TableConstraint::ForeignKey {
                    name,
                    columns,
                    ref_table,
                    ..
                },
        } = action
        {
            set.insert((
                table.to_string(),
                name.clone(),
                names_to_strings(columns),
                ref_table.to_string(),
            ));
        }
        // ReplaceConstraint with `from` = ForeignKey also counts as an
        // explicit removal of the old FK.
        if let MigrationAction::ReplaceConstraint {
            table,
            from:
                TableConstraint::ForeignKey {
                    name,
                    columns,
                    ref_table,
                    ..
                },
            ..
        } = action
        {
            set.insert((
                table.to_string(),
                name.clone(),
                names_to_strings(columns),
                ref_table.to_string(),
            ));
        }
    }
    set
}

fn fk_in_removed_set(
    removed: &HashSet<RemovedFkKey>,
    owner: &str,
    name: Option<&String>,
    columns: &[ColumnName],
    ref_table: &str,
) -> bool {
    let key: RemovedFkKey = (
        owner.to_string(),
        name.cloned(),
        names_to_strings(columns),
        ref_table.to_string(),
    );
    removed.contains(&key)
}
