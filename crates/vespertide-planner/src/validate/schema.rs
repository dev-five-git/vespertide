use std::collections::{BTreeMap, HashSet};

use rayon::prelude::*;
use vespertide_core::{TableConstraint, TableDef, schema::primary_key::PrimaryKeySyntax};

use super::enums::validate_column;
use super::foreign_keys::validate_foreign_key_constraint;
use crate::error::{MultipleErrors, PlannerError};
use crate::parallel_config::{VALIDATE_SCHEMA_PAR_MIN_LEN, validate_schema_par_threshold};

/// Validate a schema for data integrity issues.
///
/// Returns `Ok(())` when every table is valid. On failure the returned error
/// follows this contract so existing single-violation tests stay byte-identical
/// while batch callers see every problem in one shot:
///
/// - exactly **1** violation → that violation's bare [`PlannerError`] variant,
/// - **2 or more** violations → wrapped in [`PlannerError::Multiple`] with all
///   violations preserved in table-index order (duplicate-name errors come
///   first, then per-table violations in declared order).
///
/// Checks for:
/// - Duplicate table names
/// - Foreign keys referencing non-existent tables
/// - Foreign keys referencing non-existent columns
/// - Indexes referencing non-existent columns
/// - Constraints referencing non-existent columns
/// - Empty constraint column lists
pub fn validate_schema(schema: &[TableDef]) -> Result<(), PlannerError> {
    let mut violations = find_schema_violations(schema);
    match violations.len() {
        0 => Ok(()),
        1 => Err(violations.remove(0)),
        _ => Err(PlannerError::Multiple(Box::new(MultipleErrors(violations)))),
    }
}

/// Collect every schema-level violation in one pass.
///
/// Returned violations follow a stable order:
/// 1. `DuplicateTableName` errors, in encounter order.
/// 2. Per-table violations, ordered by the table's index in `schema`.
///
/// Within a single table, the first failing check wins (enum / PK / column /
/// constraint helpers are still first-fail at the table-local level — see
/// `collect_table_violations`). This keeps the helpers simple while still
/// guaranteeing that *every table* with a problem contributes one violation.
///
/// Prefer this over [`validate_schema`] when surfacing **all** violations to
/// the user (CLI batch error message, LSP diagnostics, etc.).
#[must_use]
pub fn find_schema_violations(schema: &[TableDef]) -> Vec<PlannerError> {
    let mut violations = Vec::new();

    // Phase 1: duplicate-name detection — sequential, set-accumulated.
    // Reported before per-table violations so the user sees structural
    // problems first.
    let mut table_names = HashSet::new();
    for table in schema {
        if !table_names.insert(&table.name) {
            violations.push(PlannerError::DuplicateTableName(table.name.to_string()));
        }
    }

    // Phase 2: per-table validation. Each table contributes at most one
    // violation (its first failing check). Collect indexed so parallel
    // execution produces the same order as sequential.
    let table_map: BTreeMap<_, _> = schema
        .iter()
        .map(|t| {
            let columns: HashSet<_> = t.columns.iter().map(|c| c.name.as_str()).collect();
            (t.name.as_str(), columns)
        })
        .collect();

    let mut per_table: Vec<(usize, PlannerError)> =
        if schema.len() < validate_schema_par_threshold() {
            schema
                .iter()
                .enumerate()
                .filter_map(|(index, table)| {
                    validate_table_entry(table, &table_map)
                        .err()
                        .map(|e| (index, e))
                })
                .collect()
        } else {
            schema
                .par_iter()
                .with_min_len(VALIDATE_SCHEMA_PAR_MIN_LEN)
                .enumerate()
                .filter_map(|(index, table)| {
                    validate_table_entry(table, &table_map)
                        .err()
                        .map(|e| (index, e))
                })
                .collect()
        };

    per_table.sort_by_key(|(index, _)| *index);
    violations.extend(per_table.into_iter().map(|(_, err)| err));

    violations
}

fn validate_table_entry(
    table: &TableDef,
    table_map: &BTreeMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    table
        .validate_unique_column_names()
        .map_err(|e| PlannerError::TableValidation(e.to_string()))?;
    validate_table(table, table_map)?;
    super::check_default::validate_default_vs_check(table)?;
    super::check_between_order::validate_between_boundary_order(table)?;
    super::check_self_contradiction::validate_self_contradiction(table)
}

pub(super) fn validate_table(
    table: &TableDef,
    table_map: &BTreeMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    let table_columns: HashSet<_> = table.columns.iter().map(|c| c.name.as_str()).collect();

    // Check that the table has a primary key
    // Primary key can be defined either:
    // 1. As a table-level constraint (TableConstraint::PrimaryKey)
    // 2. As an inline column definition (column.primary_key = Some(...))
    let has_table_pk = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));
    let has_inline_pk = table.columns.iter().any(|c| c.primary_key.is_some());

    if !has_table_pk && !has_inline_pk {
        return Err(PlannerError::MissingPrimaryKey(table.name.to_string()));
    }

    // F12 Scenario C: every column participating in a PRIMARY KEY must be
    // NOT NULL. SQL standard defines `PRIMARY KEY` as `UNIQUE + NOT NULL`;
    // PG, MySQL, and SQLite (in strict mode) all enforce it. Allowing a
    // contradicting `nullable: true` would either silently get overridden
    // at SQL-emit time or fall back to SQLite's historical bug behaviour.
    // Reject the model up front so the typed-schema promise holds.
    let mut pk_columns: HashSet<&str> = HashSet::new();
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            for c in columns {
                pk_columns.insert(c.as_str());
            }
        }
    }
    for column in &table.columns {
        if column.primary_key.is_some() {
            pk_columns.insert(column.name.as_str());
        }
    }
    for column in &table.columns {
        if pk_columns.contains(column.name.as_str()) && column.nullable {
            return Err(PlannerError::PrimaryKeyColumnNullable {
                table: table.name.to_string(),
                column: column.name.to_string(),
            });
        }
    }

    // Validate auto_increment columns have integer types
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey {
            auto_increment: true,
            columns,
            ..
        } = constraint
        {
            for col_name in columns {
                if let Some(column) = table.columns.iter().find(|c| c.name == *col_name)
                    && !column.r#type.supports_auto_increment()
                {
                    return Err(PlannerError::InvalidAutoIncrement(
                        table.name.to_string(),
                        col_name.to_string(),
                        format!("{:?}", column.r#type),
                    ));
                }
            }
        }
    }

    // Validate auto_increment on inline primary_key definitions
    for column in &table.columns {
        if let Some(pk_syntax) = &column.primary_key {
            let has_auto_increment = match pk_syntax {
                PrimaryKeySyntax::Bool(_) => false,
                PrimaryKeySyntax::Object(pk_def) => pk_def.auto_increment,
            };
            if has_auto_increment && !column.r#type.supports_auto_increment() {
                return Err(PlannerError::InvalidAutoIncrement(
                    table.name.to_string(),
                    column.name.to_string(),
                    format!("{:?}", column.r#type),
                ));
            }
        }
    }

    // Validate columns (enum types)
    for column in &table.columns {
        validate_column(column, &table.name)?;
    }

    // Validate constraints (including indexes)
    for constraint in &table.constraints {
        validate_constraint(constraint, &table.name, &table_columns, table_map)?;
    }

    Ok(())
}

fn validate_constraint(
    constraint: &TableConstraint,
    table_name: &str,
    table_columns: &HashSet<&str>,
    table_map: &BTreeMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "PrimaryKey".to_string(),
                ));
            }
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "PrimaryKey".to_string(),
                        col.to_string(),
                    ));
                }
            }
        }
        TableConstraint::Unique { columns, .. } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "Unique".to_string(),
                ));
            }
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "Unique".to_string(),
                        col.to_string(),
                    ));
                }
            }
        }
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => validate_foreign_key_constraint(
            table_name,
            table_columns,
            table_map,
            columns,
            ref_table,
            ref_columns,
        )?,
        TableConstraint::Check { .. } => {
            // Check constraints are just expressions, no validation needed
        }
        TableConstraint::Index { name, columns } => {
            if columns.is_empty() {
                let index_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    format!("Index({index_name})"),
                ));
            }

            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    let index_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                    return Err(PlannerError::IndexColumnNotFound(
                        table_name.to_string(),
                        index_name,
                        col.to_string(),
                    ));
                }
            }
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }

    Ok(())
}
