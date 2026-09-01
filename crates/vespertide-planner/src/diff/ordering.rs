use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use vespertide_core::{
    ColumnType, ComplexColumnType, EnumValues, MigrationAction, TableConstraint, TableDef,
};

use crate::error::PlannerError;

/// Kahn's-algorithm core shared by creation and deletion ordering.
///
/// `dependencies` maps each name to the set of names it depends on and must
/// contain an entry for EVERY name to order. Returns names in ready order —
/// deterministic because the zero-degree seed follows `BTreeMap` key order and
/// each step collects newly-ready names into a `BTreeSet`. On a cycle the
/// result is partial (cyclic names are absent); callers decide how to react.
fn kahn_ready_order<'a>(dependencies: &BTreeMap<&'a str, BTreeSet<&'a str>>) -> Vec<&'a str> {
    // SEQUENTIAL BY NATURE: Kahn's algorithm requires in-degree state evolution.
    let mut in_degree: BTreeMap<&'a str, usize> = dependencies
        .iter()
        .map(|(name, deps)| (*name, deps.len()))
        .collect();

    // Start with names that have no dependencies.
    // BTreeMap iteration is already sorted by key.
    let mut queue: VecDeque<&'a str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();

    let mut order: Vec<&'a str> = Vec::with_capacity(dependencies.len());
    while let Some(name) = queue.pop_front() {
        order.push(name);

        // Collect names that become ready (in-degree becomes 0).
        // Use BTreeSet for consistent ordering.
        let mut ready: BTreeSet<&'a str> = BTreeSet::new();
        for (dependent, deps) in dependencies {
            if deps.contains(&name)
                && let Some(degree) = in_degree.get_mut(dependent)
            {
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(dependent);
                }
            }
        }
        for t in ready {
            queue.push_back(t);
        }
    }
    order
}

/// Topologically sort tables based on foreign key dependencies.
/// Returns tables in order where tables with no FK dependencies come first,
/// and tables that reference other tables come after their referenced tables.
pub(super) fn topological_sort_tables<'a>(
    tables: &[&'a TableDef],
) -> Result<Vec<&'a TableDef>, PlannerError> {
    if tables.is_empty() {
        return Ok(vec![]);
    }

    // Build a map of table names for quick lookup
    let table_names: HashSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();

    // Build adjacency list: for each table, list the tables it depends on (via FK)
    // Use BTreeMap for consistent ordering
    // Use BTreeSet to avoid duplicate dependencies (e.g., multiple FKs referencing the same table)
    let mut dependencies: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for table in tables {
        let mut deps_set: BTreeSet<&str> = BTreeSet::new();
        for constraint in &table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint {
                // Only consider dependencies within the set of tables being created
                if table_names.contains(ref_table.as_str()) && ref_table != &table.name {
                    deps_set.insert(ref_table.as_str());
                }
            }
        }
        dependencies.insert(table.name.as_str(), deps_set);
    }

    // Kahn's algorithm for topological sort (shared core), then map the
    // ready-ordered names back to their table definitions.
    let table_map: BTreeMap<&str, &TableDef> =
        tables.iter().map(|t| (t.name.as_str(), *t)).collect();
    let result: Vec<&TableDef> = kahn_ready_order(&dependencies)
        .into_iter()
        .filter_map(|name| table_map.get(name).copied())
        .collect();

    // Check for cycles
    if result.len() != tables.len() {
        // Collect the already-placed table names once so the `remaining` filter
        // is an O(log n) set lookup instead of a nested `result.iter().any(...)`
        // rescan per table. Cold error path, so this is a clarity/complexity win.
        let placed: BTreeSet<&str> = result.iter().map(|t| t.name.as_str()).collect();
        let remaining: Vec<&str> = tables
            .iter()
            .map(|t| t.name.as_str())
            .filter(|name| !placed.contains(name))
            .collect();
        return Err(PlannerError::TableValidation(format!(
            "Circular foreign key dependency detected among tables: {remaining:?}"
        )));
    }

    Ok(result)
}

/// Sort `DeleteTable` actions so that tables with FK references are deleted first.
/// This is the reverse of creation order - use topological sort then reverse.
/// Helper function to extract table name from `DeleteTable` action
/// Safety: should only be called on `DeleteTable` actions
pub(super) fn extract_delete_table_name(action: &MigrationAction) -> &str {
    match action {
        MigrationAction::DeleteTable { table } => table.as_str(),
        _ => panic!("Expected DeleteTable action"),
    }
}

pub(super) fn sort_delete_tables(
    actions: &mut [MigrationAction],
    all_tables: &BTreeMap<&str, &TableDef>,
) {
    // Collect DeleteTable actions and their indices
    let delete_indices: Vec<usize> = actions
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            if matches!(a, MigrationAction::DeleteTable { .. }) {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    if delete_indices.len() <= 1 {
        return;
    }

    // Extract table names being deleted
    // Use BTreeSet for consistent ordering
    let delete_table_names: BTreeSet<&str> = delete_indices
        .iter()
        .map(|&i| extract_delete_table_name(&actions[i]))
        .collect();

    // Build dependency graph for tables being deleted
    // dependencies[A] = [B] means A has FK referencing B
    // Use BTreeMap for consistent ordering
    // Use BTreeSet to avoid duplicate dependencies (e.g., multiple FKs referencing the same table)
    let mut dependencies: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for &table_name in &delete_table_names {
        let mut deps_set: BTreeSet<&str> = BTreeSet::new();
        if let Some(table_def) = all_tables.get(table_name) {
            for constraint in &table_def.constraints {
                if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                    && delete_table_names.contains(ref_table.as_str())
                    && ref_table != table_name
                {
                    deps_set.insert(ref_table.as_str());
                }
            }
        }
        dependencies.insert(table_name, deps_set);
    }

    // Kahn's algorithm for topological sort (shared core): creation order
    // first, then reversed below into deletion order.
    let mut sorted_tables = kahn_ready_order(&dependencies);

    // Reverse to get deletion order (tables with dependencies should be deleted first)
    sorted_tables.reverse();

    let sorted_positions: BTreeMap<&str, usize> = sorted_tables
        .iter()
        .enumerate()
        .map(|(idx, &name)| (name, idx))
        .collect();

    // Reorder the DeleteTable actions among their existing slots according to
    // `sorted_positions`, WITHOUT cloning each action.
    //
    // Everything below works in RANK space (`0..k` over the delete run) rather
    // than in raw slot space. `delete_indices` is ascending (it comes from
    // `.enumerate()`), so rank `r` and slot `delete_indices[r]` are
    // order-isomorphic — ranks index the bookkeeping arrays directly, with no
    // `slot - base` translation and no sparse side table spanning the gaps
    // between delete slots.
    //
    // `order[dst]` holds the ORIGINAL rank whose action belongs at rank `dst`;
    // the stable sort keeps equal-position actions in their original relative
    // order, matching the previous stable `sort_by` byte-for-byte.
    let k = delete_indices.len();
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&rank| {
        let name = extract_delete_table_name(&actions[delete_indices[rank]]);
        sorted_positions.get(name).copied().unwrap_or(0)
    });

    // Apply the permutation `order` onto the delete slots via selection-style
    // swaps, moving each action into its destination with owned moves and no
    // clone. We keep two mutually-inverse rank arrays and update BOTH in O(1)
    // per swap, so resolving "which position currently holds the wanted action"
    // is a direct array read instead of an inner `.position(..)` linear rescan
    // — making the apply O(k) instead of O(k²):
    //   * `origin_at[pos]`  — original rank of the action now sitting at `pos`
    //   * `pos_of[origin]`  — its inverse
    // The reordered `DeleteTable` payloads stay byte-identical to before.
    let mut origin_at: Vec<usize> = (0..k).collect();
    let mut pos_of: Vec<usize> = (0..k).collect();
    for dst in 0..k {
        let want = order[dst];
        let src = pos_of[want];
        if src != dst {
            actions.swap(delete_indices[dst], delete_indices[src]);
            // Swap the two positions' bookkeeping so both arrays stay consistent.
            let displaced = origin_at[dst];
            origin_at.swap(dst, src);
            pos_of[want] = dst;
            pos_of[displaced] = src;
        }
    }
}

/// Compare two migration actions for sorting.
/// Returns ordering where `CreateTable` comes first, then non-FK-ref actions, then FK-ref actions.
pub(super) fn compare_actions_for_create_order(
    a: &MigrationAction,
    b: &MigrationAction,
    created_tables: &BTreeSet<String>,
) -> std::cmp::Ordering {
    let a_is_create = matches!(a, MigrationAction::CreateTable { .. });
    let b_is_create = matches!(b, MigrationAction::CreateTable { .. });

    // Check if action is AddConstraint with FK referencing a created table
    let a_refs_created = if let MigrationAction::AddConstraint {
        constraint: TableConstraint::ForeignKey { ref_table, .. },
        ..
    } = a
    {
        created_tables.contains(ref_table.as_str())
    } else {
        false
    };
    let b_refs_created = if let MigrationAction::AddConstraint {
        constraint: TableConstraint::ForeignKey { ref_table, .. },
        ..
    } = b
    {
        created_tables.contains(ref_table.as_str())
    } else {
        false
    };

    if a_is_create != b_is_create {
        return if a_is_create {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        };
    }

    if a_refs_created != b_refs_created {
        return if a_refs_created {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Less
        };
    }

    std::cmp::Ordering::Equal
}

/// Sort actions so that `CreateTable` actions come before `AddConstraint` actions
/// that reference those newly created tables via foreign keys.
pub(super) fn sort_create_before_add_constraint(actions: &mut [MigrationAction]) {
    // SEQUENTIAL: mutates the full action list after all table diffs are known.
    // Collect names of tables being created
    let created_tables: BTreeSet<String> = actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.to_string())
            } else {
                None
            }
        })
        .collect();

    if created_tables.is_empty() {
        return;
    }

    actions.sort_by(|a, b| compare_actions_for_create_order(a, b, &created_tables));
}

/// Returns true when both types are string enums and `needle` is a value the
/// change removes (present in `from`, absent in `to`). Direct membership test:
/// `needle ∈ (from \ to)` ⇔ `needle ∈ from ∧ needle ∉ to` — no set or clones.
fn string_enum_value_removed(from_type: &ColumnType, to_type: &ColumnType, needle: &str) -> bool {
    match (from_type, to_type) {
        (
            ColumnType::Complex(ComplexColumnType::Enum {
                values: EnumValues::String(from_values),
                ..
            }),
            ColumnType::Complex(ComplexColumnType::Enum {
                values: EnumValues::String(to_values),
                ..
            }),
        ) => from_values.iter().any(|v| v == needle) && !to_values.iter().any(|v| v == needle),
        _ => false,
    }
}

/// Extract the unquoted value from a SQL default string.
/// For enum defaults like `'active'`, returns `active`.
/// For values without quotes, returns as-is.
fn extract_unquoted_default(default_sql: &str) -> &str {
    let trimmed = default_sql.trim();
    if let Some(inner) = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
    {
        inner
    } else {
        trimmed
    }
}

/// Sort `ModifyColumnType` and `ModifyColumnDefault` actions when they affect the same
/// column AND involve enum value removal where the old default is the removed value.
///
/// When an enum value is being removed and the current default is that value,
/// the default must be changed BEFORE the type is modified (to remove the enum value).
/// Otherwise, the database will reject the ALTER TYPE because the default still
/// references a value that would be removed.
pub(super) fn sort_enum_default_dependencies(
    actions: &mut [MigrationAction],
    from_map: &BTreeMap<&str, &TableDef>,
) {
    // SEQUENTIAL: dependent action swaps require a complete ordered action list.
    // Find indices of ModifyColumnType and ModifyColumnDefault actions
    // Group by (table, column)
    let mut type_changes: BTreeMap<(&str, &str), (usize, &ColumnType)> = BTreeMap::new();
    let mut default_changes: BTreeMap<(&str, &str), usize> = BTreeMap::new();

    for (i, action) in actions.iter().enumerate() {
        match action {
            MigrationAction::ModifyColumnType {
                table,
                column,
                new_type,
                ..
            } => {
                type_changes.insert((table.as_str(), column.as_str()), (i, new_type));
            }
            MigrationAction::ModifyColumnDefault { table, column, .. } => {
                default_changes.insert((table.as_str(), column.as_str()), i);
            }
            _ => {}
        }
    }

    // Find pairs that need reordering
    let mut swaps: Vec<(usize, usize)> = Vec::new();

    for ((table, column), (type_idx, new_type)) in &type_changes {
        if let Some(&default_idx) = default_changes.get(&(*table, *column))
            && let Some(from_table) = from_map.get(table)
            && let Some(from_column) = from_table.columns.iter().find(|c| c.name == *column)
            && let Some(ref old_default) = from_column.default
        {
            // Both ModifyColumnType and ModifyColumnDefault exist for same column
            // Check if old default is one of the removed enum values
            let old_default_sql = old_default.to_sql();
            let old_default_unquoted = extract_unquoted_default(&old_default_sql);

            if string_enum_value_removed(&from_column.r#type, new_type, old_default_unquoted)
                && *type_idx < default_idx
            {
                // Old default is being removed - must change default BEFORE type
                swaps.push((*type_idx, default_idx));
            }
        }
    }

    // Apply swaps
    for (i, j) in swaps {
        actions.swap(i, j);
    }
}
