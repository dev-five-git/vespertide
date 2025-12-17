use std::collections::{HashMap, HashSet, VecDeque};

use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};

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
    let mut dependencies: HashMap<&str, Vec<&str>> = HashMap::new();
    for table in tables {
        let mut deps = Vec::new();
        for constraint in &table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint {
                // Only consider dependencies within the set of tables being created
                if table_names.contains(ref_table.as_str()) && ref_table != &table.name {
                    deps.push(ref_table.as_str());
                }
            }
        }
        dependencies.insert(table.name.as_str(), deps);
    }

    // Kahn's algorithm for topological sort
    // Calculate in-degrees (number of tables that depend on each table)
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
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
    let mut queue: VecDeque<&str> = VecDeque::new();
    for table in tables {
        if in_degree.get(table.name.as_str()) == Some(&0) {
            queue.push_back(table.name.as_str());
        }
    }

    let mut result: Vec<&TableDef> = Vec::new();
    let table_map: HashMap<&str, &TableDef> = tables.iter().map(|t| (t.name.as_str(), *t)).collect();

    while let Some(table_name) = queue.pop_front() {
        if let Some(&table) = table_map.get(table_name) {
            result.push(table);
        }

        // For each table that depends on this one, decrement its in-degree
        for (dependent, deps) in &dependencies {
            if deps.contains(&table_name)
                && let Some(degree) = in_degree.get_mut(dependent)
            {
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(dependent);
                }
            }
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
fn sort_delete_tables(actions: &mut [MigrationAction], all_tables: &HashMap<&str, &TableDef>) {
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
    let delete_table_names: HashSet<&str> = delete_indices
        .iter()
        .filter_map(|&i| {
            if let MigrationAction::DeleteTable { table } = &actions[i] {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    // Build dependency graph for tables being deleted
    // dependencies[A] = [B] means A has FK referencing B
    let mut dependencies: HashMap<&str, Vec<&str>> = HashMap::new();
    for &table_name in &delete_table_names {
        let mut deps = Vec::new();
        if let Some(table_def) = all_tables.get(table_name) {
            for constraint in &table_def.constraints {
                if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                    && delete_table_names.contains(ref_table.as_str())
                    && ref_table != table_name
                {
                    deps.push(ref_table.as_str());
                }
            }
        }
        dependencies.insert(table_name, deps);
    }

    // Use Kahn's algorithm for topological sort
    // in_degree[A] = number of tables A depends on
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for &table_name in &delete_table_names {
        in_degree.insert(table_name, dependencies.get(table_name).map_or(0, |d| d.len()));
    }

    // Start with tables that have no dependencies (can be deleted last in creation order)
    let mut queue: VecDeque<&str> = VecDeque::new();
    for &table_name in &delete_table_names {
        if in_degree.get(table_name) == Some(&0) {
            queue.push_back(table_name);
        }
    }

    let mut sorted_tables: Vec<&str> = Vec::new();
    while let Some(table_name) = queue.pop_front() {
        sorted_tables.push(table_name);

        // For each table that has this one as a dependency, decrement its in-degree
        for (&dependent, deps) in &dependencies {
            if deps.contains(&table_name) {
                if let Some(degree) = in_degree.get_mut(dependent) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent);
                    }
                }
            }
        }
    }

    // Reverse to get deletion order (tables with dependencies should be deleted first)
    sorted_tables.reverse();

    // Reorder the DeleteTable actions according to sorted order
    let mut delete_actions: Vec<MigrationAction> = delete_indices
        .iter()
        .map(|&i| actions[i].clone())
        .collect();

    delete_actions.sort_by(|a, b| {
        let a_name = if let MigrationAction::DeleteTable { table } = a {
            table.as_str()
        } else {
            ""
        };
        let b_name = if let MigrationAction::DeleteTable { table } = b {
            table.as_str()
        } else {
            ""
        };

        let a_pos = sorted_tables.iter().position(|&t| t == a_name).unwrap_or(0);
        let b_pos = sorted_tables.iter().position(|&t| t == b_name).unwrap_or(0);
        a_pos.cmp(&b_pos)
    });

    // Put them back
    for (i, idx) in delete_indices.iter().enumerate() {
        actions[*idx] = delete_actions[i].clone();
    }
}

/// Diff two schema snapshots into a migration plan.
/// Both schemas are normalized to convert inline column constraints
/// (primary_key, unique, index, foreign_key) to table-level constraints.
pub fn diff_schemas(from: &[TableDef], to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    let mut actions: Vec<MigrationAction> = Vec::new();

    // Normalize both schemas to ensure inline constraints are converted to table-level
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

    let from_map: HashMap<_, _> = from_normalized
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let to_map: HashMap<_, _> = to_normalized.iter().map(|t| (t.name.as_str(), t)).collect();

    // Drop tables that disappeared.
    for name in from_map.keys() {
        if !to_map.contains_key(name) {
            actions.push(MigrationAction::DeleteTable {
                table: (*name).to_string(),
            });
        }
    }

    // Update existing tables and their indexes/columns.
    for (name, to_tbl) in &to_map {
        if let Some(from_tbl) = from_map.get(name) {
            // Columns
            let from_cols: HashMap<_, _> = from_tbl
                .columns
                .iter()
                .map(|c| (c.name.as_str(), c))
                .collect();
            let to_cols: HashMap<_, _> = to_tbl
                .columns
                .iter()
                .map(|c| (c.name.as_str(), c))
                .collect();

            // Deleted columns
            for col in from_cols.keys() {
                if !to_cols.contains_key(col) {
                    actions.push(MigrationAction::DeleteColumn {
                        table: (*name).to_string(),
                        column: (*col).to_string(),
                    });
                }
            }

            // Modified columns
            for (col, to_def) in &to_cols {
                if let Some(from_def) = from_cols.get(col)
                    && from_def.r#type != to_def.r#type
                {
                    actions.push(MigrationAction::ModifyColumnType {
                        table: (*name).to_string(),
                        column: (*col).to_string(),
                        new_type: to_def.r#type.clone(),
                    });
                }
            }

            // Added columns
            // Note: Inline foreign keys are already converted to TableConstraint::ForeignKey
            // by normalize(), so they will be handled in the constraint diff below.
            for (col, def) in &to_cols {
                if !from_cols.contains_key(col) {
                    actions.push(MigrationAction::AddColumn {
                        table: (*name).to_string(),
                        column: (*def).clone(),
                        fill_with: None,
                    });
                }
            }

            // Indexes
            let from_indexes: HashMap<_, _> = from_tbl
                .indexes
                .iter()
                .map(|i| (i.name.as_str(), i))
                .collect();
            let to_indexes: HashMap<_, _> = to_tbl
                .indexes
                .iter()
                .map(|i| (i.name.as_str(), i))
                .collect();

            for idx in from_indexes.keys() {
                if !to_indexes.contains_key(idx) {
                    actions.push(MigrationAction::RemoveIndex {
                        table: (*name).to_string(),
                        name: (*idx).to_string(),
                    });
                }
            }
            for (idx, def) in &to_indexes {
                if !from_indexes.contains_key(idx) {
                    actions.push(MigrationAction::AddIndex {
                        table: (*name).to_string(),
                        index: (*def).clone(),
                    });
                }
            }

            // Constraints - compare and detect additions/removals
            for from_constraint in &from_tbl.constraints {
                if !to_tbl.constraints.contains(from_constraint) {
                    actions.push(MigrationAction::RemoveConstraint {
                        table: (*name).to_string(),
                        constraint: from_constraint.clone(),
                    });
                }
            }
            for to_constraint in &to_tbl.constraints {
                if !from_tbl.constraints.contains(to_constraint) {
                    actions.push(MigrationAction::AddConstraint {
                        table: (*name).to_string(),
                        constraint: to_constraint.clone(),
                    });
                }
            }
        }
    }

    // Create new tables (and their indexes).
    // Collect new tables first, then topologically sort them by FK dependencies.
    let new_tables: Vec<&TableDef> = to_map
        .iter()
        .filter(|(name, _)| !from_map.contains_key(*name))
        .map(|(_, tbl)| *tbl)
        .collect();

    let sorted_new_tables = topological_sort_tables(&new_tables)?;

    for tbl in sorted_new_tables {
        actions.push(MigrationAction::CreateTable {
            table: tbl.name.clone(),
            columns: tbl.columns.clone(),
            constraints: tbl.constraints.clone(),
        });
        for idx in &tbl.indexes {
            actions.push(MigrationAction::AddIndex {
                table: tbl.name.clone(),
                index: idx.clone(),
            });
        }
    }

    // Sort DeleteTable actions so tables with FK dependencies are deleted first
    sort_delete_tables(&mut actions, &from_map);

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
    use vespertide_core::{ColumnDef, ColumnType, IndexDef, MigrationAction, SimpleColumnType};

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
        indexes: Vec<IndexDef>,
    ) -> TableDef {
        TableDef {
            name: name.to_string(),
            columns,
            constraints,
            indexes,
        }
    }

    #[rstest]
    #[case::add_column_and_index(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
            vec![IndexDef {
                name: "idx_users_name".into(),
                columns: vec!["name".into()],
                unique: false,
            }],
        )],
        vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: col("name", ColumnType::Simple(SimpleColumnType::Text)),
                fill_with: None,
            },
            MigrationAction::AddIndex {
                table: "users".into(),
                index: IndexDef {
                    name: "idx_users_name".into(),
                    columns: vec!["name".into()],
                    unique: false,
                },
            },
        ]
    )]
    #[case::drop_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
        vec![],
        vec![MigrationAction::DeleteTable {
            table: "users".into()
        }]
    )]
    #[case::add_table(
        vec![],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![IndexDef {
                name: "idx_users_id".into(),
                columns: vec!["id".into()],
                unique: true,
            }],
        )],
        vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
            },
            MigrationAction::AddIndex {
                table: "users".into(),
                index: IndexDef {
                    name: "idx_users_id".into(),
                    columns: vec!["id".into()],
                    unique: true,
                },
            },
        ]
    )]
    #[case::delete_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
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
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
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
            vec![],
            vec![IndexDef {
                name: "idx_users_id".into(),
                columns: vec!["id".into()],
                unique: false,
            }],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
        vec![MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx_users_id".into(),
        }]
    )]
    #[case::add_index_existing_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![IndexDef {
                name: "idx_users_id".into(),
                columns: vec!["id".into()],
                unique: true,
            }],
        )],
        vec![MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef {
                name: "idx_users_id".into(),
                columns: vec!["id".into()],
                unique: true,
            },
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
                    vec![],
                )],
            )
            .unwrap();

            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable { constraints, .. } = &plan.actions[0] {
                assert_eq!(constraints.len(), 1);
                assert!(matches!(
                    &constraints[0],
                    TableConstraint::PrimaryKey { columns, .. } if columns == &["id".to_string()]
                ));
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
                    vec![],
                )],
            )
            .unwrap();

            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable { constraints, .. } = &plan.actions[0] {
                assert_eq!(constraints.len(), 1);
                assert!(matches!(
                    &constraints[0],
                    TableConstraint::Unique { name: None, columns } if columns == &["email".to_string()]
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
                    vec![],
                )],
            )
            .unwrap();

            // Should have CreateTable + AddIndex
            assert_eq!(plan.actions.len(), 2);
            assert!(matches!(
                &plan.actions[0],
                MigrationAction::CreateTable { .. }
            ));
            if let MigrationAction::AddIndex { index, .. } = &plan.actions[1] {
                assert_eq!(index.name, "idx_users_name");
                assert_eq!(index.columns, vec!["name".to_string()]);
            } else {
                panic!("Expected AddIndex action");
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
                    vec![],
                )],
            )
            .unwrap();

            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::CreateTable { constraints, .. } = &plan.actions[0] {
                assert_eq!(constraints.len(), 1);
                assert!(matches!(
                    &constraints[0],
                    TableConstraint::ForeignKey { columns, ref_table, ref_columns, .. }
                        if columns == &["user_id".to_string()]
                        && ref_table == "users"
                        && ref_columns == &["id".to_string()]
                ));
            } else {
                panic!("Expected CreateTable action");
            }
        }

        #[test]
        fn add_index_via_inline_constraint() {
            // Existing table without index -> table with inline index
            let plan = diff_schemas(
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                    vec![],
                )],
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_index("name", ColumnType::Simple(SimpleColumnType::Text)),
                    ],
                    vec![],
                    vec![],
                )],
            )
            .unwrap();

            assert_eq!(plan.actions.len(), 1);
            if let MigrationAction::AddIndex { table, index } = &plan.actions[0] {
                assert_eq!(table, "users");
                assert_eq!(index.name, "idx_users_name");
                assert_eq!(index.columns, vec!["name".to_string()]);
            } else {
                panic!("Expected AddIndex action, got {:?}", plan.actions[0]);
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
                    vec![],
                )],
            )
            .unwrap();

            // Should have CreateTable + AddIndex
            assert_eq!(plan.actions.len(), 2);

            if let MigrationAction::CreateTable { constraints, .. } = &plan.actions[0] {
                // Should have: PrimaryKey, Unique, ForeignKey (3 constraints)
                assert_eq!(constraints.len(), 3);
            } else {
                panic!("Expected CreateTable action");
            }

            // Check for AddIndex action
            assert!(matches!(&plan.actions[1], MigrationAction::AddIndex { .. }));
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
                vec![],
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
                vec![],
            )];

            let to_schema = vec![table(
                "users",
                vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("email", ColumnType::Simple(SimpleColumnType::Text)),
                ],
                vec![],
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
                indexes: vec![],
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
                indexes: vec![],
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
                indexes: vec![],
            }
        }

        fn simple_table(name: &str) -> TableDef {
            TableDef {
                name: name.to_string(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
                indexes: vec![],
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
                indexes: vec![],
            };

            let table_b = TableDef {
                name: "table_b".to_string(),
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
                indexes: vec![],
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
    }
}
