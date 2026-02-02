use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use vespertide_core::{
    ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan, TableConstraint,
    TableDef,
};

use crate::error::PlannerError;

/// Topologically sort tables based on foreign key dependencies.
/// Returns tables in order where tables with no FK dependencies come first,
/// and tables that reference other tables come after their referenced tables.
fn topological_sort_tables<'a>(tables: &[&'a TableDef]) -> Result<Vec<&'a TableDef>, PlannerError> {
    if tables.is_empty() {
        return Ok(vec![]);
    }

    // Build a map of table names for quick lookup
    let table_names: HashSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();

    // Build adjacency list: for each table, list the tables it depends on (via FK)
    // Use BTreeMap for consistent ordering
    // Use BTreeSet to avoid duplicate dependencies (e.g., multiple FKs referencing the same table)
    let mut dependencies: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
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
        dependencies.insert(table.name.as_str(), deps_set.into_iter().collect());
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

    let mut result: Vec<&TableDef> = Vec::new();
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
            "Circular foreign key dependency detected among tables: {:?}",
            remaining
        )));
    }

    Ok(result)
}

/// Sort DeleteTable actions so that tables with FK references are deleted first.
/// This is the reverse of creation order - use topological sort then reverse.
/// Helper function to extract table name from DeleteTable action
/// Safety: should only be called on DeleteTable actions
fn extract_delete_table_name(action: &MigrationAction) -> &str {
    match action {
        MigrationAction::DeleteTable { table } => table.as_str(),
        _ => panic!("Expected DeleteTable action"),
    }
}

fn sort_delete_tables(actions: &mut [MigrationAction], all_tables: &BTreeMap<&str, &TableDef>) {
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
    let mut dependencies: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
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
        dependencies.insert(table_name, deps_set.into_iter().collect());
    }

    // Use Kahn's algorithm for topological sort
    // in_degree[A] = number of tables A depends on
    // Use BTreeMap for consistent ordering
    let mut in_degree: BTreeMap<&str, usize> = BTreeMap::new();
    for &table_name in &delete_table_names {
        in_degree.insert(
            table_name,
            dependencies.get(table_name).map_or(0, |d| d.len()),
        );
    }

    // Start with tables that have no dependencies (can be deleted last in creation order)
    // BTreeMap iteration is already sorted
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();

    let mut sorted_tables: Vec<&str> = Vec::new();
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

    // Reorder the DeleteTable actions according to sorted order
    let mut delete_actions: Vec<MigrationAction> =
        delete_indices.iter().map(|&i| actions[i].clone()).collect();

    delete_actions.sort_by(|a, b| {
        let a_name = extract_delete_table_name(a);
        let b_name = extract_delete_table_name(b);

        let a_pos = sorted_tables.iter().position(|&t| t == a_name).unwrap_or(0);
        let b_pos = sorted_tables.iter().position(|&t| t == b_name).unwrap_or(0);
        a_pos.cmp(&b_pos)
    });

    // Put them back
    for (i, idx) in delete_indices.iter().enumerate() {
        actions[*idx] = delete_actions[i].clone();
    }
}

/// Compare two migration actions for sorting.
/// Returns ordering where CreateTable comes first, then non-FK-ref actions, then FK-ref actions.
fn compare_actions_for_create_order(
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
        created_tables.contains(ref_table)
    } else {
        false
    };
    let b_refs_created = if let MigrationAction::AddConstraint {
        constraint: TableConstraint::ForeignKey { ref_table, .. },
        ..
    } = b
    {
        created_tables.contains(ref_table)
    } else {
        false
    };

    // Order: CreateTable first, then non-referencing actions, then referencing AddConstraint last
    match (a_is_create, b_is_create, a_refs_created, b_refs_created) {
        // Both CreateTable - maintain original order
        (true, true, _, _) => std::cmp::Ordering::Equal,
        // a is CreateTable, b is not - a comes first
        (true, false, _, _) => std::cmp::Ordering::Less,
        // a is not CreateTable, b is - b comes first
        (false, true, _, _) => std::cmp::Ordering::Greater,
        // Neither is CreateTable
        // If a refs created table and b doesn't, a comes after
        (false, false, true, false) => std::cmp::Ordering::Greater,
        // If b refs created table and a doesn't, b comes after
        (false, false, false, true) => std::cmp::Ordering::Less,
        // Both ref or both don't ref - maintain original order
        (false, false, _, _) => std::cmp::Ordering::Equal,
    }
}

/// Sort actions so that CreateTable actions come before AddConstraint actions
/// that reference those newly created tables via foreign keys.
fn sort_create_before_add_constraint(actions: &mut [MigrationAction]) {
    // Collect names of tables being created
    let created_tables: BTreeSet<String> = actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.clone())
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
            let to_set: HashSet<&str> = to_values.iter().map(|s| s.as_str()).collect();
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
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

/// Sort ModifyColumnType and ModifyColumnDefault actions when they affect the same
/// column AND involve enum value removal where the old default is the removed value.
///
/// When an enum value is being removed and the current default is that value,
/// the default must be changed BEFORE the type is modified (to remove the enum value).
/// Otherwise, the database will reject the ALTER TYPE because the default still
/// references a value that would be removed.
fn sort_enum_default_dependencies(
    actions: &mut [MigrationAction],
    from_map: &BTreeMap<&str, &TableDef>,
) {
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

/// Diff two schema snapshots into a migration plan.
/// Schemas are normalized for comparison purposes, but the original (non-normalized)
/// tables are used in migration actions to preserve inline constraint definitions.
pub fn diff_schemas(from: &[TableDef], to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    let mut actions: Vec<MigrationAction> = Vec::new();

    // Normalize both schemas for comparison (to ensure inline and table-level constraints are treated equally)
    let from_normalized: Vec<TableDef> = from
        .iter()
        .map(|t| {
            t.normalize().map_err(|e| {
                PlannerError::TableValidation(format!(
                    "Failed to normalize table '{}': {}",
                    t.name, e
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let to_normalized: Vec<TableDef> = to
        .iter()
        .map(|t| {
            t.normalize().map_err(|e| {
                PlannerError::TableValidation(format!(
                    "Failed to normalize table '{}': {}",
                    t.name, e
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Use BTreeMap for consistent ordering
    // Normalized versions for comparison
    let from_map: BTreeMap<_, _> = from_normalized
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let to_map: BTreeMap<_, _> = to_normalized.iter().map(|t| (t.name.as_str(), t)).collect();

    // Original (non-normalized) versions for migration storage
    let to_original_map: BTreeMap<_, _> = to.iter().map(|t| (t.name.as_str(), t)).collect();

    // Drop tables that disappeared.
    for name in from_map.keys() {
        if !to_map.contains_key(name) {
            actions.push(MigrationAction::DeleteTable {
                table: name.to_string(),
            });
        }
    }

    // Update existing tables and their indexes/columns.
    for (name, to_tbl) in &to_map {
        if let Some(from_tbl) = from_map.get(name) {
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

            // Deleted columns - collect the set of columns being deleted for this table
            let deleted_columns: BTreeSet<&str> = from_cols
                .keys()
                .filter(|col| !to_cols.contains_key(*col))
                .copied()
                .collect();

            for col in &deleted_columns {
                actions.push(MigrationAction::DeleteColumn {
                    table: name.to_string(),
                    column: col.to_string(),
                });
            }

            // Modified columns - type changes
            for (col, to_def) in &to_cols {
                if let Some(from_def) = from_cols.get(col)
                    && from_def.r#type.requires_migration(&to_def.r#type)
                {
                    actions.push(MigrationAction::ModifyColumnType {
                        table: name.to_string(),
                        column: col.to_string(),
                        new_type: to_def.r#type.clone(),
                    });
                }
            }

            // Modified columns - nullable changes
            for (col, to_def) in &to_cols {
                if let Some(from_def) = from_cols.get(col)
                    && from_def.nullable != to_def.nullable
                {
                    actions.push(MigrationAction::ModifyColumnNullable {
                        table: name.to_string(),
                        column: col.to_string(),
                        nullable: to_def.nullable,
                        fill_with: None,
                    });
                }
            }

            // Modified columns - default value changes
            for (col, to_def) in &to_cols {
                if let Some(from_def) = from_cols.get(col) {
                    let from_default = from_def.default.as_ref().map(|d| d.to_sql());
                    let to_default = to_def.default.as_ref().map(|d| d.to_sql());
                    if from_default != to_default {
                        actions.push(MigrationAction::ModifyColumnDefault {
                            table: name.to_string(),
                            column: col.to_string(),
                            new_default: to_default,
                        });
                    }
                }
            }

            // Modified columns - comment changes
            for (col, to_def) in &to_cols {
                if let Some(from_def) = from_cols.get(col)
                    && from_def.comment != to_def.comment
                {
                    actions.push(MigrationAction::ModifyColumnComment {
                        table: name.to_string(),
                        column: col.to_string(),
                        new_comment: to_def.comment.clone(),
                    });
                }
            }

            // Added columns
            // Note: Inline foreign keys are already converted to TableConstraint::ForeignKey
            // by normalize(), so they will be handled in the constraint diff below.
            for (col, def) in &to_cols {
                if !from_cols.contains_key(col) {
                    actions.push(MigrationAction::AddColumn {
                        table: name.to_string(),
                        column: Box::new((*def).clone()),
                        fill_with: None,
                    });
                }
            }

            // Constraints - compare and detect additions/removals (includes indexes)
            // Skip RemoveConstraint for constraints where ALL columns are being deleted
            // (the constraint will be automatically dropped when the column is dropped)
            for from_constraint in &from_tbl.constraints {
                if !to_tbl.constraints.contains(from_constraint) {
                    // Get the columns referenced by this constraint
                    let constraint_columns = from_constraint.columns();

                    // Skip if ALL columns of the constraint are being deleted
                    let all_columns_deleted = !constraint_columns.is_empty()
                        && constraint_columns
                            .iter()
                            .all(|col| deleted_columns.contains(col.as_str()));

                    if !all_columns_deleted {
                        actions.push(MigrationAction::RemoveConstraint {
                            table: name.to_string(),
                            constraint: from_constraint.clone(),
                        });
                    }
                }
            }
            for to_constraint in &to_tbl.constraints {
                if !from_tbl.constraints.contains(to_constraint) {
                    actions.push(MigrationAction::AddConstraint {
                        table: name.to_string(),
                        constraint: to_constraint.clone(),
                    });
                }
            }
        }
    }

    // Create new tables (and their indexes).
    // Use original (non-normalized) tables to preserve inline constraint definitions.
    // Collect new tables first, then topologically sort them by FK dependencies.
    let new_tables: Vec<&TableDef> = to_map
        .iter()
        .filter(|(name, _)| !from_map.contains_key(*name))
        .map(|(_, tbl)| *tbl)
        .collect();

    let sorted_new_tables = topological_sort_tables(&new_tables)?;

    for tbl in sorted_new_tables {
        // Get the original (non-normalized) table to preserve inline constraints
        let original_tbl = to_original_map.get(tbl.name.as_str()).unwrap();
        actions.push(MigrationAction::CreateTable {
            table: original_tbl.name.clone(),
            columns: original_tbl.columns.clone(),
            constraints: original_tbl.constraints.clone(),
        });
    }

    // Sort DeleteTable actions so tables with FK dependencies are deleted first
    sort_delete_tables(&mut actions, &from_map);

    // Sort so CreateTable comes before AddConstraint that references the new table
    sort_create_before_add_constraint(&mut actions);

    // Sort so ModifyColumnDefault comes before ModifyColumnType when removing enum values
    // that were used as the default
    sort_enum_default_dependencies(&mut actions, &from_map);

    Ok(MigrationPlan {
        comment: None,
        created_at: None,
        version: 0,
        actions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, SimpleColumnType,
        schema::{primary_key::PrimaryKeySyntax, str_or_bool::StrOrBoolOrArray},
    };

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(
        name: &str,
        columns: Vec<ColumnDef>,
        constraints: Vec<vespertide_core::TableConstraint>,
    ) -> TableDef {
        TableDef {
            name: name.to_string(),
            description: None,
            columns,
            constraints,
        }
    }

    fn idx(name: &str, columns: Vec<&str>) -> TableConstraint {
        TableConstraint::Index {
            name: Some(name.to_string()),
            columns: columns.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[rstest]
    #[case::add_column_and_index(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![idx("ix_users__name", vec!["name"])],
        )],
        vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(col("name", ColumnType::Simple(SimpleColumnType::Text))),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "users".into(),
                constraint: idx("ix_users__name", vec!["name"]),
            },
        ]
    )]
    #[case::drop_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        vec![],
        vec![MigrationAction::DeleteTable {
            table: "users".into()
        }]
    )]
    #[case::add_table_with_index(
        vec![],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )],
        vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![idx("idx_users_id", vec!["id"])],
            },
        ]
    )]
    #[case::delete_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        vec![MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "name".into(),
        }]
    )]
    #[case::modify_column_type(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
        )],
        vec![MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "id".into(),
            new_type: ColumnType::Simple(SimpleColumnType::Text),
        }]
    )]
    #[case::remove_index(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        vec![MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: idx("idx_users_id", vec!["id"]),
        }]
    )]
    #[case::add_index_existing_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![idx("idx_users_id", vec!["id"])],
        )],
        vec![MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: idx("idx_users_id", vec!["id"]),
        }]
    )]
    fn diff_schemas_detects_additions(
        #[case] from_schema: Vec<TableDef>,
        #[case] to_schema: Vec<TableDef>,
        #[case] expected_actions: Vec<MigrationAction>,
    ) {
        let plan = diff_schemas(&from_schema, &to_schema).unwrap();
        assert_eq!(plan.actions, expected_actions);
    }

    // Tests for integer enum handling
    mod integer_enum {
        use super::*;
        use vespertide_core::{ComplexColumnType, EnumValues, NumValue};

        #[test]
        fn integer_enum_values_changed_no_migration() {
            // Integer enum values changed - should NOT generate ModifyColumnType
            let from = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Pending".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Shipped".into(),
                                value: 1,
                            },
                        ]),
                    }),
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Pending".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Shipped".into(),
                                value: 1,
                            },
                            NumValue {
                                name: "Delivered".into(),
                                value: 2,
                            },
                            NumValue {
                                name: "Cancelled".into(),
                                value: 100,
                            },
                        ]),
                    }),
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert!(
                plan.actions.is_empty(),
                "Expected no actions, got: {:?}",
                plan.actions
            );
        }

        #[test]
        fn string_enum_values_changed_requires_migration() {
            // String enum values changed - SHOULD generate ModifyColumnType
            let from = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                    }),
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                            "delivered".into(),
                        ]),
                    }),
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnType { table, column, .. }
                if table == "orders" && column == "status"
            ));
        }

        #[test]
        fn string_enum_name_changed_same_values_no_migration() {
            // String enum name changed but values identical - should NOT generate migration
            // Reproduces the phantom diff: model "status" vs migration "article_status"
            let from = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                    }),
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "status".into(),
                        values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                    }),
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert!(
                plan.actions.is_empty(),
                "Expected no actions for enum name-only change, got: {:?}",
                plan.actions
            );
        }

        #[test]
        fn string_enum_name_and_values_changed_requires_migration() {
            // String enum name AND values changed - SHOULD generate ModifyColumnType
            let from = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                    }),
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                            "delivered".into(),
                        ]),
                    }),
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnType { table, column, .. }
                if table == "orders" && column == "status"
            ));
        }
    }

    // Tests for enum + default value ordering
    mod enum_default_ordering {
        use super::*;
        use vespertide_core::{ComplexColumnType, EnumValues};

        fn col_enum_with_default(
            name: &str,
            enum_name: &str,
            values: Vec<&str>,
            default: &str,
        ) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: enum_name.to_string(),
                    values: EnumValues::String(values.into_iter().map(String::from).collect()),
                }),
                nullable: false,
                default: Some(default.into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }
        }

        #[test]
        fn enum_add_value_with_new_default() {
            // Case 1: Add new enum value and change default to that new value
            // Expected order: ModifyColumnType FIRST (add value), then ModifyColumnDefault
            let from = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped"],
                    "'pending'",
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped", "delivered"],
                    "'delivered'", // new default uses newly added value
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate both actions
            assert_eq!(
                plan.actions.len(),
                2,
                "Expected 2 actions, got: {:?}",
                plan.actions
            );

            // ModifyColumnType should come FIRST (to add the new enum value)
            assert!(
                matches!(&plan.actions[0], MigrationAction::ModifyColumnType { table, column, .. }
                    if table == "orders" && column == "status"),
                "First action should be ModifyColumnType, got: {:?}",
                plan.actions[0]
            );

            // ModifyColumnDefault should come SECOND
            assert!(
                matches!(&plan.actions[1], MigrationAction::ModifyColumnDefault { table, column, .. }
                    if table == "orders" && column == "status"),
                "Second action should be ModifyColumnDefault, got: {:?}",
                plan.actions[1]
            );
        }

        #[test]
        fn enum_remove_value_that_was_default() {
            // Case 2: Remove enum value that was the default
            // Expected order: ModifyColumnDefault FIRST (change away from removed value),
            //                 then ModifyColumnType (remove the value)
            let from = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped", "cancelled"],
                    "'cancelled'", // default is 'cancelled' which will be removed
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped"], // 'cancelled' removed
                    "'pending'",                // default changed to existing value
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate both actions
            assert_eq!(
                plan.actions.len(),
                2,
                "Expected 2 actions, got: {:?}",
                plan.actions
            );

            // ModifyColumnDefault should come FIRST (change default before removing enum value)
            assert!(
                matches!(&plan.actions[0], MigrationAction::ModifyColumnDefault { table, column, .. }
                    if table == "orders" && column == "status"),
                "First action should be ModifyColumnDefault, got: {:?}",
                plan.actions[0]
            );

            // ModifyColumnType should come SECOND (now safe to remove enum value)
            assert!(
                matches!(&plan.actions[1], MigrationAction::ModifyColumnType { table, column, .. }
                    if table == "orders" && column == "status"),
                "Second action should be ModifyColumnType, got: {:?}",
                plan.actions[1]
            );
        }

        #[test]
        fn enum_remove_value_default_unchanged() {
            // Remove enum value, but default was NOT that value (no reordering needed)
            let from = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped", "cancelled"],
                    "'pending'", // default is 'pending', not the removed 'cancelled'
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped"], // 'cancelled' removed
                    "'pending'",                // default unchanged
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate only ModifyColumnType (default unchanged)
            assert_eq!(
                plan.actions.len(),
                1,
                "Expected 1 action, got: {:?}",
                plan.actions
            );
            assert!(
                matches!(&plan.actions[0], MigrationAction::ModifyColumnType { table, column, .. }
                    if table == "orders" && column == "status"),
                "Action should be ModifyColumnType, got: {:?}",
                plan.actions[0]
            );
        }

        #[test]
        fn enum_remove_value_with_default_change_to_remaining() {
            // Remove multiple enum values, old default was one of them, new default is a remaining value
            let from = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["draft", "pending", "shipped", "delivered", "cancelled"],
                    "'cancelled'",
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped", "delivered"], // removed 'draft' and 'cancelled'
                    "'pending'",
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(
                plan.actions.len(),
                2,
                "Expected 2 actions, got: {:?}",
                plan.actions
            );

            // ModifyColumnDefault MUST come first because old default 'cancelled' is being removed
            assert!(
                matches!(
                    &plan.actions[0],
                    MigrationAction::ModifyColumnDefault { .. }
                ),
                "First action should be ModifyColumnDefault, got: {:?}",
                plan.actions[0]
            );
            assert!(
                matches!(&plan.actions[1], MigrationAction::ModifyColumnType { .. }),
                "Second action should be ModifyColumnType, got: {:?}",
                plan.actions[1]
            );
        }

        #[test]
        fn enum_remove_value_with_unquoted_default() {
            // Test coverage for extract_unquoted_default else branch (line 335)
            // When default value doesn't have quotes, it should still be compared correctly
            let from = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped", "cancelled"],
                    "cancelled", // unquoted default (no single quotes)
                )],
                vec![],
            )];

            let to = vec![table(
                "orders",
                vec![col_enum_with_default(
                    "status",
                    "order_status",
                    vec!["pending", "shipped"], // 'cancelled' removed
                    "pending",                  // unquoted default
                )],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate both actions
            assert_eq!(
                plan.actions.len(),
                2,
                "Expected 2 actions, got: {:?}",
                plan.actions
            );

            // ModifyColumnDefault should come FIRST because unquoted 'cancelled' matches removed value
            assert!(
                matches!(
                    &plan.actions[0],
                    MigrationAction::ModifyColumnDefault { .. }
                ),
                "First action should be ModifyColumnDefault, got: {:?}",
                plan.actions[0]
            );
            assert!(
                matches!(&plan.actions[1], MigrationAction::ModifyColumnType { .. }),
                "Second action should be ModifyColumnType, got: {:?}",
                plan.actions[1]
            );
        }
    }

    // Tests for inline column constraints normalization
    mod inline_constraints {
        use super::*;
        use vespertide_core::schema::foreign_key::ForeignKeyDef;
        use vespertide_core::schema::foreign_key::ForeignKeySyntax;
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        use vespertide_core::{StrOrBoolOrArray, TableConstraint};

        fn col_with_pk(name: &str, ty: ColumnType) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            }
        }

        fn col_with_unique(name: &str, ty: ColumnType) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: Some(StrOrBoolOrArray::Bool(true)),
                index: None,
                foreign_key: None,
            }
        }

        fn col_with_index(name: &str, ty: ColumnType) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: Some(StrOrBoolOrArray::Bool(true)),
                foreign_key: None,
            }
        }

        fn col_with_fk(name: &str, ty: ColumnType, ref_table: &str, ref_col: &str) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: ref_table.to_string(),
                    ref_columns: vec![ref_col.to_string()],
                    on_delete: None,
                    on_update: None,
                })),
            }
        }

        #[test]
        fn create_table_with_inline_pk() {
            let plan = diff_schemas(
                &[],
                &[table(
                    "users",
                    vec![
                        col_with_pk("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                )],
            )
            .unwrap();

            // Inline PK should be preserved in column definition
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable {
                columns,
                constraints,
                ..
            } = &plan.actions[0]
            {
                // Constraints should be empty (inline PK not moved here)
                assert_eq!(constraints.len(), 0);
                // Check that the column has inline PK
                let id_col = columns.iter().find(|c| c.name == "id").unwrap();
                assert!(id_col.primary_key.is_some());
            } else {
                panic!("Expected CreateTable action");
            }
        }

        #[test]
        fn create_table_with_inline_unique() {
            let plan = diff_schemas(
                &[],
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_unique("email", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                )],
            )
            .unwrap();

            // Inline unique should be preserved in column definition
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable {
                columns,
                constraints,
                ..
            } = &plan.actions[0]
            {
                // Constraints should be empty (inline unique not moved here)
                assert_eq!(constraints.len(), 0);
                // Check that the column has inline unique
                let email_col = columns.iter().find(|c| c.name == "email").unwrap();
                assert!(matches!(
                    email_col.unique,
                    Some(StrOrBoolOrArray::Bool(true))
                ));
            } else {
                panic!("Expected CreateTable action");
            }
        }

        #[test]
        fn create_table_with_inline_index() {
            let plan = diff_schemas(
                &[],
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_index("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                )],
            )
            .unwrap();

            // Inline index should be preserved in column definition, not moved to constraints
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable {
                columns,
                constraints,
                ..
            } = &plan.actions[0]
            {
                // Constraints should be empty (inline index not moved here)
                assert_eq!(constraints.len(), 0);
                // Check that the column has inline index
                let name_col = columns.iter().find(|c| c.name == "name").unwrap();
                assert!(matches!(name_col.index, Some(StrOrBoolOrArray::Bool(true))));
            } else {
                panic!("Expected CreateTable action");
            }
        }

        #[test]
        fn create_table_with_inline_fk() {
            let plan = diff_schemas(
                &[],
                &[table(
                    "posts",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_fk(
                            "user_id",
                            ColumnType::Simple(SimpleColumnType::Integer),
                            "users",
                            "id",
                        ),
                    ],
                    vec![],
                )],
            )
            .unwrap();

            // Inline FK should be preserved in column definition
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable {
                columns,
                constraints,
                ..
            } = &plan.actions[0]
            {
                // Constraints should be empty (inline FK not moved here)
                assert_eq!(constraints.len(), 0);
                // Check that the column has inline FK
                let user_id_col = columns.iter().find(|c| c.name == "user_id").unwrap();
                assert!(user_id_col.foreign_key.is_some());
            } else {
                panic!("Expected CreateTable action");
            }
        }

        #[test]
        fn add_index_via_inline_constraint() {
            // Existing table without index -> table with inline index
            // Inline index (Bool(true)) is normalized to a named table-level constraint
            let plan = diff_schemas(
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                )],
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_index("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                )],
            )
            .unwrap();

            // Should generate AddConstraint with name: None (auto-generated indexes)
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::AddConstraint { table, constraint } = &plan.actions[0] {
                assert_eq!(table, "users");
                if let TableConstraint::Index { name, columns } = constraint {
                    assert_eq!(name, &None); // Auto-generated indexes use None
                    assert_eq!(columns, &vec!["name".to_string()]);
                } else {
                    panic!("Expected Index constraint, got {:?}", constraint);
                }
            } else {
                panic!("Expected AddConstraint action, got {:?}", plan.actions[0]);
            }
        }

        #[test]
        fn create_table_with_all_inline_constraints() {
            let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
            id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));
            id_col.nullable = false;

            let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
            email_col.unique = Some(StrOrBoolOrArray::Bool(true));

            let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
            name_col.index = Some(StrOrBoolOrArray::Bool(true));

            let mut org_id_col = col("org_id", ColumnType::Simple(SimpleColumnType::Integer));
            org_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
                ref_table: "orgs".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }));

            let plan = diff_schemas(
                &[],
                &[table(
                    "users",
                    vec![id_col, email_col, name_col, org_id_col],
                    vec![],
                )],
            )
            .unwrap();

            // All inline constraints should be preserved in column definitions
            assert_eq!(plan.actions.len(), 1);

            if let MigrationAction::CreateTable {
                columns,
                constraints,
                ..
            } = &plan.actions[0]
            {
                // Constraints should be empty (all inline)
                assert_eq!(constraints.len(), 0);

                // Check each column has its inline constraint
                let id_col = columns.iter().find(|c| c.name == "id").unwrap();
                assert!(id_col.primary_key.is_some());

                let email_col = columns.iter().find(|c| c.name == "email").unwrap();
                assert!(matches!(
                    email_col.unique,
                    Some(StrOrBoolOrArray::Bool(true))
                ));

                let name_col = columns.iter().find(|c| c.name == "name").unwrap();
                assert!(matches!(name_col.index, Some(StrOrBoolOrArray::Bool(true))));

                let org_id_col = columns.iter().find(|c| c.name == "org_id").unwrap();
                assert!(org_id_col.foreign_key.is_some());
            } else {
                panic!("Expected CreateTable action");
            }
        }

        #[test]
        fn add_constraint_to_existing_table() {
            // Add a unique constraint to an existing table
            let from_schema = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            )];

            let to_schema = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![vespertide_core::TableConstraint::Unique {
                    name: Some("uq_users_email".into()),
                    columns: vec!["email".into()],
                }],
            )];

            let plan = diff_schemas(&from_schema, &to_schema).unwrap();
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::AddConstraint { table, constraint } = &plan.actions[0] {
                assert_eq!(table, "users");
                assert!(matches!(
                    constraint,
                    vespertide_core::TableConstraint::Unique { name: Some(n), columns }
                        if n == "uq_users_email" && columns == &vec!["email".to_string()]
                ));
            } else {
                panic!("Expected AddConstraint action, got {:?}", plan.actions[0]);
            }
        }

        #[test]
        fn remove_constraint_from_existing_table() {
            // Remove a unique constraint from an existing table
            let from_schema = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![vespertide_core::TableConstraint::Unique {
                    name: Some("uq_users_email".into()),
                    columns: vec!["email".into()],
                }],
            )];

            let to_schema = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from_schema, &to_schema).unwrap();
            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::RemoveConstraint { table, constraint } = &plan.actions[0] {
                assert_eq!(table, "users");
                assert!(matches!(
                    constraint,
                    vespertide_core::TableConstraint::Unique { name: Some(n), columns }
                        if n == "uq_users_email" && columns == &vec!["email".to_string()]
                ));
            } else {
                panic!(
                    "Expected RemoveConstraint action, got {:?}",
                    plan.actions[0]
                );
            }
        }

        #[test]
        fn diff_schemas_with_normalize_error() {
            // Test that normalize errors are properly propagated
            let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
            col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

            let table = TableDef {
                name: "test".into(),
                description: None,
                columns: vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col1.clone(),
                    {
                        // Same column with same index name - should error
                        let mut c = col1.clone();
                        c.index = Some(StrOrBoolOrArray::Str("idx1".into()));
                        c
                    },
                ],
                constraints: vec![],
            };

            let result = diff_schemas(&[], &[table]);
            assert!(result.is_err());
            if let Err(PlannerError::TableValidation(msg)) = result {
                assert!(msg.contains("Failed to normalize table"));
                assert!(msg.contains("Duplicate index"));
            } else {
                panic!("Expected TableValidation error, got {:?}", result);
            }
        }

        #[test]
        fn diff_schemas_with_normalize_error_in_from_schema() {
            // Test that normalize errors in 'from' schema are properly propagated
            let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
            col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

            let table = TableDef {
                name: "test".into(),
                description: None,
                columns: vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col1.clone(),
                    {
                        // Same column with same index name - should error
                        let mut c = col1.clone();
                        c.index = Some(StrOrBoolOrArray::Str("idx1".into()));
                        c
                    },
                ],
                constraints: vec![],
            };

            // 'from' schema has the invalid table
            let result = diff_schemas(&[table], &[]);
            assert!(result.is_err());
            if let Err(PlannerError::TableValidation(msg)) = result {
                assert!(msg.contains("Failed to normalize table"));
                assert!(msg.contains("Duplicate index"));
            } else {
                panic!("Expected TableValidation error, got {:?}", result);
            }
        }
    }

    // Direct unit tests for sort_create_before_add_constraint and compare_actions_for_create_order
    mod sort_create_before_add_constraint_tests {
        use super::*;
        use crate::diff::{compare_actions_for_create_order, sort_create_before_add_constraint};
        use std::cmp::Ordering;

        fn make_add_column(table: &str, col: &str) -> MigrationAction {
            MigrationAction::AddColumn {
                table: table.into(),
                column: Box::new(ColumnDef {
                    name: col.into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }
        }

        fn make_create_table(name: &str) -> MigrationAction {
            MigrationAction::CreateTable {
                table: name.into(),
                columns: vec![],
                constraints: vec![],
            }
        }

        fn make_add_fk(table: &str, ref_table: &str) -> MigrationAction {
            MigrationAction::AddConstraint {
                table: table.into(),
                constraint: TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["fk_col".into()],
                    ref_table: ref_table.into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            }
        }

        /// Test line 218: (false, true, _, _) - a is NOT CreateTable, b IS CreateTable
        /// Direct test of comparison function
        #[test]
        fn test_compare_non_create_vs_create() {
            let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

            let add_col = make_add_column("users", "name");
            let create_table = make_create_table("roles");

            // a=AddColumn (non-create), b=CreateTable (create) -> Greater (b comes first)
            let result = compare_actions_for_create_order(&add_col, &create_table, &created_tables);
            assert_eq!(
                result,
                Ordering::Greater,
                "Non-CreateTable vs CreateTable should return Greater"
            );
        }

        /// Test line 216: (true, false, _, _) - a IS CreateTable, b is NOT CreateTable
        #[test]
        fn test_compare_create_vs_non_create() {
            let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

            let create_table = make_create_table("roles");
            let add_col = make_add_column("users", "name");

            // a=CreateTable (create), b=AddColumn (non-create) -> Less (a comes first)
            let result = compare_actions_for_create_order(&create_table, &add_col, &created_tables);
            assert_eq!(
                result,
                Ordering::Less,
                "CreateTable vs Non-CreateTable should return Less"
            );
        }

        /// Test line 214: (true, true, _, _) - both CreateTable
        #[test]
        fn test_compare_create_vs_create() {
            let created_tables: BTreeSet<String> = ["roles".to_string(), "categories".to_string()]
                .into_iter()
                .collect();

            let create1 = make_create_table("roles");
            let create2 = make_create_table("categories");

            // Both CreateTable -> Equal (maintain original order)
            let result = compare_actions_for_create_order(&create1, &create2, &created_tables);
            assert_eq!(
                result,
                Ordering::Equal,
                "CreateTable vs CreateTable should return Equal"
            );
        }

        /// Test line 221: (false, false, true, false) - neither CreateTable, a refs created, b doesn't
        #[test]
        fn test_compare_refs_vs_non_refs() {
            let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

            let add_fk = make_add_fk("users", "roles"); // refs created
            let add_col = make_add_column("posts", "title"); // doesn't ref

            // a refs created, b doesn't -> Greater (a comes after)
            let result = compare_actions_for_create_order(&add_fk, &add_col, &created_tables);
            assert_eq!(
                result,
                Ordering::Greater,
                "FK-ref vs non-ref should return Greater"
            );
        }

        /// Test line 223: (false, false, false, true) - neither CreateTable, a doesn't ref, b refs
        #[test]
        fn test_compare_non_refs_vs_refs() {
            let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

            let add_col = make_add_column("posts", "title"); // doesn't ref
            let add_fk = make_add_fk("users", "roles"); // refs created

            // a doesn't ref, b refs -> Less (b comes after, a comes first)
            let result = compare_actions_for_create_order(&add_col, &add_fk, &created_tables);
            assert_eq!(
                result,
                Ordering::Less,
                "Non-ref vs FK-ref should return Less"
            );
        }

        /// Test line 225: (false, false, _, _) - neither CreateTable, both don't ref
        #[test]
        fn test_compare_non_refs_vs_non_refs() {
            let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

            let add_col1 = make_add_column("users", "name");
            let add_col2 = make_add_column("posts", "title");

            // Both don't ref -> Equal
            let result = compare_actions_for_create_order(&add_col1, &add_col2, &created_tables);
            assert_eq!(
                result,
                Ordering::Equal,
                "Non-ref vs non-ref should return Equal"
            );
        }

        /// Test line 225: (false, false, _, _) - neither CreateTable, both ref created
        #[test]
        fn test_compare_refs_vs_refs() {
            let created_tables: BTreeSet<String> = ["roles".to_string(), "categories".to_string()]
                .into_iter()
                .collect();

            let add_fk1 = make_add_fk("users", "roles");
            let add_fk2 = make_add_fk("posts", "categories");

            // Both ref -> Equal
            let result = compare_actions_for_create_order(&add_fk1, &add_fk2, &created_tables);
            assert_eq!(
                result,
                Ordering::Equal,
                "FK-ref vs FK-ref should return Equal"
            );
        }

        /// Integration test: sort function works correctly
        #[test]
        fn test_sort_integration() {
            let mut actions = vec![
                make_add_column("t1", "c1"),
                make_add_fk("users", "roles"),
                make_create_table("roles"),
            ];

            sort_create_before_add_constraint(&mut actions);

            // CreateTable first, AddColumn second, AddConstraint FK last
            assert!(matches!(&actions[0], MigrationAction::CreateTable { .. }));
            assert!(matches!(&actions[1], MigrationAction::AddColumn { .. }));
            assert!(matches!(&actions[2], MigrationAction::AddConstraint { .. }));
        }
    }

    // Tests for foreign key dependency ordering
    mod fk_ordering {
        use super::*;
        use vespertide_core::TableConstraint;

        fn table_with_fk(
            name: &str,
            ref_table: &str,
            fk_column: &str,
            ref_column: &str,
        ) -> TableDef {
            TableDef {
                name: name.to_string(),
                description: None,
                columns: vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col(fk_column, ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                constraints: vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec![fk_column.to_string()],
                    ref_table: ref_table.to_string(),
                    ref_columns: vec![ref_column.to_string()],
                    on_delete: None,
                    on_update: None,
                }],
            }
        }

        fn simple_table(name: &str) -> TableDef {
            TableDef {
                name: name.to_string(),
                description: None,
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
            }
        }

        #[test]
        fn create_tables_respects_fk_order() {
            // Create users and posts tables where posts references users
            // The order should be: users first, then posts
            let users = simple_table("users");
            let posts = table_with_fk("posts", "users", "user_id", "id");

            let plan = diff_schemas(&[], &[posts.clone(), users.clone()]).unwrap();

            // Extract CreateTable actions in order
            let create_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::CreateTable { table, .. } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            assert_eq!(create_order, vec!["users", "posts"]);
        }

        #[test]
        fn create_tables_chain_dependency() {
            // Chain: users <- media <- articles
            // users has no FK
            // media references users
            // articles references media
            let users = simple_table("users");
            let media = table_with_fk("media", "users", "owner_id", "id");
            let articles = table_with_fk("articles", "media", "media_id", "id");

            // Pass in reverse order to ensure sorting works
            let plan =
                diff_schemas(&[], &[articles.clone(), media.clone(), users.clone()]).unwrap();

            let create_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::CreateTable { table, .. } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            assert_eq!(create_order, vec!["users", "media", "articles"]);
        }

        #[test]
        fn create_tables_multiple_independent_branches() {
            // Two independent branches:
            // users <- posts
            // categories <- products
            let users = simple_table("users");
            let posts = table_with_fk("posts", "users", "user_id", "id");
            let categories = simple_table("categories");
            let products = table_with_fk("products", "categories", "category_id", "id");

            let plan = diff_schemas(
                &[],
                &[
                    products.clone(),
                    posts.clone(),
                    categories.clone(),
                    users.clone(),
                ],
            )
            .unwrap();

            let create_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::CreateTable { table, .. } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            // users must come before posts
            let users_pos = create_order.iter().position(|&t| t == "users").unwrap();
            let posts_pos = create_order.iter().position(|&t| t == "posts").unwrap();
            assert!(
                users_pos < posts_pos,
                "users should be created before posts"
            );

            // categories must come before products
            let categories_pos = create_order
                .iter()
                .position(|&t| t == "categories")
                .unwrap();
            let products_pos = create_order.iter().position(|&t| t == "products").unwrap();
            assert!(
                categories_pos < products_pos,
                "categories should be created before products"
            );
        }

        #[test]
        fn delete_tables_respects_fk_order() {
            // When deleting users and posts where posts references users,
            // posts should be deleted first (reverse of creation order)
            let users = simple_table("users");
            let posts = table_with_fk("posts", "users", "user_id", "id");

            let plan = diff_schemas(&[users.clone(), posts.clone()], &[]).unwrap();

            let delete_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::DeleteTable { table } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            assert_eq!(delete_order, vec!["posts", "users"]);
        }

        #[test]
        fn delete_tables_chain_dependency() {
            // Chain: users <- media <- articles
            // Delete order should be: articles, media, users
            let users = simple_table("users");
            let media = table_with_fk("media", "users", "owner_id", "id");
            let articles = table_with_fk("articles", "media", "media_id", "id");

            let plan =
                diff_schemas(&[users.clone(), media.clone(), articles.clone()], &[]).unwrap();

            let delete_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::DeleteTable { table } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            // articles must be deleted before media
            let articles_pos = delete_order.iter().position(|&t| t == "articles").unwrap();
            let media_pos = delete_order.iter().position(|&t| t == "media").unwrap();
            assert!(
                articles_pos < media_pos,
                "articles should be deleted before media"
            );

            // media must be deleted before users
            let users_pos = delete_order.iter().position(|&t| t == "users").unwrap();
            assert!(
                media_pos < users_pos,
                "media should be deleted before users"
            );
        }

        #[test]
        fn circular_fk_dependency_returns_error() {
            // Create circular dependency: A -> B -> A
            let table_a = TableDef {
                name: "table_a".to_string(),
                description: None,
                columns: vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("b_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                constraints: vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["b_id".to_string()],
                    ref_table: "table_b".to_string(),
                    ref_columns: vec!["id".to_string()],
                    on_delete: None,
                    on_update: None,
                }],
            };

            let table_b = TableDef {
                name: "table_b".to_string(),
                description: None,
                columns: vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("a_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                constraints: vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["a_id".to_string()],
                    ref_table: "table_a".to_string(),
                    ref_columns: vec!["id".to_string()],
                    on_delete: None,
                    on_update: None,
                }],
            };

            let result = diff_schemas(&[], &[table_a, table_b]);
            assert!(result.is_err());
            if let Err(PlannerError::TableValidation(msg)) = result {
                assert!(
                    msg.contains("Circular foreign key dependency"),
                    "Expected circular dependency error, got: {}",
                    msg
                );
            } else {
                panic!("Expected TableValidation error, got {:?}", result);
            }
        }

        #[test]
        fn fk_to_external_table_is_ignored() {
            // FK referencing a table not in the migration should not affect ordering
            let posts = table_with_fk("posts", "users", "user_id", "id");
            let comments = table_with_fk("comments", "posts", "post_id", "id");

            // users is NOT being created in this migration
            let plan = diff_schemas(&[], &[comments.clone(), posts.clone()]).unwrap();

            let create_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::CreateTable { table, .. } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            // posts must come before comments (comments depends on posts)
            let posts_pos = create_order.iter().position(|&t| t == "posts").unwrap();
            let comments_pos = create_order.iter().position(|&t| t == "comments").unwrap();
            assert!(
                posts_pos < comments_pos,
                "posts should be created before comments"
            );
        }

        #[test]
        fn delete_tables_mixed_with_other_actions() {
            // Test that sort_delete_actions correctly handles actions that are not DeleteTable
            // This tests lines 124, 193, 198 (the else branches)
            use crate::diff::diff_schemas;

            let from_schema = vec![
                table(
                    "users",
                    vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    vec![],
                ),
                table(
                    "posts",
                    vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    vec![],
                ),
            ];

            let to_schema = vec![
                // Drop posts table, but also add a new column to users
                table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                ),
            ];

            let plan = diff_schemas(&from_schema, &to_schema).unwrap();

            // Should have: AddColumn (for users.name) and DeleteTable (for posts)
            assert!(
                plan.actions
                    .iter()
                    .any(|a| matches!(a, MigrationAction::AddColumn { .. }))
            );
            assert!(
                plan.actions
                    .iter()
                    .any(|a| matches!(a, MigrationAction::DeleteTable { .. }))
            );

            // The else branches in sort_delete_actions should handle AddColumn gracefully
            // (returning empty string for table name, which sorts it to position 0)
        }

        #[test]
        #[should_panic(expected = "Expected DeleteTable action")]
        fn test_extract_delete_table_name_panics_on_non_delete_action() {
            // Test that extract_delete_table_name panics when called with non-DeleteTable action
            use super::extract_delete_table_name;

            let action = MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            };

            // This should panic
            extract_delete_table_name(&action);
        }

        /// Test that inline FK across multiple tables works correctly with topological sort
        #[test]
        fn create_tables_with_inline_fk_chain() {
            use super::*;
            use vespertide_core::schema::foreign_key::ForeignKeySyntax;
            use vespertide_core::schema::primary_key::PrimaryKeySyntax;

            fn col_pk(name: &str) -> ColumnDef {
                ColumnDef {
                    name: name.to_string(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: None,
                    foreign_key: None,
                }
            }

            fn col_inline_fk(name: &str, ref_table: &str) -> ColumnDef {
                ColumnDef {
                    name: name.to_string(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: Some(ForeignKeySyntax::String(format!("{}.id", ref_table))),
                }
            }

            // Reproduce the app example structure:
            // user -> (no deps)
            // product -> (no deps)
            // project -> user
            // code -> product, user, project
            // order -> user, project, product, code
            // payment -> order

            let user = TableDef {
                name: "user".to_string(),
                description: None,
                columns: vec![col_pk("id")],
                constraints: vec![],
            };

            let product = TableDef {
                name: "product".to_string(),
                description: None,
                columns: vec![col_pk("id")],
                constraints: vec![],
            };

            let project = TableDef {
                name: "project".to_string(),
                description: None,
                columns: vec![col_pk("id"), col_inline_fk("user_id", "user")],
                constraints: vec![],
            };

            let code = TableDef {
                name: "code".to_string(),
                description: None,
                columns: vec![
                    col_pk("id"),
                    col_inline_fk("product_id", "product"),
                    col_inline_fk("creator_user_id", "user"),
                    col_inline_fk("project_id", "project"),
                ],
                constraints: vec![],
            };

            let order = TableDef {
                name: "order".to_string(),
                description: None,
                columns: vec![
                    col_pk("id"),
                    col_inline_fk("user_id", "user"),
                    col_inline_fk("project_id", "project"),
                    col_inline_fk("product_id", "product"),
                    col_inline_fk("code_id", "code"),
                ],
                constraints: vec![],
            };

            let payment = TableDef {
                name: "payment".to_string(),
                description: None,
                columns: vec![col_pk("id"), col_inline_fk("order_id", "order")],
                constraints: vec![],
            };

            // Pass in arbitrary order - should NOT return circular dependency error
            let result = diff_schemas(&[], &[payment, order, code, project, product, user]);
            assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

            let plan = result.unwrap();
            let create_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::CreateTable { table, .. } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            // Verify order respects FK dependencies
            let get_pos = |name: &str| create_order.iter().position(|&t| t == name).unwrap();

            // user and product have no deps, can be in any order
            // project depends on user
            assert!(
                get_pos("user") < get_pos("project"),
                "user must come before project"
            );
            // code depends on product, user, project
            assert!(
                get_pos("product") < get_pos("code"),
                "product must come before code"
            );
            assert!(
                get_pos("user") < get_pos("code"),
                "user must come before code"
            );
            assert!(
                get_pos("project") < get_pos("code"),
                "project must come before code"
            );
            // order depends on user, project, product, code
            assert!(
                get_pos("code") < get_pos("order"),
                "code must come before order"
            );
            // payment depends on order
            assert!(
                get_pos("order") < get_pos("payment"),
                "order must come before payment"
            );
        }

        /// Test that AddConstraint FK to a new table comes AFTER CreateTable for that table
        #[test]
        fn add_constraint_fk_to_new_table_comes_after_create_table() {
            use super::*;

            // Existing table: notification (with broadcast_id column)
            let notification_from = table(
                "notification",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col(
                        "broadcast_id",
                        ColumnType::Simple(SimpleColumnType::Integer),
                    ),
                ],
                vec![],
            );

            // New table: notification_broadcast
            let notification_broadcast = table(
                "notification_broadcast",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            );

            // Modified notification with FK constraint to the new table
            let notification_to = table(
                "notification",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col(
                        "broadcast_id",
                        ColumnType::Simple(SimpleColumnType::Integer),
                    ),
                ],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["broadcast_id".into()],
                    ref_table: "notification_broadcast".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            );

            let from_schema = vec![notification_from];
            let to_schema = vec![notification_to, notification_broadcast];

            let plan = diff_schemas(&from_schema, &to_schema).unwrap();

            // Find positions
            let create_pos = plan.actions.iter().position(|a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "notification_broadcast"));
            let add_constraint_pos = plan.actions.iter().position(|a| {
                matches!(a, MigrationAction::AddConstraint {
                    constraint: TableConstraint::ForeignKey { ref_table, .. }, ..
                } if ref_table == "notification_broadcast")
            });

            assert!(
                create_pos.is_some(),
                "Should have CreateTable for notification_broadcast"
            );
            assert!(
                add_constraint_pos.is_some(),
                "Should have AddConstraint for FK to notification_broadcast"
            );
            assert!(
                create_pos.unwrap() < add_constraint_pos.unwrap(),
                "CreateTable must come BEFORE AddConstraint FK that references it. Got CreateTable at {}, AddConstraint at {}",
                create_pos.unwrap(),
                add_constraint_pos.unwrap()
            );
        }

        /// Test sort_create_before_add_constraint with multiple action types
        /// Covers lines 218, 221, 223, 225 in sort_create_before_add_constraint
        #[test]
        fn sort_create_before_add_constraint_all_branches() {
            use super::*;

            // Scenario: Existing table gets modified (column change) AND gets FK to new table
            // Plus another existing table gets a regular index added (not FK to new table)
            // This creates:
            // - ModifyColumnComment (doesn't ref created table)
            // - AddConstraint FK (refs created table)
            // - AddConstraint Index (doesn't ref created table)
            // - CreateTable

            let users_from = table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    {
                        let mut c = col("name", ColumnType::Simple(SimpleColumnType::Text));
                        c.comment = Some("Old comment".into());
                        c
                    },
                    col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![],
            );

            let posts_from = table(
                "posts",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("title", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            );

            // New table: roles
            let roles = table(
                "roles",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            );

            // Modified users: comment change + FK to new roles table
            let users_to = table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    {
                        let mut c = col("name", ColumnType::Simple(SimpleColumnType::Text));
                        c.comment = Some("New comment".into());
                        c
                    },
                    col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["role_id".into()],
                    ref_table: "roles".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            );

            // Modified posts: add index (not FK to new table)
            let posts_to = table(
                "posts",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("title", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![TableConstraint::Index {
                    name: Some("idx_title".into()),
                    columns: vec!["title".into()],
                }],
            );

            let from_schema = vec![users_from, posts_from];
            let to_schema = vec![users_to, posts_to, roles];

            let plan = diff_schemas(&from_schema, &to_schema).unwrap();

            // Verify CreateTable comes first
            let create_pos = plan
                .actions
                .iter()
                .position(
                    |a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "roles"),
                )
                .expect("Should have CreateTable for roles");

            // ModifyColumnComment should come after CreateTable (line 218: non-create vs create)
            let modify_pos = plan
                .actions
                .iter()
                .position(|a| matches!(a, MigrationAction::ModifyColumnComment { .. }))
                .expect("Should have ModifyColumnComment");

            // AddConstraint Index (not FK to created) should come after CreateTable (line 218)
            let add_index_pos = plan
                .actions
                .iter()
                .position(|a| {
                    matches!(
                        a,
                        MigrationAction::AddConstraint {
                            constraint: TableConstraint::Index { .. },
                            ..
                        }
                    )
                })
                .expect("Should have AddConstraint Index");

            // AddConstraint FK to roles should come last (line 221: refs created, others don't)
            let add_fk_pos = plan
                .actions
                .iter()
                .position(|a| {
                    matches!(
                        a,
                        MigrationAction::AddConstraint {
                            constraint: TableConstraint::ForeignKey { ref_table, .. },
                            ..
                        } if ref_table == "roles"
                    )
                })
                .expect("Should have AddConstraint FK to roles");

            assert!(
                create_pos < modify_pos,
                "CreateTable must come before ModifyColumnComment"
            );
            assert!(
                create_pos < add_index_pos,
                "CreateTable must come before AddConstraint Index"
            );
            assert!(
                create_pos < add_fk_pos,
                "CreateTable must come before AddConstraint FK"
            );
            // FK to created table should come after non-FK-to-created actions
            assert!(
                add_index_pos < add_fk_pos,
                "AddConstraint Index (not referencing created) should come before AddConstraint FK (referencing created)"
            );
        }

        /// Test that two AddConstraint FKs both referencing created tables maintain stable order
        /// Covers line 225: both ref created tables
        #[test]
        fn sort_multiple_fks_to_created_tables() {
            use super::*;

            // Two existing tables, each getting FK to a different new table
            let users_from = table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![],
            );

            let posts_from = table(
                "posts",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("category_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![],
            );

            // Two new tables
            let roles = table(
                "roles",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            );
            let categories = table(
                "categories",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            );

            let users_to = table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["role_id".into()],
                    ref_table: "roles".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            );

            let posts_to = table(
                "posts",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("category_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["category_id".into()],
                    ref_table: "categories".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            );

            let from_schema = vec![users_from, posts_from];
            let to_schema = vec![users_to, posts_to, roles, categories];

            let plan = diff_schemas(&from_schema, &to_schema).unwrap();

            // Both CreateTable should come before both AddConstraint FK
            let create_roles_pos = plan.actions.iter().position(
                |a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "roles"),
            );
            let create_categories_pos = plan.actions.iter().position(|a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "categories"));
            let add_fk_roles_pos = plan.actions.iter().position(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        constraint: TableConstraint::ForeignKey { ref_table, .. },
                        ..
                    } if ref_table == "roles"
                )
            });
            let add_fk_categories_pos = plan.actions.iter().position(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        constraint: TableConstraint::ForeignKey { ref_table, .. },
                        ..
                    } if ref_table == "categories"
                )
            });

            assert!(create_roles_pos.is_some());
            assert!(create_categories_pos.is_some());
            assert!(add_fk_roles_pos.is_some());
            assert!(add_fk_categories_pos.is_some());

            // All CreateTable before all AddConstraint FK
            let max_create = create_roles_pos
                .unwrap()
                .max(create_categories_pos.unwrap());
            let min_add_fk = add_fk_roles_pos
                .unwrap()
                .min(add_fk_categories_pos.unwrap());
            assert!(
                max_create < min_add_fk,
                "All CreateTable actions must come before all AddConstraint FK actions"
            );
        }

        /// Test that multiple FKs to the same table are deduplicated correctly
        #[test]
        fn create_tables_with_duplicate_fk_references() {
            use super::*;
            use vespertide_core::schema::foreign_key::ForeignKeySyntax;
            use vespertide_core::schema::primary_key::PrimaryKeySyntax;

            fn col_pk(name: &str) -> ColumnDef {
                ColumnDef {
                    name: name.to_string(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: None,
                    foreign_key: None,
                }
            }

            fn col_inline_fk(name: &str, ref_table: &str) -> ColumnDef {
                ColumnDef {
                    name: name.to_string(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: Some(ForeignKeySyntax::String(format!("{}.id", ref_table))),
                }
            }

            // Table with multiple FKs referencing the same table (like code.creator_user_id and code.used_by_user_id)
            let user = TableDef {
                name: "user".to_string(),
                description: None,
                columns: vec![col_pk("id")],
                constraints: vec![],
            };

            let code = TableDef {
                name: "code".to_string(),
                description: None,
                columns: vec![
                    col_pk("id"),
                    col_inline_fk("creator_user_id", "user"),
                    col_inline_fk("used_by_user_id", "user"), // Second FK to same table
                ],
                constraints: vec![],
            };

            // This should NOT return circular dependency error even with duplicate FK refs
            let result = diff_schemas(&[], &[code, user]);
            assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

            let plan = result.unwrap();
            let create_order: Vec<&str> = plan
                .actions
                .iter()
                .filter_map(|a| {
                    if let MigrationAction::CreateTable { table, .. } = a {
                        Some(table.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            // user must come before code
            let user_pos = create_order.iter().position(|&t| t == "user").unwrap();
            let code_pos = create_order.iter().position(|&t| t == "code").unwrap();
            assert!(user_pos < code_pos, "user must come before code");
        }
    }

    mod primary_key_changes {
        use super::*;

        fn pk(columns: Vec<&str>) -> TableConstraint {
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: columns.into_iter().map(|s| s.to_string()).collect(),
            }
        }

        #[test]
        fn add_column_to_composite_pk() {
            // Primary key: [id] -> [id, tenant_id]
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id", "tenant_id"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should remove old PK and add new composite PK
            assert_eq!(plan.actions.len(), 2);

            let has_remove = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string()]
                )
            });
            assert!(has_remove, "Should have RemoveConstraint for old PK");

            let has_add = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string(), "tenant_id".to_string()]
                )
            });
            assert!(has_add, "Should have AddConstraint for new composite PK");
        }

        #[test]
        fn remove_column_from_composite_pk() {
            // Primary key: [id, tenant_id] -> [id]
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id", "tenant_id"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should remove old composite PK and add new single-column PK
            assert_eq!(plan.actions.len(), 2);

            let has_remove = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string(), "tenant_id".to_string()]
                )
            });
            assert!(
                has_remove,
                "Should have RemoveConstraint for old composite PK"
            );

            let has_add = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string()]
                )
            });
            assert!(
                has_add,
                "Should have AddConstraint for new single-column PK"
            );
        }

        #[test]
        fn change_pk_columns_entirely() {
            // Primary key: [id] -> [uuid]
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("uuid", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![pk(vec!["id"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("uuid", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![pk(vec!["uuid"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_remove = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string()]
                )
            });
            assert!(has_remove, "Should have RemoveConstraint for old PK");

            let has_add = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["uuid".to_string()]
                )
            });
            assert!(has_add, "Should have AddConstraint for new PK");
        }

        #[test]
        fn add_multiple_columns_to_composite_pk() {
            // Primary key: [id] -> [id, tenant_id, region_id]
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id", "tenant_id", "region_id"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_remove = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string()]
                )
            });
            assert!(
                has_remove,
                "Should have RemoveConstraint for old single-column PK"
            );

            let has_add = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec![
                        "id".to_string(),
                        "tenant_id".to_string(),
                        "region_id".to_string()
                    ]
                )
            });
            assert!(
                has_add,
                "Should have AddConstraint for new 3-column composite PK"
            );
        }

        #[test]
        fn remove_multiple_columns_from_composite_pk() {
            // Primary key: [id, tenant_id, region_id] -> [id]
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id", "tenant_id", "region_id"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_remove = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec![
                        "id".to_string(),
                        "tenant_id".to_string(),
                        "region_id".to_string()
                    ]
                )
            });
            assert!(
                has_remove,
                "Should have RemoveConstraint for old 3-column composite PK"
            );

            let has_add = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string()]
                )
            });
            assert!(
                has_add,
                "Should have AddConstraint for new single-column PK"
            );
        }

        #[test]
        fn change_composite_pk_columns_partially() {
            // Primary key: [id, tenant_id] -> [id, region_id]
            // One column kept, one removed, one added
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id", "tenant_id"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![pk(vec!["id", "region_id"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_remove = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string(), "tenant_id".to_string()]
                )
            });
            assert!(
                has_remove,
                "Should have RemoveConstraint for old PK with tenant_id"
            );

            let has_add = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::AddConstraint {
                        table,
                        constraint: TableConstraint::PrimaryKey { columns, .. }
                    } if table == "users" && columns == &vec!["id".to_string(), "region_id".to_string()]
                )
            });
            assert!(
                has_add,
                "Should have AddConstraint for new PK with region_id"
            );
        }
    }

    mod default_changes {
        use super::*;

        fn col_with_default(name: &str, ty: ColumnType, default: Option<&str>) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: true,
                default: default.map(|s| s.into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }
        }

        #[test]
        fn add_default_value() {
            // Column: no default -> has default
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default("status", ColumnType::Simple(SimpleColumnType::Text), None),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'active'"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default: Some(default),
                } if table == "users" && column == "status" && default == "'active'"
            ));
        }

        #[test]
        fn remove_default_value() {
            // Column: has default -> no default
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'active'"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default("status", ColumnType::Simple(SimpleColumnType::Text), None),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default: None,
                } if table == "users" && column == "status"
            ));
        }

        #[test]
        fn change_default_value() {
            // Column: 'active' -> 'pending'
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'active'"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'pending'"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default: Some(default),
                } if table == "users" && column == "status" && default == "'pending'"
            ));
        }

        #[test]
        fn no_change_same_default() {
            // Column: same default -> no action
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'active'"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'active'"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert!(plan.actions.is_empty());
        }

        #[test]
        fn multiple_columns_default_changes() {
            // Multiple columns with default changes
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default("status", ColumnType::Simple(SimpleColumnType::Text), None),
                    col_with_default(
                        "role",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'user'"),
                    ),
                    col_with_default(
                        "active",
                        ColumnType::Simple(SimpleColumnType::Boolean),
                        Some("true"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "status",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'pending'"),
                    ), // None -> 'pending'
                    col_with_default("role", ColumnType::Simple(SimpleColumnType::Text), None), // 'user' -> None
                    col_with_default(
                        "active",
                        ColumnType::Simple(SimpleColumnType::Boolean),
                        Some("true"),
                    ), // no change
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_status_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnDefault {
                        table,
                        column,
                        new_default: Some(default),
                    } if table == "users" && column == "status" && default == "'pending'"
                )
            });
            assert!(has_status_change, "Should detect status default added");

            let has_role_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnDefault {
                        table,
                        column,
                        new_default: None,
                    } if table == "users" && column == "role"
                )
            });
            assert!(has_role_change, "Should detect role default removed");
        }

        #[test]
        fn default_change_with_type_change() {
            // Column changing both type and default
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "count",
                        ColumnType::Simple(SimpleColumnType::Integer),
                        Some("0"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_default(
                        "count",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("'0'"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate both ModifyColumnType and ModifyColumnDefault
            assert_eq!(plan.actions.len(), 2);

            let has_type_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnType { table, column, .. }
                    if table == "users" && column == "count"
                )
            });
            assert!(has_type_change, "Should detect type change");

            let has_default_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnDefault {
                        table,
                        column,
                        new_default: Some(default),
                    } if table == "users" && column == "count" && default == "'0'"
                )
            });
            assert!(has_default_change, "Should detect default change");
        }
    }

    mod comment_changes {
        use super::*;

        fn col_with_comment(name: &str, ty: ColumnType, comment: Option<&str>) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: true,
                default: None,
                comment: comment.map(|s| s.to_string()),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }
        }

        #[test]
        fn add_comment() {
            // Column: no comment -> has comment
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("User's email address"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment: Some(comment),
                } if table == "users" && column == "email" && comment == "User's email address"
            ));
        }

        #[test]
        fn remove_comment() {
            // Column: has comment -> no comment
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("User's email address"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment: None,
                } if table == "users" && column == "email"
            ));
        }

        #[test]
        fn change_comment() {
            // Column: 'old comment' -> 'new comment'
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Old comment"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("New comment"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment: Some(comment),
                } if table == "users" && column == "email" && comment == "New comment"
            ));
        }

        #[test]
        fn no_change_same_comment() {
            // Column: same comment -> no action
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Same comment"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Same comment"),
                    ),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert!(plan.actions.is_empty());
        }

        #[test]
        fn multiple_columns_comment_changes() {
            // Multiple columns with comment changes
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None),
                    col_with_comment(
                        "name",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("User name"),
                    ),
                    col_with_comment(
                        "phone",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Phone number"),
                    ),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Email address"),
                    ), // None -> "Email address"
                    col_with_comment("name", ColumnType::Simple(SimpleColumnType::Text), None), // "User name" -> None
                    col_with_comment(
                        "phone",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Phone number"),
                    ), // no change
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_email_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnComment {
                        table,
                        column,
                        new_comment: Some(comment),
                    } if table == "users" && column == "email" && comment == "Email address"
                )
            });
            assert!(has_email_change, "Should detect email comment added");

            let has_name_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnComment {
                        table,
                        column,
                        new_comment: None,
                    } if table == "users" && column == "name"
                )
            });
            assert!(has_name_change, "Should detect name comment removed");
        }

        #[test]
        fn comment_change_with_nullable_change() {
            // Column changing both nullable and comment
            let from = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                    let mut c =
                        col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None);
                    c.nullable = true;
                    c
                }],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                    let mut c = col_with_comment(
                        "email",
                        ColumnType::Simple(SimpleColumnType::Text),
                        Some("Required email"),
                    );
                    c.nullable = false;
                    c
                }],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate both ModifyColumnNullable and ModifyColumnComment
            assert_eq!(plan.actions.len(), 2);

            let has_nullable_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnNullable {
                        table,
                        column,
                        nullable: false,
                        ..
                    } if table == "users" && column == "email"
                )
            });
            assert!(has_nullable_change, "Should detect nullable change");

            let has_comment_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnComment {
                        table,
                        column,
                        new_comment: Some(comment),
                    } if table == "users" && column == "email" && comment == "Required email"
                )
            });
            assert!(has_comment_change, "Should detect comment change");
        }
    }

    mod nullable_changes {
        use super::*;

        fn col_nullable(name: &str, ty: ColumnType, nullable: bool) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }
        }

        #[test]
        fn column_nullable_to_non_nullable() {
            // Column: nullable -> non-nullable
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), true),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), false),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnNullable {
                    table,
                    column,
                    nullable: false,
                    fill_with: None,
                } if table == "users" && column == "email"
            ));
        }

        #[test]
        fn column_non_nullable_to_nullable() {
            // Column: non-nullable -> nullable
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), false),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), true),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::ModifyColumnNullable {
                    table,
                    column,
                    nullable: true,
                    fill_with: None,
                } if table == "users" && column == "email"
            ));
        }

        #[test]
        fn multiple_columns_nullable_changes() {
            // Multiple columns changing nullability at once
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), true),
                    col_nullable("name", ColumnType::Simple(SimpleColumnType::Text), false),
                    col_nullable("phone", ColumnType::Simple(SimpleColumnType::Text), true),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), false), // nullable -> non-nullable
                    col_nullable("name", ColumnType::Simple(SimpleColumnType::Text), true), // non-nullable -> nullable
                    col_nullable("phone", ColumnType::Simple(SimpleColumnType::Text), true), // no change
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 2);

            let has_email_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnNullable {
                        table,
                        column,
                        nullable: false,
                        ..
                    } if table == "users" && column == "email"
                )
            });
            assert!(
                has_email_change,
                "Should detect email nullable -> non-nullable"
            );

            let has_name_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnNullable {
                        table,
                        column,
                        nullable: true,
                        ..
                    } if table == "users" && column == "name"
                )
            });
            assert!(
                has_name_change,
                "Should detect name non-nullable -> nullable"
            );
        }

        #[test]
        fn nullable_change_with_type_change() {
            // Column changing both type and nullability
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("age", ColumnType::Simple(SimpleColumnType::Integer), true),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col_nullable("age", ColumnType::Simple(SimpleColumnType::Text), false),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should generate both ModifyColumnType and ModifyColumnNullable
            assert_eq!(plan.actions.len(), 2);

            let has_type_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnType { table, column, .. }
                    if table == "users" && column == "age"
                )
            });
            assert!(has_type_change, "Should detect type change");

            let has_nullable_change = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::ModifyColumnNullable {
                        table,
                        column,
                        nullable: false,
                        ..
                    } if table == "users" && column == "age"
                )
            });
            assert!(has_nullable_change, "Should detect nullable change");
        }
    }

    mod diff_tables {
        use insta::assert_debug_snapshot;

        use super::*;

        #[test]
        fn create_table_with_inline_index() {
            let base = [table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: Some(StrOrBoolOrArray::Bool(false)),
                        foreign_key: None,
                    },
                    ColumnDef {
                        name: "name".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: true,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: Some(StrOrBoolOrArray::Bool(true)),
                        index: Some(StrOrBoolOrArray::Bool(true)),
                        foreign_key: None,
                    },
                ],
                vec![],
            )];
            let plan = diff_schemas(&[], &base).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert_debug_snapshot!(plan.actions);

            let plan = diff_schemas(
                &base,
                &[table(
                    "users",
                    vec![
                        ColumnDef {
                            name: "id".to_string(),
                            r#type: ColumnType::Simple(SimpleColumnType::Integer),
                            nullable: false,
                            default: None,
                            comment: None,
                            primary_key: Some(PrimaryKeySyntax::Bool(true)),
                            unique: None,
                            index: Some(StrOrBoolOrArray::Bool(false)),
                            foreign_key: None,
                        },
                        ColumnDef {
                            name: "name".to_string(),
                            r#type: ColumnType::Simple(SimpleColumnType::Text),
                            nullable: true,
                            default: None,
                            comment: None,
                            primary_key: None,
                            unique: Some(StrOrBoolOrArray::Bool(true)),
                            index: Some(StrOrBoolOrArray::Bool(false)),
                            foreign_key: None,
                        },
                    ],
                    vec![],
                )],
            )
            .unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert_debug_snapshot!(plan.actions);
        }

        #[rstest]
        #[case(
            "add_index",
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: None,
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: Some(StrOrBoolOrArray::Bool(true)),
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
        )]
        #[case(
            "remove_index",
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: Some(StrOrBoolOrArray::Bool(true)),
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: Some(StrOrBoolOrArray::Bool(false)),
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
        )]
        #[case(
            "add_named_index",
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: None,
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: Some(StrOrBoolOrArray::Str("hello".to_string())),
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
        )]
        #[case(
            "remove_named_index",
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: Some(StrOrBoolOrArray::Str("hello".to_string())),
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
            vec![table(
                "users",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: None,
                        foreign_key: None,
                    },
                ],
                vec![],
            )],
        )]
        fn diff_tables(#[case] name: &str, #[case] base: Vec<TableDef>, #[case] to: Vec<TableDef>) {
            use insta::with_settings;

            let plan = diff_schemas(&base, &to).unwrap();
            with_settings!({ snapshot_suffix => name }, {
                assert_debug_snapshot!(plan.actions);
            });
        }
    }

    // Explicit coverage tests for lines that tarpaulin might miss in rstest
    mod coverage_explicit {
        use super::*;

        #[test]
        fn delete_column_explicit() {
            // Covers lines 292-294: DeleteColumn action inside modified table loop
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("name", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::DeleteColumn { table, column }
                if table == "users" && column == "name"
            ));
        }

        #[test]
        fn add_column_explicit() {
            // Covers lines 359-362: AddColumn action inside modified table loop
            let from = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::AddColumn { table, column, .. }
                if table == "users" && column.name == "email"
            ));
        }

        #[test]
        fn remove_constraint_explicit() {
            // Covers lines 370-372: RemoveConstraint action inside modified table loop
            let from = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![idx("idx_users_id", vec!["id"])],
            )];

            let to = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::RemoveConstraint { table, constraint }
                if table == "users" && matches!(constraint, TableConstraint::Index { name: Some(n), .. } if n == "idx_users_id")
            ));
        }

        #[test]
        fn add_constraint_explicit() {
            // Covers lines 378-380: AddConstraint action inside modified table loop
            let from = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            )];

            let to = vec![table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![idx("idx_users_id", vec!["id"])],
            )];

            let plan = diff_schemas(&from, &to).unwrap();
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::AddConstraint { table, constraint }
                if table == "users" && matches!(constraint, TableConstraint::Index { name: Some(n), .. } if n == "idx_users_id")
            ));
        }
    }

    mod constraint_removal_on_deleted_columns {
        use super::*;

        fn fk(columns: Vec<&str>, ref_table: &str, ref_columns: Vec<&str>) -> TableConstraint {
            TableConstraint::ForeignKey {
                name: None,
                columns: columns.into_iter().map(|s| s.to_string()).collect(),
                ref_table: ref_table.to_string(),
                ref_columns: ref_columns.into_iter().map(|s| s.to_string()).collect(),
                on_delete: None,
                on_update: None,
            }
        }

        #[test]
        fn skip_remove_constraint_when_all_columns_deleted() {
            // When a column with FK and index is deleted, the constraints should NOT
            // generate separate RemoveConstraint actions (they are dropped with the column)
            let from = vec![table(
                "project",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("template_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![
                    fk(vec!["template_id"], "book_template", vec!["id"]),
                    idx("ix_project__template_id", vec!["template_id"]),
                ],
            )];

            let to = vec![table(
                "project",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should only have DeleteColumn, NO RemoveConstraint actions
            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::DeleteColumn { table, column }
                if table == "project" && column == "template_id"
            ));

            // Explicitly verify no RemoveConstraint
            let has_remove_constraint = plan
                .actions
                .iter()
                .any(|a| matches!(a, MigrationAction::RemoveConstraint { .. }));
            assert!(
                !has_remove_constraint,
                "Should NOT have RemoveConstraint when column is deleted"
            );
        }

        #[test]
        fn keep_remove_constraint_when_only_some_columns_deleted() {
            // If a composite constraint has some columns remaining, RemoveConstraint is needed
            let from = vec![table(
                "orders",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![idx(
                    "ix_orders__user_product",
                    vec!["user_id", "product_id"],
                )],
            )];

            let to = vec![table(
                "orders",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    // product_id is deleted, but user_id remains
                ],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should have both DeleteColumn AND RemoveConstraint
            // (because user_id is still there, the composite index needs explicit removal)
            let has_delete_column = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::DeleteColumn { table, column }
                    if table == "orders" && column == "product_id"
                )
            });
            assert!(has_delete_column, "Should have DeleteColumn for product_id");

            let has_remove_constraint = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    MigrationAction::RemoveConstraint { table, .. }
                    if table == "orders"
                )
            });
            assert!(
                has_remove_constraint,
                "Should have RemoveConstraint for composite index when only some columns deleted"
            );
        }

        #[test]
        fn skip_remove_constraint_when_all_composite_columns_deleted() {
            // If ALL columns of a composite constraint are deleted, skip RemoveConstraint
            let from = vec![table(
                "orders",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                ],
                vec![idx(
                    "ix_orders__user_product",
                    vec!["user_id", "product_id"],
                )],
            )];

            let to = vec![table(
                "orders",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            // Should only have DeleteColumn actions, no RemoveConstraint
            let delete_columns: Vec<_> = plan
                .actions
                .iter()
                .filter(|a| matches!(a, MigrationAction::DeleteColumn { .. }))
                .collect();
            assert_eq!(
                delete_columns.len(),
                2,
                "Should have 2 DeleteColumn actions"
            );

            let has_remove_constraint = plan
                .actions
                .iter()
                .any(|a| matches!(a, MigrationAction::RemoveConstraint { .. }));
            assert!(
                !has_remove_constraint,
                "Should NOT have RemoveConstraint when all composite columns deleted"
            );
        }

        #[test]
        fn keep_remove_constraint_when_no_columns_deleted() {
            // Normal case: constraint removed but columns remain
            let from = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![idx("ix_users__email", vec!["email"])],
            )];

            let to = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![], // Index removed but column remains
            )];

            let plan = diff_schemas(&from, &to).unwrap();

            assert_eq!(plan.actions.len(), 1);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::RemoveConstraint { table, .. }
                if table == "users"
            ));
        }
    }
}
