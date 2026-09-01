use vespertide_core::{TableConstraint, TableDef};

use crate::error::QueryError;
use crate::sql::helpers::{build_sqlite_table_rebuild, require_table_in_schema};
use crate::sql::types::{BuiltQuery, DatabaseBackend};

pub fn requires_rebuild(constraint: &TableConstraint) -> bool {
    matches!(
        constraint,
        TableConstraint::PrimaryKey { .. }
            | TableConstraint::Unique { .. }
            | TableConstraint::ForeignKey { .. }
            | TableConstraint::Check { .. }
    )
}

pub fn build_remove_constraint(
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let table_def = require_table_in_schema(
        current_schema,
        table,
        "SQLite requires current schema information to remove constraints",
    )?;

    let new_constraints = constraints_without(table_def, constraint);
    let constraints_to_recreate = if matches!(constraint, TableConstraint::Unique { .. }) {
        &new_constraints
    } else {
        &table_def.constraints
    };

    Ok(rebuild_table_without_constraint(
        table,
        table_def,
        &new_constraints,
        constraints_to_recreate,
        pending_constraints,
    ))
}

fn constraints_without(table_def: &TableDef, removed: &TableConstraint) -> Vec<TableConstraint> {
    table_def
        .constraints
        .iter()
        .filter(|candidate| !same_constraint(candidate, removed))
        .cloned()
        .collect()
}

fn same_constraint(candidate: &TableConstraint, removed: &TableConstraint) -> bool {
    match (candidate, removed) {
        (TableConstraint::PrimaryKey { .. }, TableConstraint::PrimaryKey { .. }) => true,
        (
            TableConstraint::Unique {
                name: candidate_name,
                columns: candidate_columns,
                ..
            },
            TableConstraint::Unique {
                name: removed_name,
                columns: removed_columns,
                ..
            },
        )
        | (
            TableConstraint::ForeignKey {
                name: candidate_name,
                columns: candidate_columns,
                ..
            },
            TableConstraint::ForeignKey {
                name: removed_name,
                columns: removed_columns,
                ..
            },
        ) => same_named_or_column_constraint(
            candidate_name.as_ref(),
            candidate_columns,
            removed_name.as_ref(),
            removed_columns,
        ),
        (
            TableConstraint::Check {
                name: candidate_name,
                ..
            },
            TableConstraint::Check {
                name: removed_name, ..
            },
        ) => candidate_name == removed_name,
        _ => false,
    }
}

fn same_named_or_column_constraint<T: AsRef<str>, U: AsRef<str>>(
    candidate_name: Option<&String>,
    candidate_columns: &[T],
    removed_name: Option<&String>,
    removed_columns: &[U],
) -> bool {
    if let (Some(candidate_name), Some(removed_name)) = (candidate_name, removed_name) {
        candidate_name == removed_name
    } else {
        candidate_columns.len() == removed_columns.len()
            && candidate_columns
                .iter()
                .zip(removed_columns)
                .all(|(candidate, removed)| candidate.as_ref() == removed.as_ref())
    }
}

fn rebuild_table_without_constraint(
    table: &str,
    table_def: &TableDef,
    new_constraints: &[TableConstraint],
    constraints_to_recreate: &[TableConstraint],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    // SQLite has no native ALTER TABLE DROP CONSTRAINT. Use the canonical
    // rebuild sequence: create temp table without the removed constraint,
    // copy rows, drop original, rename temp, then recreate indexes.
    build_sqlite_table_rebuild(
        DatabaseBackend::Sqlite,
        table,
        &table_def.columns,
        new_constraints,
        &table_def.columns,
        constraints_to_recreate,
        pending_constraints,
    )
}
