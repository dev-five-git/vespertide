use vespertide_core::{ColumnDef, TableConstraint, TableDef};

use crate::error::PlannerError;

pub(super) fn create_table(
    schema: &mut Vec<TableDef>,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<(), PlannerError> {
    if schema.iter().any(|t| t.name == table) {
        return Err(PlannerError::TableExists(table.to_string()));
    }

    let table_def = TableDef {
        name: table.to_string().into(),
        description: None,
        columns: columns.to_vec(),
        constraints: constraints.to_vec(),
    };
    // Normalize to promote inline constraints (unique, index, foreign_key, primary_key)
    // to table-level TableConstraint entries. This is critical for SQLite which needs
    // to know about constraints when dropping columns.
    let normalized = table_def.normalize().map_err(|e| {
        PlannerError::TableValidation(format!("Failed to normalize table '{table}': {e}"))
    })?;
    schema.push(normalized);
    Ok(())
}

pub(super) fn delete_table(schema: &mut Vec<TableDef>, table: &str) -> Result<(), PlannerError> {
    let before = schema.len();
    schema.retain(|t| t.name != table);
    if schema.len() == before {
        Err(PlannerError::TableNotFound(table.to_string()))
    } else {
        Ok(())
    }
}

pub(super) fn rename_table(
    schema: &mut Vec<TableDef>,
    from: &str,
    to: &str,
) -> Result<(), PlannerError> {
    if schema.iter().any(|t| t.name == to) {
        Err(PlannerError::TableExists(to.to_string()))
    } else {
        {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == from)
                .ok_or_else(|| PlannerError::TableNotFound(from.to_string()))?;
            tbl.name = to.into();
        }
        for tbl in schema {
            for constraint in &mut tbl.constraints {
                if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                    && ref_table == from
                {
                    *ref_table = to.into();
                }
            }
        }
        Ok(())
    }
}
