//! Interactive resolution for `DeleteColumn` / `DeleteTable` actions.
//!
//! When the user removes a column or model file from disk, the planner emits
//! a `DeleteColumn` / `DeleteTable` action. Most of the time this is exactly
//! what the user meant — but sometimes the user *renamed* the column / table
//! (split the old declaration in their editor, retyped the new name) and the
//! pure-state diff cannot tell those two intents apart.
//!
//! Vespertide's answer is to keep the model JSON **purely declarative**
//! (current state only — no `renamed_from` hint) and capture the intent at
//! `vespertide revision` time. For every drop action, the planner gathers
//! candidate rename targets from the *same* plan and asks the user:
//!
//! ```text
//! ⚠️  Resolve drop: column `user.email` (text NOT NULL)
//!
//! This column will be permanently removed. Pick one:
//!   › 1. Rename → email_address  (new column, same type)
//!     2. Rename → backup_email   (new column, type differs: varchar(50))
//!     3. Drop permanently (data lost, irreversible)
//!     4. Cancel migration
//! ```
//!
//! The user's choice is then applied to the plan:
//! - Drop keeps the original `DeleteColumn` / `DeleteTable`.
//! - `RenameTo(target)` rewrites the plan: removes the matching `DeleteX` and
//!   `AddX` / `CreateTable` actions and inserts the equivalent `RenameColumn`
//!   / `RenameTable`. Any property differences (type, nullable, default,
//!   column-set) are auto-emitted as additional `ModifyColumn*` / column-level
//!   actions so a single migration captures the full intent (option β).
//! - Cancel aborts the entire migration.
//!
//! This module is **discovery + plan rewriting**. Interactive prompting lives
//! in `vespertide-cli` (`commands/revision/prompts.rs`); planner stays purely
//! library-level.
//!
//! ## Candidate policy (option B: show all)
//!
//! For a `DeleteColumn { table, column }`, every `AddColumn` action in the
//! same plan whose table matches is a candidate. For a `DeleteTable
//! { table }`, every `CreateTable` action in the same plan is a candidate.
//!
//! Candidates are sorted so the *most likely intended rename target* appears
//! first:
//! 1. `Match::Exact`  — all properties equal (column: type/nullable/default;
//!    table: column-set identical).
//! 2. `Match::SameType` — column type equal but other properties differ.
//! 3. `Match::Different` — type or column-set differs.

use vespertide_core::{
    ColumnDef, ColumnName, DefaultValue, MigrationAction, MigrationPlan, TableConstraint, TableDef,
    TableName,
};

/// What is being dropped, plus enough context to render a useful prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropTarget {
    /// A column drop. `column_type` is a human-readable rendering used by
    /// the prompt (e.g. `"text NOT NULL"`).
    Column {
        table: String,
        column: String,
        column_type: String,
    },
    /// A whole-table drop.
    Table { name: String },
}

impl DropTarget {
    /// Human-friendly key used inside prompt headers / log messages.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Column { table, column, .. } => format!("{table}.{column}"),
            Self::Table { name } => name.clone(),
        }
    }
}

/// One possible rename target for a [`DropTarget`].
///
/// The candidate carries the `target_name` (column or table being added in
/// the same plan), a [`Match`] grade summarising how close it is to the
/// dropped item, and a list of `differences` ready for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameCandidate {
    /// Name of the new column / table that could be the rename target.
    pub target_name: String,
    /// How close this candidate is to the dropped item (drives sort order).
    pub match_quality: Match,
    /// Human-readable description of the differences. Empty for exact matches.
    pub differences: Vec<String>,
}

/// Closeness grade between a dropped item and a candidate add. Drives the
/// candidate's position in the prompt list (Exact first, Different last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// All properties equal — the strongest rename hint.
    Exact,
    /// Same column type but at least one of nullable / default / comment
    /// differs. Column-only.
    SameType,
    /// Type or column-set differs. The user is most likely doing something
    /// other than a rename, but the candidate is still listed for completeness.
    Different,
}

/// One drop action that needs user resolution, with all the rename candidates
/// found in the same plan.
///
/// `candidates` is sorted by [`Match`] (`Exact` → `SameType` → `Different`) and
/// then by `target_name` for determinism. When `candidates.is_empty()` the
/// prompt collapses to a plain confirm / cancel pair (no rename option to
/// offer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropResolution {
    /// Index of the originating action in [`MigrationPlan::actions`]. Used by
    /// the CLI to drive `apply_drop_resolution` and by tests for order
    /// assertions.
    pub action_index: usize,
    pub target: DropTarget,
    pub candidates: Vec<RenameCandidate>,
}

/// Scan the plan for every drop action that needs user resolution.
///
/// Order: action-index ascending. For each `DeleteColumn` / `DeleteTable` the
/// returned [`DropResolution::candidates`] is pre-sorted by closeness so the
/// CLI can render the prompt directly.
#[must_use]
pub fn find_drop_resolutions(plan: &MigrationPlan, baseline: &[TableDef]) -> Vec<DropResolution> {
    let mut out = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        match action {
            MigrationAction::DeleteColumn { table, column } => {
                out.push(resolve_column_drop(
                    idx,
                    table.as_str(),
                    column.as_str(),
                    plan,
                    baseline,
                ));
            }
            MigrationAction::DeleteTable { table } => {
                out.push(resolve_table_drop(idx, table.as_str(), plan, baseline));
            }
            _ => {}
        }
    }
    out
}

fn resolve_column_drop(
    action_index: usize,
    table: &str,
    column: &str,
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> DropResolution {
    // Look up the dropped column's properties from baseline so we can compare
    // against the candidates. If baseline lookup fails we still emit a
    // resolution — the user will see a Drop / Cancel prompt with no
    // candidates rather than crash.
    let dropped = baseline
        .iter()
        .find(|t| t.name == table)
        .and_then(|t| t.columns.iter().find(|c| c.name == column));

    let column_type = dropped.map_or_else(|| "(unknown)".to_string(), render_column_type);

    let mut candidates: Vec<RenameCandidate> = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            MigrationAction::AddColumn {
                table: add_table,
                column: add_column,
                ..
            } if add_table.as_str() == table => Some(column_candidate(dropped, add_column)),
            _ => None,
        })
        .collect();

    sort_candidates(&mut candidates);

    DropResolution {
        action_index,
        target: DropTarget::Column {
            table: table.to_string(),
            column: column.to_string(),
            column_type,
        },
        candidates,
    }
}

fn resolve_table_drop(
    action_index: usize,
    table: &str,
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> DropResolution {
    let baseline_columns: Vec<String> = baseline
        .iter()
        .find(|t| t.name == table)
        .map(|t| t.columns.iter().map(|c| c.name.to_string()).collect())
        .unwrap_or_default();

    let mut candidates: Vec<RenameCandidate> = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            MigrationAction::CreateTable {
                table: new_name,
                columns,
                ..
            } => Some(table_candidate(
                &baseline_columns,
                new_name.as_str(),
                columns,
            )),
            _ => None,
        })
        .collect();

    sort_candidates(&mut candidates);

    DropResolution {
        action_index,
        target: DropTarget::Table {
            name: table.to_string(),
        },
        candidates,
    }
}

fn column_candidate(dropped: Option<&ColumnDef>, added: &ColumnDef) -> RenameCandidate {
    // No baseline info → cannot grade. Treat as Different with a single hint
    // that the comparison was skipped; sorts to the bottom of the list.
    let Some(dropped) = dropped else {
        return RenameCandidate {
            target_name: added.name.to_string(),
            match_quality: Match::Different,
            differences: vec!["dropped column not found in baseline".to_string()],
        };
    };

    let mut differences = Vec::new();

    let same_type = dropped.r#type == added.r#type;
    if !same_type {
        differences.push(format!(
            "type: {} → {}",
            render_column_type(dropped),
            render_column_type(added)
        ));
    }
    if dropped.nullable != added.nullable {
        differences.push(format!(
            "nullable: {} → {}",
            dropped.nullable, added.nullable
        ));
    }
    if dropped.default != added.default {
        differences.push(format!(
            "default: {} → {}",
            render_default(dropped.default.as_ref()),
            render_default(added.default.as_ref())
        ));
    }

    let match_quality = if differences.is_empty() {
        Match::Exact
    } else if same_type {
        Match::SameType
    } else {
        Match::Different
    };

    RenameCandidate {
        target_name: added.name.to_string(),
        match_quality,
        differences,
    }
}

fn table_candidate(
    baseline_columns: &[String],
    new_name: &str,
    new_columns: &[ColumnDef],
) -> RenameCandidate {
    let added_names: Vec<String> = new_columns.iter().map(|c| c.name.to_string()).collect();

    let baseline_set: std::collections::HashSet<&str> =
        baseline_columns.iter().map(String::as_str).collect();
    let added_set: std::collections::HashSet<&str> =
        added_names.iter().map(String::as_str).collect();

    let only_in_baseline: Vec<&&str> = baseline_set.difference(&added_set).collect();
    let only_in_new: Vec<&&str> = added_set.difference(&baseline_set).collect();

    let mut differences = Vec::new();
    if !only_in_baseline.is_empty() {
        let mut names: Vec<String> = only_in_baseline.iter().map(ToString::to_string).collect();
        names.sort();
        differences.push(format!("removed columns: {}", names.join(", ")));
    }
    if !only_in_new.is_empty() {
        let mut names: Vec<String> = only_in_new.iter().map(ToString::to_string).collect();
        names.sort();
        differences.push(format!("added columns: {}", names.join(", ")));
    }

    let match_quality = if differences.is_empty() {
        Match::Exact
    } else {
        // For tables we currently grade everything as Different when the
        // column-set is not identical. Refining this to SameType when the
        // column types align but names differ would need richer per-column
        // diff logic; out of scope for the first pass.
        Match::Different
    };

    RenameCandidate {
        target_name: new_name.to_string(),
        match_quality,
        differences,
    }
}

fn sort_candidates(candidates: &mut [RenameCandidate]) {
    candidates.sort_by(|a, b| {
        a.match_quality
            .cmp(&b.match_quality)
            .then_with(|| a.target_name.cmp(&b.target_name))
    });
}

fn render_column_type(c: &ColumnDef) -> String {
    let nullable = if c.nullable { "" } else { " NOT NULL" };
    format!("{}{nullable}", c.r#type.to_display_string())
}

fn render_default(default: Option<&vespertide_core::DefaultValue>) -> String {
    match default {
        Some(v) => v.to_sql(),
        None => "(none)".to_string(),
    }
}

/// The user's choice for a single [`DropResolution`].
///
/// `Cancel` is handled at the CLI layer (it aborts the whole `revision`
/// command), so this enum only carries the two outcomes that translate into
/// plan changes: `Drop` (keep the original delete action) and `RenameTo`
/// (rewrite into rename + property changes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropChoice {
    /// Confirm the permanent drop. The original `DeleteColumn` /
    /// `DeleteTable` action stays in the plan untouched.
    Drop,
    /// Rewrite into a rename. `target` is the name picked from
    /// [`DropResolution::candidates`].
    RenameTo(String),
}

/// Apply the user's choice to the plan in place.
///
/// On `Drop` this is a no-op (the original action stays). On `RenameTo` the
/// plan is rewritten so the [`DropResolution::target`] becomes a
/// `RenameColumn` / `RenameTable`, with any property differences auto-emitted
/// as follow-up `ModifyColumn*` / column-level actions (option β).
///
/// `baseline` must be the schema state **before** this plan is applied — it
/// is needed so column/table rename can compute the property differences
/// against the *original* declaration, not against the post-add state.
///
/// Returns `Err` only when the chosen target cannot be found in the plan
/// (a programming error: the CLI must pass a `target` that came from
/// `resolution.candidates`).
pub fn apply_drop_resolution(
    plan: &mut MigrationPlan,
    baseline: &[TableDef],
    resolution: &DropResolution,
    choice: &DropChoice,
) -> Result<(), DropResolutionError> {
    match choice {
        DropChoice::Drop => Ok(()),
        DropChoice::RenameTo(target) => match &resolution.target {
            DropTarget::Column { table, column, .. } => {
                apply_column_rename(plan, baseline, table, column, target)
            }
            DropTarget::Table { name } => apply_table_rename(plan, baseline, name, target),
        },
    }
}

/// Errors that can arise from [`apply_drop_resolution`]. Each variant points
/// at a specific authoring or wiring mistake; production code should treat
/// these as bugs.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DropResolutionError {
    /// The plan did not contain the expected `DeleteColumn` / `DeleteTable`.
    #[error("drop action not found in plan: {0}")]
    DropActionMissing(String),
    /// The plan did not contain the expected `AddColumn` / `CreateTable`
    /// target.
    #[error("rename target action not found in plan: {0}")]
    TargetActionMissing(String),
}

/// Rewrite a `DeleteColumn` + `AddColumn` pair into `RenameColumn` plus any
/// `ModifyColumn*` actions needed to capture property differences.
fn apply_column_rename(
    plan: &mut MigrationPlan,
    baseline: &[TableDef],
    table: &str,
    old_column: &str,
    new_column: &str,
) -> Result<(), DropResolutionError> {
    let delete_idx = plan
        .actions
        .iter()
        .position(|a| {
            matches!(a, MigrationAction::DeleteColumn { table: t, column: c }
                if t.as_str() == table && c.as_str() == old_column)
        })
        .ok_or_else(|| {
            DropResolutionError::DropActionMissing(format!("DeleteColumn {table}.{old_column}"))
        })?;

    let add_idx = plan
        .actions
        .iter()
        .position(|a| {
            matches!(
                a,
                MigrationAction::AddColumn { table: t, column, .. }
                    if t.as_str() == table && column.name.as_str() == new_column
            )
        })
        .ok_or_else(|| {
            DropResolutionError::TargetActionMissing(format!("AddColumn {table}.{new_column}"))
        })?;

    // Extract the added column so we can compare against baseline.
    let added: Box<ColumnDef> = match &plan.actions[add_idx] {
        MigrationAction::AddColumn { column, .. } => column.clone(),
        _ => unreachable!("add_idx points at AddColumn"),
    };

    // Look up the old column's properties in the baseline.
    let baseline_col = baseline
        .iter()
        .find(|t| t.name == table)
        .and_then(|t| t.columns.iter().find(|c| c.name == old_column))
        .cloned();

    // Compute follow-up actions BEFORE mutating the plan so we can append
    // them in a deterministic order right after the RenameColumn.
    let mut follow_ups: Vec<MigrationAction> = Vec::new();
    if let Some(old) = baseline_col {
        if old.r#type != added.r#type {
            follow_ups.push(MigrationAction::ModifyColumnType {
                table: TableName::from(table),
                column: ColumnName::from(new_column),
                new_type: added.r#type.clone(),
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            });
        }
        if old.nullable != added.nullable {
            follow_ups.push(MigrationAction::ModifyColumnNullable {
                table: TableName::from(table),
                column: ColumnName::from(new_column),
                nullable: added.nullable,
                fill_with: None,
                delete_null_rows: None,
            });
        }
        if old.default != added.default {
            follow_ups.push(MigrationAction::ModifyColumnDefault {
                table: TableName::from(table),
                column: ColumnName::from(new_column),
                new_default: added.default.as_ref().map(DefaultValue::to_sql),
                backfill: None,
            });
        }
    }

    // Mutate: remove DeleteColumn + AddColumn (higher index first so the
    // earlier index stays valid), then insert RenameColumn at the position
    // of the original DeleteColumn so action ordering stays sensible.
    let (lo, hi) = (delete_idx.min(add_idx), delete_idx.max(add_idx));
    plan.actions.remove(hi);
    plan.actions.remove(lo);

    let rename = MigrationAction::RenameColumn {
        table: TableName::from(table),
        from: ColumnName::from(old_column),
        to: ColumnName::from(new_column),
    };
    plan.actions.insert(lo, rename);
    for (offset, action) in follow_ups.into_iter().enumerate() {
        plan.actions.insert(lo + 1 + offset, action);
    }

    Ok(())
}

/// Rewrite a `DeleteTable` + `CreateTable` pair into `RenameTable` plus the
/// column-level diff between the old declaration and the new one.
fn apply_table_rename(
    plan: &mut MigrationPlan,
    baseline: &[TableDef],
    old_name: &str,
    new_name: &str,
) -> Result<(), DropResolutionError> {
    let delete_idx = plan
        .actions
        .iter()
        .position(|a| {
            matches!(a, MigrationAction::DeleteTable { table }
                if table.as_str() == old_name)
        })
        .ok_or_else(|| DropResolutionError::DropActionMissing(format!("DeleteTable {old_name}")))?;

    let create_idx = plan
        .actions
        .iter()
        .position(|a| {
            matches!(
                a,
                MigrationAction::CreateTable { table, .. } if table.as_str() == new_name
            )
        })
        .ok_or_else(|| {
            DropResolutionError::TargetActionMissing(format!("CreateTable {new_name}"))
        })?;

    // Extract the new table's columns/constraints so we can compute diff
    // against the baseline.
    let (new_columns, new_constraints): (Vec<ColumnDef>, Vec<TableConstraint>) =
        match &plan.actions[create_idx] {
            MigrationAction::CreateTable {
                columns,
                constraints,
                ..
            } => (columns.clone(), constraints.clone()),
            _ => unreachable!("create_idx points at CreateTable"),
        };

    let old_table = baseline.iter().find(|t| t.name == old_name).cloned();

    // Build the follow-up action list — RenameTable first, then column- and
    // constraint-level diff so the table mutations apply to the *renamed*
    // table.
    let mut follow_ups: Vec<MigrationAction> = Vec::new();
    follow_ups.push(MigrationAction::RenameTable {
        from: TableName::from(old_name),
        to: TableName::from(new_name),
    });

    if let Some(old) = old_table {
        follow_ups.extend(diff_table_columns(new_name, &old.columns, &new_columns));
        follow_ups.extend(diff_table_constraints(
            new_name,
            &old.constraints,
            &new_constraints,
        ));
    }

    // Mutate: remove DeleteTable + CreateTable then insert the follow-ups at
    // the earlier of the two positions.
    let (lo, hi) = (delete_idx.min(create_idx), delete_idx.max(create_idx));
    plan.actions.remove(hi);
    plan.actions.remove(lo);
    for (offset, action) in follow_ups.into_iter().enumerate() {
        plan.actions.insert(lo + offset, action);
    }

    Ok(())
}

/// Compute the per-column diff between the old (baseline) table and the new
/// declaration, expressed in the renamed table's namespace.
fn diff_table_columns(table: &str, old: &[ColumnDef], new: &[ColumnDef]) -> Vec<MigrationAction> {
    let mut actions = Vec::new();
    let old_by_name: std::collections::HashMap<&str, &ColumnDef> =
        old.iter().map(|c| (c.name.as_str(), c)).collect();
    let new_by_name: std::collections::HashMap<&str, &ColumnDef> =
        new.iter().map(|c| (c.name.as_str(), c)).collect();

    // Drop columns that vanished from the new declaration.
    for col in old {
        if !new_by_name.contains_key(col.name.as_str()) {
            actions.push(MigrationAction::DeleteColumn {
                table: TableName::from(table),
                column: col.name.clone(),
            });
        }
    }

    // Add columns introduced by the new declaration.
    for col in new {
        if !old_by_name.contains_key(col.name.as_str()) {
            actions.push(MigrationAction::AddColumn {
                table: TableName::from(table),
                column: Box::new(col.clone()),
                fill_with: None,
            });
        }
    }

    // Modify columns that exist in both with property differences.
    for col in new {
        if let Some(old_col) = old_by_name.get(col.name.as_str()) {
            if old_col.r#type != col.r#type {
                actions.push(MigrationAction::ModifyColumnType {
                    table: TableName::from(table),
                    column: col.name.clone(),
                    new_type: col.r#type.clone(),
                    fill_with: None,
                    narrowing_strategy: None,
                    timezone: None,
                });
            }
            if old_col.nullable != col.nullable {
                actions.push(MigrationAction::ModifyColumnNullable {
                    table: TableName::from(table),
                    column: col.name.clone(),
                    nullable: col.nullable,
                    fill_with: None,
                    delete_null_rows: None,
                });
            }
            if old_col.default != col.default {
                actions.push(MigrationAction::ModifyColumnDefault {
                    table: TableName::from(table),
                    column: col.name.clone(),
                    new_default: col.default.as_ref().map(DefaultValue::to_sql),
                    backfill: None,
                });
            }
        }
    }

    actions
}

/// Compute the per-constraint diff between the old (baseline) and new table
/// declarations. Constraints that are byte-equal stay put; everything else
/// is emitted as a remove-then-add pair.
fn diff_table_constraints(
    table: &str,
    old: &[TableConstraint],
    new: &[TableConstraint],
) -> Vec<MigrationAction> {
    let mut actions = Vec::new();
    for c in old {
        if !new.contains(c) {
            actions.push(MigrationAction::RemoveConstraint {
                table: TableName::from(table),
                constraint: c.clone(),
            });
        }
    }
    for c in new {
        if !old.contains(c) {
            actions.push(MigrationAction::AddConstraint {
                table: TableName::from(table),
                constraint: c.clone(),
            });
        }
    }
    actions
}

#[cfg(test)]
mod tests;
