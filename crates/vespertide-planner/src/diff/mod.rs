use std::collections::BTreeMap;

use rayon::prelude::*;
use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

use crate::error::PlannerError;
use crate::parallel_config::{DIFF_PAR_TABLE_MIN_LEN, diff_par_table_threshold};

mod columns;
mod constraints;
mod ordering;
mod tables;

#[cfg(test)]
mod tests;

/// Diff two schema snapshots into a migration plan.
/// Schemas are normalized for comparison purposes, but the original (non-normalized)
/// tables are used in migration actions to preserve inline constraint definitions.
pub fn diff_schemas(from: &[TableDef], to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    for table in from.iter().chain(to) {
        table
            .validate_unique_column_names()
            .map_err(|e| PlannerError::TableValidation(e.to_string()))?;
    }

    let estimated_actions = from.len().saturating_add(to.len());
    let mut actions: Vec<MigrationAction> = Vec::with_capacity(estimated_actions);

    let from_normalized = tables::normalize_schema(from)?;
    let to_normalized = tables::normalize_schema(to)?;

    // Use BTreeMap for consistent ordering
    // Normalized versions for comparison
    let from_map: BTreeMap<_, _> = from_normalized
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let to_map: BTreeMap<_, _> = to_normalized.iter().map(|t| (t.name.as_str(), t)).collect();

    // Original (non-normalized) versions for migration storage
    let to_original_map: BTreeMap<_, _> = to.iter().map(|t| (t.name.as_str(), t)).collect();

    tables::diff_deleted_tables(&mut actions, &from_map, &to_map);

    // Update existing tables and their indexes/columns.
    // Per-table work is independent; collect local action lists, then flatten in
    // BTreeMap iteration order for deterministic migration output.
    if to_map.len() < diff_par_table_threshold() {
        for (&name, &to_tbl) in &to_map {
            diff_existing_table_into(&mut actions, name, &from_map, to_tbl);
        }
    } else {
        let existing_tables: Vec<(&str, &TableDef)> = to_map
            .iter()
            .map(|(&name, &to_tbl)| (name, to_tbl))
            .collect();
        let per_table_actions: Vec<Vec<MigrationAction>> = existing_tables
            .par_iter()
            .with_min_len(DIFF_PAR_TABLE_MIN_LEN)
            .map(|(name, to_tbl)| diff_existing_table(name, &from_map, to_tbl))
            .collect();
        actions.extend(per_table_actions.into_iter().flatten());
    }

    // SEQUENTIAL BY NATURE: Kahn's algorithm requires in-degree state evolution.
    tables::diff_created_tables(&mut actions, &from_map, &to_map, &to_original_map)?;

    // SEQUENTIAL BY NATURE: Kahn's algorithm requires in-degree state evolution.
    // Sort DeleteTable actions so tables with FK dependencies are deleted first
    ordering::sort_delete_tables(&mut actions, &from_map);

    // Sort so CreateTable comes before AddConstraint that references the new table
    ordering::sort_create_before_add_constraint(&mut actions);

    // Sort so ModifyColumnDefault comes before ModifyColumnType when removing enum values
    // that were used as the default
    ordering::sort_enum_default_dependencies(&mut actions, &from_map);

    Ok(MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 0,
        actions,
    })
}

fn diff_existing_table(
    name: &str,
    from_map: &BTreeMap<&str, &TableDef>,
    to_tbl: &TableDef,
) -> Vec<MigrationAction> {
    let mut local_actions = Vec::with_capacity(4);
    diff_existing_table_into(&mut local_actions, name, from_map, to_tbl);
    local_actions
}

fn diff_existing_table_into(
    actions: &mut Vec<MigrationAction>,
    name: &str,
    from_map: &BTreeMap<&str, &TableDef>,
    to_tbl: &TableDef,
) {
    if let Some(from_tbl) = from_map.get(name) {
        let deleted_columns = columns::diff_columns(actions, name, from_tbl, to_tbl);
        constraints::diff_constraints(actions, name, from_tbl, to_tbl, &deleted_columns);
    }
}
