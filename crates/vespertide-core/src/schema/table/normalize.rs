use std::collections::{BTreeMap, BTreeSet};

use crate::schema::{
    ColumnName, PrimaryKeyAdditionStrategy, StrOrBoolOrArray, TableName,
    constraint::TableConstraint, foreign_key::ForeignKeySyntax, primary_key::PrimaryKeySyntax,
};

use super::{TableDef, TableValidationError};

type ColumnGroups = BTreeMap<String, Vec<ColumnName>>;
type ColumnGroupOrder = Vec<String>;
type ForeignKeyParts = (
    TableName,
    Vec<ColumnName>,
    Option<crate::schema::ReferenceAction>,
    Option<crate::schema::ReferenceAction>,
);

pub(super) fn normalize(table: &TableDef) -> Result<TableDef, TableValidationError> {
    let mut constraints = table.constraints.clone();

    add_primary_key_constraint(table, &mut constraints);
    add_unique_constraints(table, &mut constraints)?;
    add_foreign_key_constraints(table, &mut constraints)?;
    add_index_constraints(table, &mut constraints)?;

    Ok(TableDef {
        name: table.name.clone(),
        description: table.description.clone(),
        columns: table.columns.clone(),
        constraints,
    })
}

fn add_primary_key_constraint(table: &TableDef, constraints: &mut Vec<TableConstraint>) {
    let (pk_columns, pk_auto_increment) = collect_primary_key_columns(table);
    if pk_columns.is_empty() || has_primary_key_constraint(constraints) {
        return;
    }

    constraints.push(TableConstraint::PrimaryKey {
        auto_increment: pk_auto_increment,
        columns: pk_columns,
        strategy: PrimaryKeyAdditionStrategy::default(),
    });
}

fn collect_primary_key_columns(table: &TableDef) -> (Vec<ColumnName>, bool) {
    let mut columns = Vec::new();
    let mut auto_increment = false;

    for col in &table.columns {
        if let Some(ref pk) = col.primary_key {
            match pk {
                PrimaryKeySyntax::Bool(true) => columns.push(col.name.clone()),
                PrimaryKeySyntax::Bool(false) => {}
                PrimaryKeySyntax::Object(pk_def) => {
                    columns.push(col.name.clone());
                    auto_increment |= pk_def.auto_increment;
                }
            }
        }
    }

    (columns, auto_increment)
}

fn has_primary_key_constraint(constraints: &[TableConstraint]) -> bool {
    constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }))
}

fn add_unique_constraints(
    table: &TableDef,
    constraints: &mut Vec<TableConstraint>,
) -> Result<(), TableValidationError> {
    let (unique_groups, unique_order) = collect_unique_groups(table);
    add_unique_constraints_from_groups(&unique_groups, &unique_order, constraints)
}

fn add_unique_constraints_from_groups(
    unique_groups: &ColumnGroups,
    unique_order: &[String],
    constraints: &mut Vec<TableConstraint>,
) -> Result<(), TableValidationError> {
    for unique_name in unique_order {
        let columns = unique_groups
            .get(unique_name)
            .ok_or_else(|| TableValidationError::InvariantViolation {
                context: format!("unique group '{unique_name}' missing during normalize"),
            })?
            .clone();
        let constraint_name = generated_name_to_constraint_name(unique_name);

        if !unique_constraint_exists(constraints, constraint_name.as_ref(), &columns) {
            constraints.push(TableConstraint::Unique {
                name: constraint_name,
                columns,
                strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: crate::schema::KeepPolicy::First,
                },
            });
        }
    }
    Ok(())
}

fn collect_unique_groups(table: &TableDef) -> (ColumnGroups, ColumnGroupOrder) {
    let mut groups = ColumnGroups::new();
    let mut order = Vec::new();

    for col in &table.columns {
        if let Some(ref unique_val) = col.unique {
            match unique_val {
                StrOrBoolOrArray::Str(name) => {
                    push_grouped_column(&mut groups, &mut order, name.clone(), &col.name);
                }
                StrOrBoolOrArray::Bool(true) => {
                    push_grouped_column(
                        &mut groups,
                        &mut order,
                        format!("__auto_{}", col.name),
                        &col.name,
                    );
                }
                StrOrBoolOrArray::Bool(false) => {}
                StrOrBoolOrArray::Array(names) => {
                    for unique_name in names {
                        push_grouped_column(
                            &mut groups,
                            &mut order,
                            unique_name.clone(),
                            &col.name,
                        );
                    }
                }
            }
        }
    }

    (groups, order)
}

fn push_grouped_column(
    groups: &mut ColumnGroups,
    order: &mut Vec<String>,
    group_name: String,
    column_name: &str,
) {
    // Own the key exactly once: `or_default()` needs it as the map key, and a
    // first sighting also needs it in `order`. `groups` never stores empty
    // vectors (a group exists only after a push below), so a Vec just created
    // by `or_default()` is exactly the first sighting that must register in
    // `order`. To avoid a second `.to_string()`, register in `order` before
    // moving the key into the map on that first sighting.
    let first_sighting = !groups.contains_key(&group_name);
    if first_sighting {
        order.push(group_name.clone());
    }
    groups
        .entry(group_name)
        .or_default()
        .push(column_name.into());
}

fn generated_name_to_constraint_name(name: &str) -> Option<String> {
    if name.starts_with("__auto_") {
        None
    } else {
        Some(name.to_string())
    }
}

fn unique_constraint_exists(
    constraints: &[TableConstraint],
    constraint_name: Option<&String>,
    columns: &[ColumnName],
) -> bool {
    constraints.iter().any(|c| {
        if let TableConstraint::Unique {
            name,
            columns: cols,
            ..
        } = c
        {
            match (constraint_name, name) {
                (Some(n1), Some(n2)) => n1 == n2,
                (None, None) => cols.as_slice() == columns,
                _ => false,
            }
        } else {
            false
        }
    })
}

fn add_foreign_key_constraints(
    table: &TableDef,
    constraints: &mut Vec<TableConstraint>,
) -> Result<(), TableValidationError> {
    for col in &table.columns {
        if let Some(ref fk_syntax) = col.foreign_key {
            let (ref_table, ref_columns, on_delete, on_update) =
                parse_foreign_key(&col.name, fk_syntax)?;
            if !foreign_key_constraint_exists(constraints, &col.name) {
                constraints.push(TableConstraint::ForeignKey {
                    name: None,
                    columns: vec![col.name.clone()],
                    ref_table,
                    ref_columns,
                    on_delete,
                    on_update,
                    orphan_strategy: crate::schema::ForeignKeyOrphanStrategy::default(),
                });
            }
        }
    }
    Ok(())
}

fn parse_foreign_key(
    column_name: &str,
    fk_syntax: &ForeignKeySyntax,
) -> Result<ForeignKeyParts, TableValidationError> {
    match fk_syntax {
        ForeignKeySyntax::String(s) => parse_foreign_key_reference(column_name, s)
            .map(|(table, columns)| (table, columns, None, None)),
        ForeignKeySyntax::Reference(ref_syntax) => {
            parse_foreign_key_reference(column_name, &ref_syntax.references).map(
                |(table, columns)| {
                    (
                        table,
                        columns,
                        ref_syntax.on_delete.clone(),
                        ref_syntax.on_update.clone(),
                    )
                },
            )
        }
        ForeignKeySyntax::Object(fk_def) => Ok((
            fk_def.ref_table.clone(),
            fk_def.ref_columns.clone(),
            fk_def.on_delete.clone(),
            fk_def.on_update.clone(),
        )),
    }
}

fn parse_foreign_key_reference(
    column_name: &str,
    reference: &str,
) -> Result<(TableName, Vec<ColumnName>), TableValidationError> {
    let (head, tail) = reference
        .split_once('.')
        .filter(|(head, tail)| !head.is_empty() && !tail.is_empty() && !tail.contains('.'))
        .ok_or_else(|| TableValidationError::InvalidForeignKeyFormat {
            column_name: column_name.to_string(),
            value: reference.to_string(),
        })?;

    Ok((head.into(), vec![tail.into()]))
}

fn foreign_key_constraint_exists(constraints: &[TableConstraint], column_name: &str) -> bool {
    constraints.iter().any(|c| {
        if let TableConstraint::ForeignKey { columns, .. } = c {
            columns.len() == 1 && columns[0] == column_name
        } else {
            false
        }
    })
}

fn add_index_constraints(
    table: &TableDef,
    constraints: &mut Vec<TableConstraint>,
) -> Result<(), TableValidationError> {
    let (index_groups, index_order) = collect_index_groups(table)?;
    add_index_constraints_from_groups(&index_groups, &index_order, constraints)
}

fn add_index_constraints_from_groups(
    index_groups: &ColumnGroups,
    index_order: &[String],
    constraints: &mut Vec<TableConstraint>,
) -> Result<(), TableValidationError> {
    for index_name in index_order {
        let columns = index_groups
            .get(index_name)
            .ok_or_else(|| TableValidationError::InvariantViolation {
                context: format!("index group '{index_name}' missing during normalize"),
            })?
            .clone();
        let constraint_name = generated_name_to_constraint_name(index_name);

        if !index_constraint_exists(constraints, constraint_name.as_ref(), &columns) {
            constraints.push(TableConstraint::Index {
                name: constraint_name,
                columns,
            });
        }
    }
    Ok(())
}

fn collect_index_groups(
    table: &TableDef,
) -> Result<(ColumnGroups, ColumnGroupOrder), TableValidationError> {
    let mut groups = ColumnGroups::new();
    let mut order = Vec::new();
    let mut tracker: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for col in &table.columns {
        if let Some(ref index_val) = col.index {
            collect_column_indexes(index_val, &col.name, &mut groups, &mut order, &mut tracker)?;
        }
    }

    Ok((groups, order))
}

fn collect_column_indexes(
    index_val: &StrOrBoolOrArray,
    column_name: &str,
    groups: &mut ColumnGroups,
    order: &mut Vec<String>,
    tracker: &mut BTreeMap<String, BTreeSet<String>>,
) -> Result<(), TableValidationError> {
    match index_val {
        StrOrBoolOrArray::Str(name) => {
            push_checked_index(groups, order, tracker, name.clone(), column_name)
        }
        StrOrBoolOrArray::Bool(true) => push_checked_index(
            groups,
            order,
            tracker,
            format!("__auto_{column_name}"),
            column_name,
        ),
        StrOrBoolOrArray::Bool(false) => Ok(()),
        StrOrBoolOrArray::Array(names) => {
            push_index_array(groups, order, tracker, names, column_name)
        }
    }
}

fn push_index_array(
    groups: &mut ColumnGroups,
    order: &mut Vec<String>,
    tracker: &mut BTreeMap<String, BTreeSet<String>>,
    names: &[String],
    column_name: &str,
) -> Result<(), TableValidationError> {
    let mut seen_in_array = BTreeSet::new();
    for index_name in names {
        if seen_in_array.contains(index_name.as_str()) {
            return Err(duplicate_index(index_name, column_name));
        }
        seen_in_array.insert(index_name.clone());
        push_checked_index(groups, order, tracker, index_name.clone(), column_name)?;
    }
    Ok(())
}

fn push_checked_index(
    groups: &mut ColumnGroups,
    order: &mut Vec<String>,
    tracker: &mut BTreeMap<String, BTreeSet<String>>,
    index_name: String,
    column_name: &str,
) -> Result<(), TableValidationError> {
    if let Some(columns) = tracker.get(index_name.as_str())
        && columns.contains(column_name)
    {
        return Err(duplicate_index(&index_name, column_name));
    }

    push_grouped_column(groups, order, index_name.clone(), column_name);
    tracker
        .entry(index_name)
        .or_default()
        .insert(column_name.to_string());
    Ok(())
}

fn duplicate_index(index_name: &str, column_name: &str) -> TableValidationError {
    TableValidationError::DuplicateIndexColumn {
        index_name: index_name.to_string(),
        column_name: column_name.to_string(),
    }
}

fn index_constraint_exists(
    constraints: &[TableConstraint],
    constraint_name: Option<&String>,
    columns: &[ColumnName],
) -> bool {
    constraints.iter().any(|c| {
        if let TableConstraint::Index {
            name,
            columns: cols,
        } = c
        {
            match (constraint_name, name) {
                (Some(n1), Some(n2)) => n1 == n2,
                (None, None) => cols.as_slice() == columns,
                _ => false,
            }
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_unique_group_returns_invariant_violation() {
        let mut constraints = Vec::new();
        let groups = ColumnGroups::new();
        let order = vec!["uq_missing".to_string()];

        let err =
            add_unique_constraints_from_groups(&groups, &order, &mut constraints).unwrap_err();

        assert_eq!(
            err,
            TableValidationError::InvariantViolation {
                context: "unique group 'uq_missing' missing during normalize".to_string()
            }
        );
    }

    #[test]
    fn missing_index_group_returns_invariant_violation() {
        let mut constraints = Vec::new();
        let groups = ColumnGroups::new();
        let order = vec!["ix_missing".to_string()];

        let err = add_index_constraints_from_groups(&groups, &order, &mut constraints).unwrap_err();

        assert_eq!(
            err,
            TableValidationError::InvariantViolation {
                context: "index group 'ix_missing' missing during normalize".to_string()
            }
        );
    }
}
