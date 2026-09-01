use std::collections::{BTreeMap, HashSet};

use vespertide_core::{
    ColumnName, TableConstraint, TableDef,
    schema::names::{join_column_names, names_to_strings},
};

use crate::error::PlannerError;

pub(super) fn validate_foreign_key_constraint(
    table_name: &str,
    table_columns: &HashSet<&str>,
    table_map: &BTreeMap<&str, HashSet<&str>>,
    columns: &[ColumnName],
    ref_table: &str,
    ref_columns: &[ColumnName],
) -> Result<(), PlannerError> {
    if columns.is_empty() {
        return Err(PlannerError::EmptyConstraintColumns(
            table_name.to_string(),
            "ForeignKey".to_string(),
        ));
    }
    if ref_columns.is_empty() {
        return Err(PlannerError::EmptyConstraintColumns(
            ref_table.to_string(),
            "ForeignKey (ref_columns)".to_string(),
        ));
    }

    let ref_table_columns = table_map.get(ref_table).ok_or_else(|| {
        PlannerError::ForeignKeyTableNotFound(
            table_name.to_string(),
            columns.join(", "),
            ref_table.to_string(),
        )
    })?;

    for col in columns {
        if !table_columns.contains(col.as_str()) {
            return Err(PlannerError::ConstraintColumnNotFound(
                table_name.to_string(),
                "ForeignKey".to_string(),
                col.to_string(),
            ));
        }
    }

    for ref_col in ref_columns {
        if !ref_table_columns.contains(ref_col.as_str()) {
            return Err(PlannerError::ForeignKeyColumnNotFound(
                table_name.to_string(),
                columns.join(", "),
                ref_table.to_string(),
                ref_col.to_string(),
            ));
        }
    }

    if columns.len() != ref_columns.len() {
        return Err(PlannerError::ForeignKeyColumnNotFound(
            table_name.to_string(),
            format!(
                "column count mismatch: {} != {}",
                columns.len(),
                ref_columns.len()
            ),
            ref_table.to_string(),
            String::new(),
        ));
    }

    Ok(())
}

/// Describes a foreign-key constraint whose referencing (child) columns are
/// not covered by any leading-prefix index on the child table.
///
/// Without such an index, equality lookups triggered by FK enforcement —
/// cascade DELETE on the parent, parent UPDATE with a referential action,
/// or a JOIN through the FK column — degrade to full scans on the child
/// table. This is fault **F51** in the data-dependent migration fault
/// taxonomy: it never produces a SQL error, but silently regresses query
/// performance as the child table grows.
///
/// `PrimaryKey` and `Unique` constraints count as covering indexes because
/// every backend Vespertide supports materialises them as unique B-tree
/// indexes.
///
/// Returned by [`find_missing_fk_supporting_indexes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFkSupportingIndex {
    /// Child table that owns the foreign key.
    pub table: String,
    /// FK constraint name, if one was provided in the model.
    /// `None` for inline FK declarations that did not name the constraint.
    pub constraint_name: Option<String>,
    /// Referencing (child) columns the FK is defined over, in declared order.
    pub columns: Vec<String>,
    /// Parent (referenced) table.
    pub ref_table: String,
    /// Parent (referenced) columns, in declared order.
    pub ref_columns: Vec<String>,
    /// Suggested index name following Vespertide's `ix_{table}__{cols}` convention.
    pub suggested_index_name: String,
}

/// Scan a (normalised) schema for foreign-key constraints whose referencing
/// columns are not covered by any leading-prefix index on the child table.
///
/// An existing constraint *covers* an FK iff its column list begins with the
/// FK's column list in the same order. `PrimaryKey`, `Unique`, and `Index`
/// constraints all count as indexes for this purpose.
///
/// This is **purely static**: no database access, no row inspection. It only
/// reads structural information already present in `TableDef`s. Callers should
/// pass schemas that have already been normalised via
/// [`TableDef::normalize`](vespertide_core::TableDef::normalize) so that
/// inline column constraints have been promoted to table-level
/// `TableConstraint`s.
#[must_use]
pub fn find_missing_fk_supporting_indexes(schema: &[TableDef]) -> Vec<MissingFkSupportingIndex> {
    schema
        .iter()
        .flat_map(missing_fk_supporting_indexes_for_table)
        .collect()
}

fn missing_fk_supporting_indexes_for_table(table: &TableDef) -> Vec<MissingFkSupportingIndex> {
    // Collect the column prefix of every constraint that materialises as an
    // index on this table. Order within each slice is preserved; that is what
    // makes the prefix check below meaningful.
    let covering_prefixes: Vec<&[ColumnName]> = table
        .constraints
        .iter()
        .filter_map(|c| match c {
            TableConstraint::PrimaryKey { columns, .. }
            | TableConstraint::Unique { columns, .. }
            | TableConstraint::Index { columns, .. } => Some(columns.as_slice()),
            _ => None,
        })
        .collect();

    table
        .constraints
        .iter()
        .filter_map(|c| match c {
            TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                ..
            } if !columns.is_empty() && !has_covering_index(&covering_prefixes, columns) => {
                Some(MissingFkSupportingIndex {
                    table: table.name.to_string(),
                    constraint_name: name.clone(),
                    columns: names_to_strings(columns),
                    ref_table: ref_table.to_string(),
                    ref_columns: names_to_strings(ref_columns),
                    suggested_index_name: build_suggested_index_name(table.name.as_str(), columns),
                })
            }
            _ => None,
        })
        .collect()
}

fn has_covering_index(covering_prefixes: &[&[ColumnName]], fk_columns: &[ColumnName]) -> bool {
    covering_prefixes
        .iter()
        .any(|idx_cols| idx_cols.starts_with(fk_columns))
}

fn build_suggested_index_name(table: &str, columns: &[ColumnName]) -> String {
    let cols_joined = join_column_names(columns, "_");
    format!("ix_{table}__{cols_joined}")
}
