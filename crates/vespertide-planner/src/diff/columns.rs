use std::collections::{BTreeMap, BTreeSet};

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, MigrationAction, TableDef,
};

pub(super) fn diff_columns<'a>(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_tbl: &'a TableDef,
    to_tbl: &TableDef,
) -> BTreeSet<&'a str> {
    // Columns - use BTreeMap for consistent ordering
    let from_cols: BTreeMap<_, _> = from_tbl
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    let to_cols: BTreeMap<_, _> = to_tbl
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let deleted_columns = diff_deleted_columns(actions, table_name, &from_cols, &to_cols);
    diff_column_types(actions, table_name, &from_cols, &to_cols);
    diff_integer_enum_remappings(actions, table_name, &from_cols, &to_cols);
    diff_column_nullability(actions, table_name, &from_cols, &to_cols);
    diff_column_defaults(actions, table_name, &from_cols, &to_cols);
    diff_column_comments(actions, table_name, &from_cols, &to_cols);
    diff_added_columns(actions, table_name, &from_cols, &to_cols);

    deleted_columns
}

/// F7-(b): integer enum value re-stamp.
///
/// `requires_migration` returns `false` for integer-enum diffs because the
/// underlying SQL type stays `INTEGER`. That misses the case where the user
/// rewrites a variant's numeric `value` (e.g. `medium: 5 -> 10`): every
/// existing row in the column still stores the *old* integer, but the ORM
/// mapping now binds the new integer to that name. Without a re-stamp the
/// next deserialisation silently mis-classifies rows. This pass emits a
/// `RemapEnumValues` action carrying the `(old -> new)` pairs so the SQL
/// generator can produce a single atomic `UPDATE ... CASE WHEN ... END`.
fn diff_integer_enum_remappings(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        let Some(from_def) = from_cols.get(col) else {
            continue;
        };
        let mapping = compute_integer_enum_remapping(&from_def.r#type, &to_def.r#type);
        if mapping.is_empty() {
            continue;
        }
        actions.push(MigrationAction::RemapEnumValues {
            table: table_name.into(),
            column: (*col).into(),
            mapping,
        });
    }
}

/// Pure helper: returns a `BTreeMap<old_value, new_value>` for every
/// integer-enum variant whose name is shared between `from` and `to` but
/// whose `value` has shifted. Returns an empty map for non-integer-enum
/// columns, for unchanged mappings, and for variants that exist on only
/// one side (additions / removals are handled by other pathways).
///
/// `BTreeMap` iterates in `old_value` order so the emitted action — and
/// downstream snapshots — stay deterministic across runs.
fn compute_integer_enum_remapping(from: &ColumnType, to: &ColumnType) -> BTreeMap<i64, i64> {
    let (
        ColumnType::Complex(ComplexColumnType::Enum {
            values: EnumValues::Integer(from_items),
            ..
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            values: EnumValues::Integer(to_items),
            ..
        }),
    ) = (from, to)
    else {
        return BTreeMap::new();
    };
    // Fast path: identical variant lists can never remap — skip the map build.
    // This is the overwhelmingly common case (unchanged integer enum).
    if from_items == to_items {
        return BTreeMap::new();
    }
    // Fast path: a remap pairs a `from` variant with a same-named `to` variant
    // whose value shifted. If either side is empty there can be no shared name,
    // so the result is always empty — skip building `to_by_name` entirely.
    if from_items.is_empty() || to_items.is_empty() {
        return BTreeMap::new();
    }
    let to_by_name: BTreeMap<&str, i64> = to_items
        .iter()
        .map(|nv| (nv.name.as_str(), nv.value))
        .collect();
    from_items
        .iter()
        .filter_map(|from_item| {
            to_by_name
                .get(from_item.name.as_str())
                .filter(|&&new_val| new_val != from_item.value)
                .map(|&new_val| (from_item.value, new_val))
        })
        .collect()
}

fn diff_deleted_columns<'a>(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&'a str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) -> BTreeSet<&'a str> {
    // `from_cols` is a `BTreeMap`, so its keys are already in sorted order —
    // iterating them once yields the same order a `BTreeSet` would, letting us
    // emit each `DeleteColumn` action and populate the returned set in a single
    // pass. The set borrows the column names straight from the normalized
    // `TableDef` (they outlive the whole diff), so no `String` allocation per
    // deleted column is needed — the only consumer looks each name up by `&str`.
    let mut deleted_columns = BTreeSet::new();
    for col in from_cols.keys().filter(|col| !to_cols.contains_key(*col)) {
        actions.push(MigrationAction::DeleteColumn {
            table: table_name.into(),
            column: (*col).into(),
        });
        deleted_columns.insert(*col);
    }

    deleted_columns
}

fn diff_column_types(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col) {
            let needs_type_migration = from_def.r#type.requires_migration(&to_def.r#type);
            let needs_enum_rename = !needs_type_migration
                && matches!(
                    (&from_def.r#type, &to_def.r#type),
                    (
                        ColumnType::Complex(ComplexColumnType::Enum {
                            name: old_name,
                            values: old_values,
                        }),
                        ColumnType::Complex(ComplexColumnType::Enum {
                            name: new_name,
                            values: new_values,
                        }),
                    ) if old_name != new_name
                        && !old_values.is_integer()
                        && !new_values.is_integer()
                );

            if needs_type_migration || needs_enum_rename {
                actions.push(MigrationAction::ModifyColumnType {
                    table: table_name.into(),
                    column: (*col).into(),
                    new_type: to_def.r#type.clone(),
                    fill_with: None,
                    // Set by `cmd_revision` after the user picks a strategy
                    // in the type-narrowing select prompt.
                    narrowing_strategy: None,
                    // Set by `cmd_revision` from the timezone-conversion prompt
                    // when the column transitions between timestamp/timestamptz.
                    timezone: None,
                });
            }
        }
    }
}

fn diff_column_nullability(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col)
            && from_def.nullable != to_def.nullable
        {
            actions.push(MigrationAction::ModifyColumnNullable {
                table: table_name.into(),
                column: (*col).into(),
                nullable: to_def.nullable,
                fill_with: None,
                delete_null_rows: None,
            });
        }
    }
}

fn diff_column_defaults(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col) {
            // Fast path: byte-identical raw defaults can never differ once
            // lowered to SQL, so skip the two `to_sql()` allocations for the
            // overwhelmingly common unchanged-column case. Structurally
            // distinct defaults still fall through to the SQL-level compare
            // below, so no `ModifyColumnDefault` is ever spuriously added or
            // dropped.
            if from_def.default == to_def.default {
                continue;
            }
            let from_default = from_def
                .default
                .as_ref()
                .map(vespertide_core::DefaultValue::to_sql);
            let to_default = to_def
                .default
                .as_ref()
                .map(vespertide_core::DefaultValue::to_sql);
            if from_default != to_default {
                actions.push(MigrationAction::ModifyColumnDefault {
                    table: table_name.into(),
                    column: (*col).into(),
                    new_default: to_default,
                    backfill: None,
                });
            }
        }
    }
}

fn diff_column_comments(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, to_def) in to_cols {
        if let Some(from_def) = from_cols.get(col)
            && from_def.comment != to_def.comment
        {
            actions.push(MigrationAction::ModifyColumnComment {
                table: table_name.into(),
                column: (*col).into(),
                new_comment: to_def.comment.clone(),
            });
        }
    }
}

fn diff_added_columns(
    actions: &mut Vec<MigrationAction>,
    table_name: &str,
    from_cols: &BTreeMap<&str, &ColumnDef>,
    to_cols: &BTreeMap<&str, &ColumnDef>,
) {
    for (col, def) in to_cols {
        if !from_cols.contains_key(col) {
            // Build the stripped column directly instead of cloning the full
            // `ColumnDef` then clearing the four inline-constraint fields — the
            // clone would deep-copy those `Option` payloads (usually `Some(..)`
            // on freshly-authored columns) only to discard them. Emitted payload
            // is byte-identical. An exhaustive struct literal keeps this in sync
            // with `ColumnDef` (a new field fails to compile until handled here).
            let col_def = ColumnDef {
                name: def.name.clone(),
                r#type: def.r#type.clone(),
                nullable: def.nullable,
                default: def.default.clone(),
                comment: def.comment.clone(),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            };
            actions.push(MigrationAction::AddColumn {
                table: table_name.into(),
                column: Box::new(col_def),
                fill_with: None,
            });
        }
    }
}
