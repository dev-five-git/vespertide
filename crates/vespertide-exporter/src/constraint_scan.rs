//! Shared constraint-scan helpers used by the ORM renderers.
//!
//! Every backend needs the same lookup sets when rendering a table:
//! the columns covered by table-level primary keys, the columns that
//! carry a single-column unique constraint, and the columns that carry
//! a single-column index. Centralising the scans keeps the renderers
//! from drifting apart.

use std::collections::{HashMap, HashSet};

use vespertide_core::{ColumnName, ReferenceAction, TableConstraint, TableDef};
use vespertide_naming::{infer_relation_field_name, to_pascal_case};

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

/// The table's `PrimaryKey` constraint, if it declares one.
pub(crate) fn primary_key(constraints: &[TableConstraint]) -> Option<&TableConstraint> {
    constraints
        .iter()
        .find(|c| matches!(c, TableConstraint::PrimaryKey { .. }))
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

/// A single-column foreign key's target and referential actions.
pub(crate) struct FkDetails<'a> {
    pub(crate) ref_table: &'a str,
    pub(crate) ref_column: &'a str,
    pub(crate) on_delete: Option<&'a ReferenceAction>,
    pub(crate) on_update: Option<&'a ReferenceAction>,
}

/// Map each single-column foreign key's column name to its target and
/// referential actions. Only foreign keys with exactly one owning column and
/// one referenced column are included; composite FKs are skipped.
///
/// Lookup-only, ordering unused.
pub(crate) fn single_column_fk_details(
    constraints: &[TableConstraint],
) -> HashMap<&str, FkDetails<'_>> {
    let mut map = HashMap::new();
    for constraint in constraints {
        if let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
            ..
        } = constraint
            && columns.len() == 1
            && ref_columns.len() == 1
        {
            map.insert(
                columns[0].as_str(),
                FkDetails {
                    ref_table: ref_table.as_str(),
                    ref_column: ref_columns[0].as_str(),
                    on_delete: on_delete.as_ref(),
                    on_update: on_update.as_ref(),
                },
            );
        }
    }
    map
}

/// Name segment a relation derives from its FK columns.
///
/// Every column takes part — two composite FKs to the same target can share a
/// first column, and a segment built from that column alone would hand both
/// relations one name.
pub(crate) fn relation_segment(columns: &[ColumnName]) -> String {
    columns
        .iter()
        .map(|col| infer_relation_field_name(col.as_str()))
        .collect::<Vec<_>>()
        .join("_")
}

/// Relation name for every FK of `table`, keyed by constraint index.
///
/// Both ends of a relation must carry the same name, so the forward side and
/// each backend's back-relation collector derive it from the same table
/// through this one function. Distinct columns can still strip to one segment
/// (`a_id` and `a` both become `a`), and both Prisma and Drizzle reject two
/// same-named relations between one model pair — repeats within a target's
/// group get numbered in constraint order, which is the one order both ends
/// share.
pub(crate) fn fk_relation_names(table: &TableDef) -> HashMap<usize, String> {
    let table_pascal = to_pascal_case(&table.name);
    let mut used_per_target: HashMap<(&str, String), usize> = HashMap::new();
    let mut names = HashMap::new();
    for (idx, c) in table.constraints.iter().enumerate() {
        let TableConstraint::ForeignKey {
            columns, ref_table, ..
        } = c
        else {
            continue;
        };
        let base = format!(
            "{table_pascal}{}",
            to_pascal_case(&relation_segment(columns))
        );
        let seen = used_per_target
            .entry((ref_table.as_str(), base.clone()))
            .or_insert(0);
        *seen += 1;
        let name = if *seen == 1 {
            base
        } else {
            format!("{base}{seen}")
        };
        names.insert(idx, name);
    }
    names
}

/// One table's reverse view of a foreign key another table points at it with.
///
/// Field naming and rendering differ per backend, but the scan — which FKs
/// target this table, whether each is one-to-one, and the shared relation
/// name both ends must agree on — is the same everywhere.
pub(crate) struct BackRelation {
    pub(crate) source_table: String,
    pub(crate) fk_columns: Vec<String>,
    pub(crate) ref_columns: Vec<String>,
    pub(crate) on_delete: Option<ReferenceAction>,
    pub(crate) on_update: Option<ReferenceAction>,
    pub(crate) rel_segment: String,
    pub(crate) is_one_to_one: bool,
    pub(crate) relation_name: Option<String>,
}

/// Reverse relations for `target_table`: one entry per FK any table in
/// `schema` (including itself) points at it with.
pub(crate) fn collect_back_relations(target_table: &str, schema: &[TableDef]) -> Vec<BackRelation> {
    let mut result = Vec::new();

    for source in schema {
        let fks_to_target = source
            .constraints
            .iter()
            .filter(|c| {
                matches!(c, TableConstraint::ForeignKey { ref_table, .. }
                    if ref_table.as_str() == target_table)
            })
            .count();

        if fks_to_target == 0 {
            continue;
        }

        let source_relation_names = fk_relation_names(source);
        let multi_fk = fks_to_target > 1;
        let is_self_ref = source.name.as_str() == target_table;

        for (constraint_idx, constraint) in source.constraints.iter().enumerate() {
            let TableConstraint::ForeignKey {
                columns: fk_cols,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } = constraint
            else {
                continue;
            };
            if ref_table.as_str() != target_table {
                continue;
            }

            // A key is one-to-one when the source can hold at most one row
            // per target key: its FK columns are exactly its own PK, or a
            // unique covers exactly that set.
            let fk_set: HashSet<&str> = fk_cols.iter().map(ColumnName::as_str).collect();
            let pk_cols = primary_key(&source.constraints)
                .map(TableConstraint::columns)
                .unwrap_or_default();
            let is_one_to_one = pk_cols.len() == fk_set.len()
                && pk_cols.iter().all(|c| fk_set.contains(c.as_str()))
                || source.constraints.iter().any(|c| {
                    matches!(c, TableConstraint::Unique { columns, .. }
                        if columns.len() == fk_set.len()
                            && columns.iter().all(|col| fk_set.contains(col.as_str())))
                });

            let rel_segment = relation_segment(fk_cols);
            let relation_name = if multi_fk || is_self_ref {
                source_relation_names.get(&constraint_idx).cloned()
            } else {
                None
            };

            result.push(BackRelation {
                source_table: source.name.as_str().to_string(),
                fk_columns: fk_cols.iter().map(ToString::to_string).collect(),
                ref_columns: ref_columns.iter().map(ToString::to_string).collect(),
                on_delete: on_delete.clone(),
                on_update: on_update.clone(),
                rel_segment,
                is_one_to_one,
                relation_name,
            });
        }
    }

    result
}
