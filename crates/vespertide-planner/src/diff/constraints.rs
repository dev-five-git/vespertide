use std::collections::{BTreeMap, BTreeSet};

use vespertide_core::{MigrationAction, TableConstraint, TableDef};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ConstraintIdentityKey<'a> {
    PrimaryKey,
    ForeignKey {
        columns: Vec<&'a str>,
        ref_table: &'a str,
        ref_columns: Vec<&'a str>,
    },
    Check {
        name: &'a str,
    },
    Unique {
        columns: Vec<&'a str>,
    },
    Index {
        columns: Vec<&'a str>,
    },
}

fn sorted_column_refs<T: AsRef<str>>(columns: &[T]) -> Vec<&str> {
    let mut columns: Vec<&str> = columns.iter().map(AsRef::as_ref).collect();
    columns.sort_unstable();
    columns
}

fn constraint_identity_key(constraint: &TableConstraint) -> ConstraintIdentityKey<'_> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => ConstraintIdentityKey::PrimaryKey,
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => ConstraintIdentityKey::ForeignKey {
            columns: sorted_column_refs(columns),
            ref_table,
            ref_columns: sorted_column_refs(ref_columns),
        },
        TableConstraint::Check { name, .. } => ConstraintIdentityKey::Check { name },
        TableConstraint::Unique { columns, .. } => ConstraintIdentityKey::Unique {
            columns: sorted_column_refs(columns),
        },
        TableConstraint::Index { columns, .. } => ConstraintIdentityKey::Index {
            columns: sorted_column_refs(columns),
        },
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

pub(super) fn diff_constraints(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_tbl: &TableDef,
    to_tbl: &TableDef,
    deleted_columns: &BTreeSet<String>,
) {
    let mut replaced_from: Vec<usize> = Vec::new();
    let mut replaced_to: Vec<usize> = Vec::new();

    diff_replaced_constraints(
        actions,
        table_name,
        from_tbl,
        to_tbl,
        &mut replaced_from,
        &mut replaced_to,
    );
    diff_removed_constraints(
        actions,
        table_name,
        from_tbl,
        to_tbl,
        deleted_columns,
        &replaced_from,
    );
    diff_added_constraints(actions, table_name, from_tbl, to_tbl, &replaced_to);
}

fn diff_replaced_constraints(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_tbl: &TableDef,
    to_tbl: &TableDef,
    replaced_from: &mut Vec<usize>,
    replaced_to: &mut Vec<usize>,
) {
    let exact_from: BTreeSet<&TableConstraint> = from_tbl.constraints.iter().collect();
    let exact_to: BTreeSet<&TableConstraint> = to_tbl.constraints.iter().collect();
    let mut to_index: BTreeMap<ConstraintIdentityKey<'_>, Vec<usize>> = BTreeMap::new();

    for (ti, to_constraint) in to_tbl.constraints.iter().enumerate() {
        if !exact_from.contains(to_constraint) {
            to_index
                .entry(constraint_identity_key(to_constraint))
                .or_default()
                .push(ti);
        }
    }

    let mut used_to: BTreeSet<usize> = BTreeSet::new();

    for (fi, from_constraint) in from_tbl.constraints.iter().enumerate() {
        if exact_to.contains(from_constraint) {
            continue;
        }
        let key = constraint_identity_key(from_constraint);
        let Some(candidates) = to_index.get(&key) else {
            continue;
        };
        if let Some(&ti) = candidates.iter().find(|&&ti| !used_to.contains(&ti)) {
            used_to.insert(ti);
            replaced_from.push(fi);
            replaced_to.push(ti);
            actions.push(MigrationAction::ReplaceConstraint {
                table: table_name.to_string().into(),
                from: from_constraint.clone(),
                to: to_tbl.constraints[ti].clone(),
            });
        }
    }
}

fn diff_removed_constraints(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_tbl: &TableDef,
    to_tbl: &TableDef,
    deleted_columns: &BTreeSet<String>,
    replaced_from: &[usize],
) {
    for (fi, from_constraint) in from_tbl.constraints.iter().enumerate() {
        if to_tbl.constraints.contains(from_constraint) || replaced_from.contains(&fi) {
            continue;
        }
        let constraint_columns = from_constraint.columns();
        let all_columns_deleted = !constraint_columns.is_empty()
            && constraint_columns
                .iter()
                .all(|col| deleted_columns.contains(col.as_str()));

        if !all_columns_deleted {
            actions.push(MigrationAction::RemoveConstraint {
                table: table_name.to_string().into(),
                constraint: from_constraint.clone(),
            });
        }
    }
}

fn diff_added_constraints(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_tbl: &TableDef,
    to_tbl: &TableDef,
    replaced_to: &[usize],
) {
    for (ti, to_constraint) in to_tbl.constraints.iter().enumerate() {
        if from_tbl.constraints.contains(to_constraint) || replaced_to.contains(&ti) {
            continue;
        }
        actions.push(MigrationAction::AddConstraint {
            table: table_name.to_string().into(),
            constraint: to_constraint.clone(),
        });
    }
}
