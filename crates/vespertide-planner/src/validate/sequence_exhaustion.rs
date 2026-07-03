//! Fault **F76** - sequence / identity overflow risk on primary key
//! columns.
//!
//! Single-column auto-increment primary keys typed `INTEGER` (32-bit
//! signed, max ~2.1 billion) or `SMALLINT` (16-bit, max 32,767) will
//! eventually exhaust the underlying sequence. At 1 M new rows/day an
//! `INTEGER` PK exhausts in ~5.9 years; at 10 M/day in ~7 months.
//! Once exhaustion happens every subsequent `INSERT` fails with
//! `integer out of range` and the only fix is a multi-hour
//! `ALTER TABLE ... TYPE bigint` rewrite under `ACCESS EXCLUSIVE`.
//!
//! Vespertide statically catches three shapes of this risk:
//!
//! - **Primary** - a plan introduces (via `CreateTable` or
//!   `AddConstraint(PrimaryKey)`) a *new* single-column auto-increment
//!   PK whose column type is `SmallInt` or `Integer`.
//! - **`PkTypeNarrowing`** - a `ModifyColumnType` action narrows an
//!   *existing* PK column from `BigInt` down to `Integer` or
//!   `SmallInt`, immediately exposing the column to the overflow risk.
//!   This dimension is reported in addition to (not in place of) the
//!   F6 type-narrowing prompt; both warnings surface because the F6
//!   prompt handles silent truncation and F76 handles long-term
//!   overflow - independent concerns the user should both consider.
//! - **`ForeignKeyMismatch`** - a plan adds a foreign key whose child
//!   column type is *narrower* than the parent PK's type. The child
//!   column will overflow whenever the parent's id space exceeds the
//!   child's range, which silently breaks the FK invariant. The
//!   warning recommends widening the child to match the parent.
//!
//! Specifically suppressed (never reported):
//!
//! - Composite primary keys - per-column overflow is meaningless for
//!   a multi-column identity.
//! - Non-PK auto-increment columns - they can be exotic (e.g. an
//!   `external_seq` column populated by a trigger) and vespertide
//!   does not have enough context to classify them.
//! - `BigInt` / `Uuid` / other PK types - 64-bit and 128-bit
//!   identity spaces are safe.
//! - Baseline PKs that the plan does not touch - the user has
//!   already lived with that choice; F76 only flags *new* exposure.
//!
//! All warnings include the recommended type (`BigInt`) so the CLI
//! prompt can offer a single-click "rewrite to `BigInt`" mutation.

use std::collections::{HashMap, HashSet};

use vespertide_core::{
    ColumnDef, ColumnName, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType,
    TableConstraint, TableDef,
};

/// Shape of the overflow risk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceExhaustionKind {
    /// A new single-column auto-increment PK whose column type is at
    /// risk of overflow (`SmallInt` or `Integer`).
    Primary,
    /// A `ModifyColumnType` narrows an existing PK column from a safe
    /// `BigInt` down to a risky `Integer` / `SmallInt`.
    PkTypeNarrowing {
        /// Baseline type the column was narrowed *from*. Always
        /// `SimpleColumnType::BigInt` in v0.2 (other safe-to-risky
        /// transitions are not yet detected).
        from: SimpleColumnType,
    },
    /// A new FK whose child column is narrower than the parent PK.
    /// The child will overflow before the parent exhausts.
    ForeignKeyMismatch {
        /// Parent (referenced) table.
        parent_table: String,
        /// Parent PK column type.
        parent_type: SimpleColumnType,
    },
}

/// Risk classifier for F76 warnings. Tied directly to the underlying
/// integer width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceRiskLevel {
    /// `SmallInt` PK - 16-bit, exhausts in hours to days at any
    /// realistic write rate.
    High,
    /// `Integer` PK - 32-bit, exhausts in months to years at typical
    /// production traffic.
    Medium,
}

/// One overflow-risk site needing user resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceExhaustionWarning {
    /// Plan-action index of the triggering action (`CreateTable`,
    /// `AddConstraint`, `ModifyColumnType`, ...).
    pub action_index: usize,
    /// Table the warned column lives in.
    pub table: String,
    /// Column at risk.
    pub column: String,
    /// Current (or post-plan) column type - one of `SmallInt`,
    /// `Integer`. Never `BigInt` (`BigInt` is the recommended fix).
    pub current_type: SimpleColumnType,
    /// Recommended replacement type. Always `BigInt` in v0.2.
    pub recommended_type: SimpleColumnType,
    /// Risk classifier (`High` for `SmallInt`, `Medium` for
    /// `Integer`).
    pub risk_level: SequenceRiskLevel,
    /// Which dimension of risk fired (PK, narrowing, FK mismatch).
    pub kind: SequenceExhaustionKind,
}

/// Scan the plan for new overflow-risk sites against the baseline.
///
/// Returns warnings in plan-order. Empty when the plan introduces no
/// risky single-column auto-increment PKs, PK type narrowings, or FK
/// mismatches against safe parent types.
#[expect(
    clippy::too_many_lines,
    reason = "single dispatch over four MigrationAction variants (CreateTable / AddConstraint(PrimaryKey) / AddConstraint(ForeignKey) / ModifyColumnType); extracting each arm into a helper would scatter the shared baseline + pk_type_map context across multiple call sites without aiding readability"
)]
#[must_use]
pub fn find_sequence_exhaustion_risks(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<SequenceExhaustionWarning> {
    let mut out = Vec::new();

    // Pre-compute baseline PK exposure so we can suppress already-
    // exposed cases (`D8` - only flag *new* exposure).
    let baseline_existing_risky_pk: HashSet<(String, String)> = baseline
        .iter()
        .flat_map(|t| {
            risky_single_pk_columns(t)
                .into_iter()
                .map(move |(c, _)| (t.name.to_string(), c))
        })
        .collect();

    // Lookup of baseline (and plan-future) PK column types - needed
    // for the FK-mismatch check. We merge baseline PKs with PKs being
    // added in this plan so a same-plan
    // `CreateTable(parent) + AddConstraint(FK)` chain is analysed
    // correctly.
    let mut pk_type_map: HashMap<String, SimpleColumnType> = HashMap::new();
    for t in baseline {
        if let Some((_col, ty)) = single_pk_column_with_type(t) {
            pk_type_map.insert(t.name.to_string(), ty);
        }
    }
    for action in &plan.actions {
        if let MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } = action
            && let Some(ty) = single_pk_type_from_create_table(columns, constraints)
        {
            pk_type_map.insert(table.to_string(), ty);
        }
    }

    for (idx, action) in plan.actions.iter().enumerate() {
        match action {
            // CreateTable with inline single-column auto-increment PK.
            MigrationAction::CreateTable {
                table,
                columns,
                constraints,
            } => {
                if let Some((col_name, col_type)) =
                    single_pk_with_auto_increment(columns, constraints)
                    && let Some((current, risk)) = classify_risky_int_type(col_type)
                {
                    out.push(SequenceExhaustionWarning {
                        action_index: idx,
                        table: table.to_string(),
                        column: col_name,
                        current_type: current,
                        recommended_type: SimpleColumnType::BigInt,
                        risk_level: risk,
                        kind: SequenceExhaustionKind::Primary,
                    });
                }
                // Also scan inline FKs in the new table for parent
                // mismatch.
                for col in columns {
                    if let Some(parent_table) = inline_fk_parent_table(col)
                        && let Some(parent_ty) = pk_type_map.get(&parent_table)
                        && let Some(child_ty) = simple_int_type_of(&col.r#type)
                        && is_narrower_than(child_ty, *parent_ty)
                        && let Some((current, risk)) = classify_risky_int_type(child_ty)
                    {
                        out.push(SequenceExhaustionWarning {
                            action_index: idx,
                            table: table.to_string(),
                            column: col.name.to_string(),
                            current_type: current,
                            recommended_type: SimpleColumnType::BigInt,
                            risk_level: risk,
                            kind: SequenceExhaustionKind::ForeignKeyMismatch {
                                parent_table,
                                parent_type: *parent_ty,
                            },
                        });
                    }
                }
            }
            // AddConstraint(PrimaryKey) targeting a single-column PK
            // with `auto_increment: true`.
            MigrationAction::AddConstraint {
                table,
                constraint:
                    TableConstraint::PrimaryKey {
                        auto_increment: true,
                        columns,
                        ..
                    },
            } if columns.len() == 1 => {
                let col_name = columns[0].as_str();
                // Suppress when the baseline already exposes the same
                // shape - the user has already lived with it.
                if baseline_existing_risky_pk.contains(&(table.to_string(), col_name.to_string())) {
                    continue;
                }
                // Resolve the column type from baseline (column must
                // exist for AddConstraint to be meaningful).
                let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str())
                else {
                    continue;
                };
                let Some(col) = table_def
                    .columns
                    .iter()
                    .find(|c| c.name.as_str() == col_name)
                else {
                    continue;
                };
                let Some(col_ty) = simple_int_type_of(&col.r#type) else {
                    continue;
                };
                if let Some((current, risk)) = classify_risky_int_type(col_ty) {
                    out.push(SequenceExhaustionWarning {
                        action_index: idx,
                        table: table.to_string(),
                        column: col_name.to_string(),
                        current_type: current,
                        recommended_type: SimpleColumnType::BigInt,
                        risk_level: risk,
                        kind: SequenceExhaustionKind::Primary,
                    });
                }
            }
            // AddConstraint(ForeignKey) with parent-mismatch check.
            MigrationAction::AddConstraint {
                table,
                constraint:
                    TableConstraint::ForeignKey {
                        columns, ref_table, ..
                    },
            } => {
                if columns.len() != 1 {
                    continue;
                }
                let col_name = columns[0].as_str();
                let Some(parent_ty) = pk_type_map.get(ref_table.as_str()) else {
                    continue;
                };
                let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str())
                else {
                    continue;
                };
                let Some(col) = table_def
                    .columns
                    .iter()
                    .find(|c| c.name.as_str() == col_name)
                else {
                    continue;
                };
                let Some(child_ty) = simple_int_type_of(&col.r#type) else {
                    continue;
                };
                if !is_narrower_than(child_ty, *parent_ty) {
                    continue;
                }
                if let Some((current, risk)) = classify_risky_int_type(child_ty) {
                    out.push(SequenceExhaustionWarning {
                        action_index: idx,
                        table: table.to_string(),
                        column: col_name.to_string(),
                        current_type: current,
                        recommended_type: SimpleColumnType::BigInt,
                        risk_level: risk,
                        kind: SequenceExhaustionKind::ForeignKeyMismatch {
                            parent_table: ref_table.to_string(),
                            parent_type: *parent_ty,
                        },
                    });
                }
            }
            // ModifyColumnType narrowing a PK column from BigInt to
            // a risky width. `from` is resolved against the baseline
            // (the action only carries `new_type`).
            MigrationAction::ModifyColumnType {
                table,
                column,
                new_type,
                ..
            } => {
                let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str())
                else {
                    continue;
                };
                let Some(col) = table_def
                    .columns
                    .iter()
                    .find(|c| c.name.as_str() == column.as_str())
                else {
                    continue;
                };
                let Some(from_ty) = simple_int_type_of(&col.r#type) else {
                    continue;
                };
                let Some(to_ty) = simple_int_type_of(new_type) else {
                    continue;
                };
                if from_ty != SimpleColumnType::BigInt || !is_risky_int_type(to_ty) {
                    continue;
                }
                // PK membership check (single-column PK only).
                if !is_single_pk_column(table_def, column.as_str()) {
                    continue;
                }
                if let Some((current, risk)) = classify_risky_int_type(to_ty) {
                    out.push(SequenceExhaustionWarning {
                        action_index: idx,
                        table: table.to_string(),
                        column: column.to_string(),
                        current_type: current,
                        recommended_type: SimpleColumnType::BigInt,
                        risk_level: risk,
                        kind: SequenceExhaustionKind::PkTypeNarrowing {
                            from: SimpleColumnType::BigInt,
                        },
                    });
                }
            }
            _ => {}
        }
    }

    out
}

/// Returns `(column_name, type)` when `table` has a single-column PK
/// (table-level or inline) using a risky integer width.
fn risky_single_pk_columns(table: &TableDef) -> Vec<(String, SimpleColumnType)> {
    let Some((col_name, ty)) = single_pk_column_with_type(table) else {
        return vec![];
    };
    if is_risky_int_type(ty) {
        vec![(col_name, ty)]
    } else {
        vec![]
    }
}

/// Look up the single-column PK (if any) on a `TableDef` and return
/// `(column_name, column_type)`. Returns `None` for composite or
/// missing PKs.
fn single_pk_column_with_type(table: &TableDef) -> Option<(String, SimpleColumnType)> {
    // Table-level PK.
    let table_level: Option<Vec<ColumnName>> = table.constraints.iter().find_map(|c| match c {
        TableConstraint::PrimaryKey { columns, .. } => Some(columns.clone()),
        _ => None,
    });
    let pk_columns: Vec<ColumnName> = if let Some(cols) = table_level {
        cols
    } else {
        // Inline PK fallback.
        table
            .columns
            .iter()
            .filter(|c| c.primary_key.is_some())
            .map(|c| c.name.clone())
            .collect()
    };
    if pk_columns.len() != 1 {
        return None;
    }
    let col_name = pk_columns[0].as_str();
    let col = table.columns.iter().find(|c| c.name.as_str() == col_name)?;
    let ty = simple_int_type_of(&col.r#type)?;
    Some((col_name.to_string(), ty))
}

/// Extract the single-PK column type from a `CreateTable` action's
/// `(columns, constraints)` pair. Returns `None` for composite or
/// missing PKs.
fn single_pk_type_from_create_table(
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Option<SimpleColumnType> {
    let table_level: Option<Vec<ColumnName>> = constraints.iter().find_map(|c| match c {
        TableConstraint::PrimaryKey { columns, .. } => Some(columns.clone()),
        _ => None,
    });
    let pk_columns: Vec<ColumnName> = if let Some(cols) = table_level {
        cols
    } else {
        columns
            .iter()
            .filter(|c| c.primary_key.is_some())
            .map(|c| c.name.clone())
            .collect()
    };
    if pk_columns.len() != 1 {
        return None;
    }
    let col_name = pk_columns[0].as_str();
    let col = columns.iter().find(|c| c.name.as_str() == col_name)?;
    simple_int_type_of(&col.r#type)
}

/// Extract `(pk_column_name, pk_column_type)` from a `CreateTable`
/// when the PK is single-column **and** marked `auto_increment`.
fn single_pk_with_auto_increment(
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Option<(String, SimpleColumnType)> {
    let pk = constraints.iter().find_map(|c| match c {
        TableConstraint::PrimaryKey {
            auto_increment: true,
            columns,
            ..
        } if columns.len() == 1 => Some(columns[0].clone()),
        _ => None,
    });
    if let Some(col_name) = pk {
        let col = columns
            .iter()
            .find(|c| c.name.as_str() == col_name.as_str())?;
        let ty = simple_int_type_of(&col.r#type)?;
        return Some((col_name.to_string(), ty));
    }
    // Inline PK with auto_increment: detection from
    // `PrimaryKeySyntax::Object { auto_increment: true }` is currently
    // not represented uniformly in the `ColumnDef.primary_key`
    // setter chain. v0.2 covers the table-level case only; inline
    // auto-increment PKs route through the normalised form in
    // practice.
    None
}

/// Return the `Simple(int)` type if the column carries a *single-
/// table* inline FK to another table; output is `parent_table` (the
/// referenced table). Composite inline FKs are out of scope (v0.2).
fn inline_fk_parent_table(col: &ColumnDef) -> Option<String> {
    use vespertide_core::schema::foreign_key::ForeignKeySyntax;
    let fk = col.foreign_key.as_ref()?;
    match fk {
        ForeignKeySyntax::String(s) => {
            // "parent.column" -> parent
            s.split_once('.').map(|(t, _)| t.to_string())
        }
        ForeignKeySyntax::Reference(r) => r.references.split_once('.').map(|(t, _)| t.to_string()),
        ForeignKeySyntax::Object(o) => Some(o.ref_table.to_string()),
    }
}

/// True when the column with `column_name` is the **only** column of
/// the PK on `table_def`.
fn is_single_pk_column(table_def: &TableDef, column_name: &str) -> bool {
    let table_level: Option<Vec<ColumnName>> = table_def.constraints.iter().find_map(|c| match c {
        TableConstraint::PrimaryKey { columns, .. } => Some(columns.clone()),
        _ => None,
    });
    let pk_cols: Vec<ColumnName> = if let Some(cols) = table_level {
        cols
    } else {
        table_def
            .columns
            .iter()
            .filter(|c| c.primary_key.is_some())
            .map(|c| c.name.clone())
            .collect()
    };
    pk_cols.len() == 1 && pk_cols[0].as_str() == column_name
}

/// Project a [`ColumnType`] down to its simple integer flavour, or
/// `None` for non-integer types.
fn simple_int_type_of(ty: &ColumnType) -> Option<SimpleColumnType> {
    match ty {
        ColumnType::Simple(SimpleColumnType::SmallInt) => Some(SimpleColumnType::SmallInt),
        ColumnType::Simple(SimpleColumnType::Integer) => Some(SimpleColumnType::Integer),
        ColumnType::Simple(SimpleColumnType::BigInt) => Some(SimpleColumnType::BigInt),
        _ => None,
    }
}

/// True when `ty` is one of the risky widths (`SmallInt` / `Integer`).
fn is_risky_int_type(ty: SimpleColumnType) -> bool {
    matches!(ty, SimpleColumnType::SmallInt | SimpleColumnType::Integer)
}

/// Returns `(canonical_type, risk_level)` when the type is risky,
/// otherwise `None`.
fn classify_risky_int_type(ty: SimpleColumnType) -> Option<(SimpleColumnType, SequenceRiskLevel)> {
    match ty {
        SimpleColumnType::SmallInt => Some((SimpleColumnType::SmallInt, SequenceRiskLevel::High)),
        SimpleColumnType::Integer => Some((SimpleColumnType::Integer, SequenceRiskLevel::Medium)),
        _ => None,
    }
}

/// True when `child` is narrower than `parent` (`SmallInt` < `Integer`
/// < `BigInt`). Only the strict-less direction is meaningful for the
/// FK-mismatch warning.
fn is_narrower_than(child: SimpleColumnType, parent: SimpleColumnType) -> bool {
    fn rank(t: SimpleColumnType) -> u8 {
        match t {
            SimpleColumnType::SmallInt => 0,
            SimpleColumnType::Integer => 1,
            SimpleColumnType::BigInt => 2,
            _ => 3,
        }
    }
    rank(child) < rank(parent)
}

#[cfg(test)]
mod tests;
