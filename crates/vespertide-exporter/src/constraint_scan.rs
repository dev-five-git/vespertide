//! Shared constraint-scan helpers used by the ORM renderers.
//!
//! Every backend needs the same lookup sets when rendering a table:
//! the columns covered by table-level primary keys, the columns that
//! carry a single-column unique constraint, and the columns that carry
//! a single-column index. Centralising the scans keeps the four
//! renderers from drifting apart.

use std::collections::{HashMap, HashSet};

use vespertide_core::{ColumnName, TableConstraint};

/// Collect the column names from every single-column constraint that `extract`
/// matches. Shared body for [`single_column_uniques`] and
/// [`single_column_indexes`], which differ only in the matched
/// `TableConstraint` variant. `extract` returns the constraint's column slice
/// for the variant it cares about, or `None` for every other variant.
fn single_column_scan<'a>(
    constraints: &'a [TableConstraint],
    extract: impl Fn(&'a TableConstraint) -> Option<&'a [ColumnName]>,
) -> HashSet<&'a str> {
    let mut cols = HashSet::new();
    for constraint in constraints {
        if let Some(columns) = extract(constraint)
            && columns.len() == 1
        {
            cols.insert(columns[0].as_str());
        }
    }
    cols
}

/// Collect the column names covered by table-level `PrimaryKey` constraints.
///
/// Lookup-only, ordering unused.
pub(crate) fn primary_key_columns(constraints: &[TableConstraint]) -> HashSet<&str> {
    let mut keys = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            for col in columns {
                keys.insert(col.as_str());
            }
        }
    }
    keys
}

/// Collect the column names that carry a single-column `Unique` constraint.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_uniques(constraints: &[TableConstraint]) -> HashSet<&str> {
    single_column_scan(constraints, |c| match c {
        TableConstraint::Unique { columns, .. } => Some(columns.as_slice()),
        _ => None,
    })
}

/// Collect the column names that carry a single-column `Index` constraint.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_indexes(constraints: &[TableConstraint]) -> HashSet<&str> {
    single_column_scan(constraints, |c| match c {
        TableConstraint::Index { columns, .. } => Some(columns.as_slice()),
        _ => None,
    })
}

/// Map each single-column foreign key's column name to its
/// `(ref_table, ref_col)` target. Only foreign keys with exactly one owning
/// column and one referenced column are included; composite FKs are skipped.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_fk_targets(
    constraints: &[TableConstraint],
) -> HashMap<&str, (&str, &str)> {
    let mut map = HashMap::new();
    for constraint in constraints {
        if let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } = constraint
            && columns.len() == 1
            && ref_columns.len() == 1
        {
            map.insert(
                columns[0].as_str(),
                (ref_table.as_str(), ref_columns[0].as_str()),
            );
        }
    }
    map
}
