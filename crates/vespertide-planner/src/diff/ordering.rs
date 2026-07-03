use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use vespertide_core::{
    ColumnType, ComplexColumnType, EnumValues, MigrationAction, TableConstraint, TableDef,
};

use crate::error::PlannerError;

/// Topologically sort tables based on foreign key dependencies.
/// Returns tables in order where tables with no FK dependencies come first,
/// and tables that reference other tables come after their referenced tables.
pub(super) fn topological_sort_tables<'a>(
    tables: &[&'a TableDef],
) -> Result<Vec<&'a TableDef>, PlannerError> {
    // SEQUENTIAL BY NATURE: Kahn's algorithm requires in-degree state evolution.
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

    // Kahn's algorithm for topological sort
    // Calculate in-degrees (number of tables that depend on each table)
    // Use BTreeMap for consistent ordering
    let mut in_degree: BTreeMap<&str, usize> = BTreeMap::new();
    for table in tables {
        in_degree.entry(table.name.as_str()).or_insert(0);
    }

    // For each dependency, increment the in-degree of the dependent table
    for (table_name, deps) in &dependencies {
        for _dep in deps {
            // The table has dependencies, so those referenced tables must come first
            // We actually want the reverse: tables with dependencies have higher in-degree
        }
        // Actually, we need to track: if A depends on B, then A has in-degree from B
        // So A cannot be processed until B is processed
        *in_degree.entry(table_name).or_insert(0) += deps.len();
    }

    // Start with tables that have no dependencies
    // BTreeMap iteration is already sorted by key
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();

    let mut result: Vec<&TableDef> = Vec::with_capacity(tables.len());
    let table_map: BTreeMap<&str, &TableDef> =
        tables.iter().map(|t| (t.name.as_str(), *t)).collect();

    while let Some(table_name) = queue.pop_front() {
        if let Some(&table) = table_map.get(table_name) {
            result.push(table);
        }

        // Collect tables that become ready (in-degree becomes 0)
        // Use BTreeSet for consistent ordering
        let mut ready_tables: BTreeSet<&str> = BTreeSet::new();
        for (dependent, deps) in &dependencies {
            if deps.contains(&table_name)
                && let Some(degree) = in_degree.get_mut(dependent)
            {
                *degree -= 1;
                if *degree == 0 {
                    ready_tables.insert(dependent);
                }
            }
        }
        for t in ready_tables {
            queue.push_back(t);
        }
    }

    // Check for cycles
    if result.len() != tables.len() {
        let remaining: Vec<&str> = tables
            .iter()
            .map(|t| t.name.as_str())
            .filter(|name| !result.iter().any(|t| t.name.as_str() == *name))
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
    // SEQUENTIAL BY NATURE: Kahn's algorithm requires in-degree state evolution.
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

    // Use Kahn's algorithm for topological sort
    // in_degree[A] = number of tables A depends on
    // Use BTreeMap for consistent ordering
    let mut in_degree: BTreeMap<&str, usize> = BTreeMap::new();
    for &table_name in &delete_table_names {
        in_degree.insert(
            table_name,
            dependencies
                .get(table_name)
                .map_or(0, std::collections::BTreeSet::len),
        );
    }

    // Start with tables that have no dependencies (can be deleted last in creation order)
    // BTreeMap iteration is already sorted
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();

    let mut sorted_tables: Vec<&str> = Vec::with_capacity(delete_table_names.len());
    while let Some(table_name) = queue.pop_front() {
        sorted_tables.push(table_name);

        // For each table that has this one as a dependency, decrement its in-degree
        // Use BTreeSet for consistent ordering of newly ready tables
        let mut ready_tables: BTreeSet<&str> = BTreeSet::new();
        for (&dependent, deps) in &dependencies {
            if deps.contains(&table_name)
                && let Some(degree) = in_degree.get_mut(dependent)
            {
                *degree -= 1;
                if *degree == 0 {
                    ready_tables.insert(dependent);
                }
            }
        }
        for t in ready_tables {
            queue.push_back(t);
        }
    }

    // Reverse to get deletion order (tables with dependencies should be deleted first)
    sorted_tables.reverse();

    let sorted_positions: BTreeMap<&str, usize> = sorted_tables
        .iter()
        .enumerate()
        .map(|(idx, &name)| (name, idx))
        .collect();

    // Reorder the DeleteTable actions according to sorted order
    let mut delete_actions: Vec<MigrationAction> =
        delete_indices.iter().map(|&i| actions[i].clone()).collect();

    delete_actions.sort_by(|a, b| {
        let a_name = extract_delete_table_name(a);
        let b_name = extract_delete_table_name(b);

        let a_pos = sorted_positions.get(a_name).copied().unwrap_or(0);
        let b_pos = sorted_positions.get(b_name).copied().unwrap_or(0);
        a_pos.cmp(&b_pos)
    });

    // Put them back
    for (i, idx) in delete_indices.iter().enumerate() {
        actions[*idx] = delete_actions[i].clone();
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

/// Get the set of string enum values that were removed (present in `from` but not in `to`).
/// Returns None if either type is not a string enum.
fn get_removed_string_enum_values(
    from_type: &ColumnType,
    to_type: &ColumnType,
) -> Option<Vec<String>> {
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
        ) => {
            let to_set: HashSet<&str> = to_values.iter().map(std::string::String::as_str).collect();
            let removed: Vec<String> = from_values
                .iter()
                .filter(|v| !to_set.contains(v.as_str()))
                .cloned()
                .collect();
            if removed.is_empty() {
                None
            } else {
                Some(removed)
            }
        }
        _ => None,
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
            && let Some(removed_values) =
                get_removed_string_enum_values(&from_column.r#type, new_type)
            && let Some(ref old_default) = from_column.default
        {
            // Both ModifyColumnType and ModifyColumnDefault exist for same column
            // Check if old default is one of the removed enum values
            let old_default_sql = old_default.to_sql();
            let old_default_unquoted = extract_unquoted_default(&old_default_sql);

            if removed_values.iter().any(|v| v == old_default_unquoted) && *type_idx < default_idx {
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
