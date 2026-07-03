use std::collections::BTreeMap;

use vespertide_core::{MigrationAction, TableDef};

use crate::error::PlannerError;

use super::ordering::topological_sort_tables;

pub(super) fn normalize_schema(tables: &[TableDef]) -> Result<Vec<TableDef>, PlannerError> {
    tables
        .iter()
        .map(|t| {
            t.normalize().map_err(|e| {
                PlannerError::TableValidation(format!(
                    "Failed to normalize table '{}': {}",
                    t.name, e
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

pub(super) fn diff_deleted_tables(
    actions: &mut Vec<MigrationAction>,
    from_map: &BTreeMap<&str, &TableDef>,
    to_map: &BTreeMap<&str, &TableDef>,
) {
    for name in from_map.keys() {
        if !to_map.contains_key(name) {
            actions.push(MigrationAction::DeleteTable {
                table: (*name).to_string().into(),
            });
        }
    }
}

pub(super) fn diff_created_tables(
    actions: &mut Vec<MigrationAction>,
    from_map: &BTreeMap<&str, &TableDef>,
    to_map: &BTreeMap<&str, &TableDef>,
    to_original_map: &BTreeMap<&str, &TableDef>,
) -> Result<(), PlannerError> {
    let new_tables: Vec<&TableDef> = to_map
        .iter()
        .filter(|(name, _)| !from_map.contains_key(*name))
        .map(|(_, tbl)| *tbl)
        .collect();

    // SEQUENTIAL BY NATURE: Kahn's algorithm requires in-degree state evolution.
    let sorted_new_tables = topological_sort_tables(&new_tables)?;

    for tbl in sorted_new_tables {
        let original_tbl = to_original_map.get(tbl.name.as_str()).ok_or_else(|| {
            PlannerError::TableValidation(format!(
                "normalized table '{}' missing original table",
                tbl.name
            ))
        })?;
        actions.push(MigrationAction::CreateTable {
            table: original_tbl.name.clone(),
            columns: original_tbl.columns.clone(),
            constraints: original_tbl.constraints.clone(),
        });
    }

    Ok(())
}
