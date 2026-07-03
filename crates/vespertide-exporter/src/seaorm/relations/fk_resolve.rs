//! Foreign-key chain resolution helpers.
//!
//! Resolves intermediate FK→FK chains so that a column pointing at a
//! pass-through table is rendered as a relation to the ultimate target.

use std::collections::{BTreeMap, BTreeSet};

use rayon::prelude::*;
use vespertide_core::{ColumnName, TableConstraint, TableDef};

use crate::parallel_config::{SEAORM_RELATION_PAR_FK_MIN_LEN, SEAORM_RELATION_PAR_FK_THRESHOLD};

/// Extract FK info from a constraint as a tuple.
fn as_fk(constraint: &TableConstraint) -> Option<(&[ColumnName], &str, &[ColumnName])> {
    match constraint {
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => Some((
            columns.as_slice(),
            ref_table.as_str(),
            ref_columns.as_slice(),
        )),
        _ => None,
    }
}

/// Resolve FK chain to find the ultimate target table.
/// If the referenced column is itself a FK, follow the chain.
#[cfg(test)]
pub(in crate::seaorm) fn resolve_fk_target<'a>(
    ref_table: &'a str,
    ref_columns: &[ColumnName],
    schema: &'a [TableDef],
) -> (&'a str, Vec<ColumnName>) {
    let table_map: BTreeMap<&str, &TableDef> =
        schema.iter().map(|t| (t.name.as_str(), t)).collect();
    let mut visited = BTreeSet::new();
    resolve_fk_target_inner(ref_table, ref_columns, &table_map, &mut visited)
}

fn resolve_fk_target_inner<'a, 'b>(
    ref_table: &'a str,
    ref_columns: &'b [ColumnName],
    table_map: &BTreeMap<&'a str, &'a TableDef>,
    visited: &mut BTreeSet<(&'a str, &'b str)>,
) -> (&'a str, Vec<ColumnName>)
where
    'a: 'b,
{
    if table_map.is_empty() || ref_columns.len() != 1 {
        return (ref_table, ref_columns.to_vec());
    }
    let ref_col = &ref_columns[0];
    let Some(target_table) = table_map.get(ref_table).copied() else {
        return (ref_table, ref_columns.to_vec());
    };
    for constraint in &target_table.constraints {
        let fk_match =
            as_fk(constraint).filter(|(cols, _, _)| cols.len() == 1 && cols[0] == ref_col.as_str());
        if let Some((_, next_table, next_cols)) = fk_match {
            visited.insert((ref_table, ref_col.as_str()));
            let next_key = (next_table, next_cols[0].as_str());
            if visited.contains(&next_key) {
                return (ref_table, ref_columns.to_vec());
            }
            return resolve_fk_target_inner(next_table, next_cols, table_map, visited);
        }
    }
    (ref_table, ref_columns.to_vec())
}

pub(super) struct ForwardRelationResolution<'a> {
    pub(super) columns: &'a [ColumnName],
    pub(super) resolved_table: &'a str,
    pub(super) resolved_columns: Vec<ColumnName>,
}

pub(super) fn resolve_table_fks_pure<'a>(
    table: &'a TableDef,
    schema: &'a [TableDef],
) -> Vec<ForwardRelationResolution<'a>> {
    let table_map: BTreeMap<&str, &TableDef> =
        schema.iter().map(|t| (t.name.as_str(), t)).collect();
    let fks = table
        .constraints
        .iter()
        .filter_map(as_fk)
        .collect::<Vec<_>>();
    if schema.len() < SEAORM_RELATION_PAR_FK_THRESHOLD {
        fks.iter()
            .map(|fk| resolve_fk_relation_pure(fk.0, fk.1, fk.2, &table_map))
            .collect()
    } else {
        fks.par_iter()
            .with_min_len(SEAORM_RELATION_PAR_FK_MIN_LEN)
            .map(|fk| resolve_fk_relation_pure(fk.0, fk.1, fk.2, &table_map))
            .collect()
    }
}

fn resolve_fk_relation_pure<'a>(
    columns: &'a [ColumnName],
    ref_table: &'a str,
    ref_columns: &'a [ColumnName],
    table_map: &BTreeMap<&'a str, &'a TableDef>,
) -> ForwardRelationResolution<'a> {
    let mut visited = BTreeSet::new();
    let (resolved_table, resolved_columns) =
        resolve_fk_target_inner(ref_table, ref_columns, table_map, &mut visited);
    ForwardRelationResolution {
        columns,
        resolved_table,
        resolved_columns,
    }
}
