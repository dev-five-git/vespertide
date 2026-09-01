use vespertide_core::{ColumnDef, ColumnName, StrOrBoolOrArray, TableConstraint, TableDef};

use super::find_table_mut;
use crate::error::PlannerError;

pub(super) fn add_constraint(
    schema: &mut [TableDef],
    table: &str,
    constraint: &TableConstraint,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    // Skip if an equivalent constraint already exists (e.g. inline index
    // was already promoted to table-level by normalize() during AddColumn).
    if !tbl.constraints.contains(constraint) {
        tbl.constraints.push(constraint.clone());
    }
    Ok(())
}

pub(super) fn remove_constraint(
    schema: &mut [TableDef],
    table: &str,
    constraint: &TableConstraint,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    tbl.constraints.retain(|c| c != constraint);
    clear_inline_constraint_fields(table, tbl, constraint);
    Ok(())
}

pub(super) fn replace_constraint(
    schema: &mut [TableDef],
    table: &str,
    from: &TableConstraint,
    to: &TableConstraint,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    // Replace the old constraint with the new one in-place.
    let existing = tbl
        .constraints
        .iter_mut()
        .find(|c| *c == from)
        .ok_or_else(|| {
            PlannerError::TableValidation(format!(
                "constraint to replace not found on table '{table}': {from:?}"
            ))
        })?;
    *existing = to.clone();
    // Clear inline fields for the old constraint to prevent normalize()
    // from re-adding it as a ghost constraint.
    clear_inline_constraint_fields(table, tbl, from);
    Ok(())
}

/// Clear inline column fields that correspond to a constraint.
/// This ensures `normalize()` won't re-add the constraint from stale inline fields.
pub(super) fn clear_inline_constraint_fields(
    table: &str,
    tbl: &mut TableDef,
    constraint: &TableConstraint,
) {
    match constraint {
        TableConstraint::Unique { name, columns, .. } => {
            clear_unique_fields(tbl, name.as_deref(), columns);
        }
        TableConstraint::PrimaryKey { columns, .. } => {
            clear_columns_field(tbl, columns, |c| c.primary_key = None);
        }
        TableConstraint::ForeignKey { columns, .. } => {
            clear_columns_field(tbl, columns, |c| c.foreign_key = None);
        }
        TableConstraint::Check { .. } => {}
        TableConstraint::Index { name, columns } => {
            clear_index_fields(table, tbl, name.as_deref(), columns);
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

/// Run `clear` on every column in `tbl` whose name appears in `columns`.
/// Used by the PK/FK arms of [`clear_inline_constraint_fields`] to drop the
/// single inline field that mirrors the table-level constraint being
/// removed. Centralises the "find each column and clear one field" pattern
/// so the two arms cannot drift (e.g. if the lookup later needs case-folding
/// or aliasing).
fn clear_columns_field(
    tbl: &mut TableDef,
    columns: &[ColumnName],
    mut clear: impl FnMut(&mut ColumnDef),
) {
    for col_name in columns {
        if let Some(col) = tbl.columns.iter_mut().find(|c| &c.name == col_name) {
            clear(col);
        }
    }
}

fn clear_unique_fields(tbl: &mut TableDef, name: Option<&str>, columns: &[ColumnName]) {
    if name.is_none()
        && columns.len() == 1
        && let Some(col) = tbl.columns.iter_mut().find(|c| c.name == columns[0])
    {
        col.unique = None;
    }
    if let Some(constraint_name) = name {
        for col in &mut tbl.columns {
            if let Some(StrOrBoolOrArray::Array(names)) = &mut col.unique {
                names.retain(|n| n != constraint_name);
                if names.is_empty() {
                    col.unique = None;
                }
            } else if col.unique.as_ref().and_then(StrOrBoolOrArray::as_str)
                == Some(constraint_name)
            {
                col.unique = None;
            }
        }
    }
}

fn clear_index_fields(table: &str, tbl: &mut TableDef, name: Option<&str>, columns: &[ColumnName]) {
    for col in &mut tbl.columns {
        let column_name = col.name.to_string();
        let auto_name =
            vespertide_naming::build_index_name(table, std::slice::from_ref(&column_name), None);
        if name == Some(auto_name.as_str()) {
            col.index = None;
            break;
        }
    }
    if name.is_none()
        && columns.len() == 1
        && let Some(col) = tbl.columns.iter_mut().find(|c| c.name == columns[0])
    {
        col.index = None;
    }
    if let Some(constraint_name) = name {
        for col in &mut tbl.columns {
            if col.index.as_ref().and_then(StrOrBoolOrArray::as_str) == Some(constraint_name) {
                col.index = None;
            } else if let Some(StrOrBoolOrArray::Array(names)) = &col.index {
                let filtered: Vec<_> = names
                    .iter()
                    .filter(|n| *n != constraint_name)
                    .cloned()
                    .collect();
                if filtered.is_empty() {
                    col.index = None;
                } else if filtered.len() < names.len() {
                    col.index = Some(StrOrBoolOrArray::Array(filtered));
                }
            }
        }
    }
}
