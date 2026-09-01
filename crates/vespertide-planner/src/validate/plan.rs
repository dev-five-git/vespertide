use rayon::prelude::*;
use vespertide_core::{
    ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan, TableConstraint,
    TableDef, action::sql_preview,
};

use super::enums::validate_enum_value;
use crate::error::{MultipleErrors, PlannerError};
use crate::parallel_config::{VALIDATE_PLAN_PAR_ACTION_MIN_LEN, validate_plan_par_threshold};

/// Validate a migration plan for correctness.
///
/// Returns `Ok(())` when every action is valid. On failure the returned error
/// follows this contract so existing single-violation tests stay byte-identical
/// while batch callers see every problem in one shot:
///
/// - exactly **1** violation → that violation's bare [`PlannerError`] variant,
/// - **2 or more** violations → wrapped in [`PlannerError::Multiple`] with all
///   violations preserved in action-index order.
///
/// Checks for:
/// - `AddColumn` actions with NOT NULL columns without default must have `fill_with`
/// - `ModifyColumnNullable` actions changing from nullable to non-nullable must have `fill_with`
/// - Enum columns with `default/fill_with` values must have valid enum values
/// - `DataMigration` actions must not carry DDL (see [`PlannerError::DataMigrationContainsDdl`])
pub fn validate_migration_plan(plan: &MigrationPlan) -> Result<(), PlannerError> {
    let mut violations = find_plan_violations(plan);
    match violations.len() {
        0 => Ok(()),
        1 => Err(violations.remove(0)),
        _ => Err(PlannerError::Multiple(Box::new(MultipleErrors(violations)))),
    }
}

/// Collect every plan-level violation in one pass.
///
/// Returned violations are sorted by action index so the order matches the
/// historical `validate_migration_plan` first-fail behaviour: index 0 of the
/// returned vec is the same error `validate_migration_plan` would have
/// produced under the old first-fail contract.
///
/// Prefer this over [`validate_migration_plan`] when surfacing **all**
/// violations to the user (CLI batch error message, LSP diagnostics, etc.).
#[must_use]
pub fn find_plan_violations(plan: &MigrationPlan) -> Vec<PlannerError> {
    let mut indexed: Vec<(usize, PlannerError)> =
        if plan.actions.len() < validate_plan_par_threshold() {
            plan.actions
                .iter()
                .enumerate()
                .filter_map(|(idx, action)| validate_action(action).err().map(|err| (idx, err)))
                .collect()
        } else {
            plan.actions
                .par_iter()
                .enumerate()
                .with_min_len(VALIDATE_PLAN_PAR_ACTION_MIN_LEN)
                .filter_map(|(idx, action)| validate_action(action).err().map(|err| (idx, err)))
                .collect()
        };

    indexed.sort_by_key(|(idx, _)| *idx);
    indexed.into_iter().map(|(_, err)| err).collect()
}

fn validate_action(action: &MigrationAction) -> Result<(), PlannerError> {
    match action {
        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => {
            // If column is NOT NULL and has no default, fill_with is required
            if !column.nullable && column.default.is_none() && fill_with.is_none() {
                return Err(PlannerError::MissingFillWith(
                    table.to_string(),
                    column.name.to_string(),
                ));
            }

            // Validate enum default/fill_with values
            if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &column.r#type {
                if let Some(fill) = fill_with {
                    validate_enum_value(fill, name, values, table, &column.name, "fill_with")?;
                }
                if let Some(default) = &column.default {
                    let default_str = default.to_sql();
                    validate_enum_value(
                        &default_str,
                        name,
                        values,
                        table,
                        &column.name,
                        "default",
                    )?;
                }
            }
        }
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with,
            delete_null_rows,
        }
            // If changing from nullable to non-nullable, fill_with is required
            if !nullable && fill_with.is_none() && !delete_null_rows.unwrap_or(false) =>
        {
            return Err(PlannerError::MissingFillWith(
                table.to_string(),
                column.to_string(),
            ));
        }
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            fill_with,
            // `narrowing_strategy` is validated separately by
            // `find_type_narrowings`; nothing to enum-check here.
            ..
        } => {
            // Validate that fill_with replacement values are valid enum values in the NEW type
            if let (
                Some(fw),
                ColumnType::Complex(ComplexColumnType::Enum { name, values, .. }),
            ) = (fill_with, new_type)
            {
                for replacement in fw.values() {
                    validate_enum_value(replacement, name, values, table, column, "fill_with")?;
                }
            }
        }
        MigrationAction::DataMigration { .. } => {
            if let Some((keyword, statement)) = action.data_migration_ddl_violation() {
                return Err(PlannerError::DataMigrationContainsDdl {
                    keyword,
                    statement: sql_preview(statement),
                });
            }
        }
        _ => {}
    }

    Ok(())
}

/// Describes an action whose `fill_with` is required but missing.
/// Returned by [`find_missing_fill_with`] so callers can prompt the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillWithRequired {
    /// Index of the action in the migration plan.
    pub action_index: usize,
    /// Table name.
    pub table: String,
    /// Column name.
    pub column: String,
    /// Type of action: "`AddColumn`" or "`ModifyColumnNullable`".
    pub action_type: &'static str,
    /// Column type (for display purposes).
    pub column_type: String,
    /// Default fill value hint for this column type.
    pub default_value: String,
    /// Enum values if the column is an enum type (for selection UI).
    pub enum_values: Option<Vec<String>>,
    /// Whether the current column has a foreign key constraint.
    pub has_foreign_key: bool,
}

/// Find `AddColumn` / `ModifyColumnNullable` actions that need a `fill_with`
/// value because they introduce NOT NULL on a column without a DB default.
pub fn find_missing_fill_with(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Vec<FillWithRequired> {
    if plan.actions.len() < validate_plan_par_threshold() {
        plan.actions
            .iter()
            .enumerate()
            .filter_map(|(idx, action)| missing_fill_with_for_action(idx, action, current_schema))
            .collect()
    } else {
        let mut missing: Vec<_> = plan
            .actions
            .par_iter()
            .enumerate()
            .with_min_len(VALIDATE_PLAN_PAR_ACTION_MIN_LEN)
            .filter_map(|(idx, action)| missing_fill_with_for_action(idx, action, current_schema))
            .collect();
        missing.sort_by_key(|item| item.action_index);
        missing
    }
}

fn missing_fill_with_for_action(
    idx: usize,
    action: &MigrationAction,
    current_schema: &[TableDef],
) -> Option<FillWithRequired> {
    match action {
        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        }
            // If column is NOT NULL and has no default, fill_with is required
            if !column.nullable && column.default.is_none() && fill_with.is_none() =>
        {
            Some(FillWithRequired {
                action_index: idx,
                table: table.to_string(),
                column: column.name.to_string(),
                action_type: "AddColumn",
                column_type: column.r#type.to_display_string(),
                default_value: column.r#type.default_fill_value().to_string(),
                enum_values: column.r#type.enum_variant_names(),
                has_foreign_key: false,
            })
        }
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with,
            delete_null_rows,
        }
            // If changing from nullable to non-nullable, fill_with is required
            // UNLESS the column already has a default value (which will be used)
            if !nullable && fill_with.is_none() && !delete_null_rows.unwrap_or(false) =>
        {
            // Look up column from the current schema
            let table_def = current_schema.iter().find(|t| t.name == *table);

            let col_def = table_def.and_then(|t| t.columns.iter().find(|c| c.name == *column));

            let has_foreign_key = table_def.is_some_and(|t| t.constraints.iter().any(|constraint| matches!(constraint, TableConstraint::ForeignKey { columns, .. } if columns.iter().any(|col_name| col_name == column))));

            // If column has a default value, fill_with is not needed
            if col_def.is_some_and(|c| c.default.is_some()) {
                return None;
            }

            let (col_type_str, default_val, enum_vals) = match col_def {
                Some(c) => (
                    c.r#type.to_display_string(),
                    c.r#type.default_fill_value().to_string(),
                    c.r#type.enum_variant_names(),
                ),
                None => (column.to_string(), "''".to_string(), None),
            };

            Some(FillWithRequired {
                action_index: idx,
                table: table.to_string(),
                column: column.to_string(),
                action_type: "ModifyColumnNullable",
                column_type: col_type_str,
                default_value: default_val,
                enum_values: enum_vals,
                has_foreign_key,
            })
        }
        _ => None,
    }
}

/// Describes an enum-narrowing action whose `fill_with` is required but missing.
/// Returned by [`find_missing_enum_fill_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumFillWithRequired {
    /// Index of the action in the migration plan.
    pub action_index: usize,
    /// Table name.
    pub table: String,
    /// Column name.
    pub column: String,
    /// Removed enum values that need replacement mappings.
    pub removed_values: Vec<String>,
    /// Remaining valid enum values (for selection UI).
    pub remaining_values: Vec<String>,
}

/// Find `ModifyColumnType` actions that remove enum values, requiring a
/// `fill_with` to substitute for rows still using the removed value.
pub fn find_missing_enum_fill_with(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Vec<EnumFillWithRequired> {
    if plan.actions.len() < validate_plan_par_threshold() {
        plan.actions
            .iter()
            .enumerate()
            .filter_map(|(idx, action)| {
                missing_enum_fill_with_for_action(idx, action, current_schema)
            })
            .collect()
    } else {
        let mut missing: Vec<_> = plan
            .actions
            .par_iter()
            .enumerate()
            .with_min_len(VALIDATE_PLAN_PAR_ACTION_MIN_LEN)
            .filter_map(|(idx, action)| {
                missing_enum_fill_with_for_action(idx, action, current_schema)
            })
            .collect();
        missing.sort_by_key(|item| item.action_index);
        missing
    }
}

fn missing_enum_fill_with_for_action(
    idx: usize,
    action: &MigrationAction,
    current_schema: &[TableDef],
) -> Option<EnumFillWithRequired> {
    let MigrationAction::ModifyColumnType {
        table,
        column,
        new_type,
        fill_with,
        ..
    } = action
    else {
        return None;
    };

    // Only applies to string enum → string enum changes
    let old_type = current_schema
        .iter()
        .find(|t| t.name == *table)
        .and_then(|t| t.columns.iter().find(|c| c.name == *column))
        .map(|c| &c.r#type);

    let (
        Some(ColumnType::Complex(ComplexColumnType::Enum {
            values: EnumValues::String(old_values),
            ..
        })),
        ColumnType::Complex(ComplexColumnType::Enum {
            values: EnumValues::String(new_values),
            ..
        }),
    ) = (old_type, new_type)
    else {
        return None;
    };

    // Find removed values (in old but not in new)
    let removed: Vec<String> = old_values
        .iter()
        .filter(|v| !new_values.contains(v))
        .cloned()
        .collect();

    if removed.is_empty() {
        return None;
    }

    // Check if fill_with covers all removed values
    let all_covered = match fill_with {
        Some(fw) => removed.iter().all(|r| fw.contains_key(r)),
        None => false,
    };

    if all_covered {
        return None;
    }

    // Filter to only uncovered removed values
    let uncovered: Vec<String> = match fill_with {
        Some(fw) => removed
            .into_iter()
            .filter(|r| !fw.contains_key(r))
            .collect(),
        None => removed,
    };

    Some(EnumFillWithRequired {
        action_index: idx,
        table: table.to_string(),
        column: column.to_string(),
        removed_values: uncovered,
        remaining_values: new_values.clone(),
    })
}
