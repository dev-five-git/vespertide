use std::collections::HashMap;

use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

use crate::error::PlannerError;

/// Diff two schema snapshots into a migration plan.
/// Both schemas are normalized to convert inline column constraints
/// (primary_key, unique, index, foreign_key) to table-level constraints.
pub fn diff_schemas(from: &[TableDef], to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    let mut actions: Vec<MigrationAction> = Vec::new();

    // Normalize both schemas to ensure inline constraints are converted to table-level
    let from_normalized: Vec<TableDef> = from.iter().map(|t| t.normalize()).collect();
    let to_normalized: Vec<TableDef> = to.iter().map(|t| t.normalize()).collect();

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
    for (name, tbl) in &to_map {
        if !from_map.contains_key(name) {
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
    }

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
    use vespertide_core::{ColumnDef, ColumnType, IndexDef, MigrationAction};

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
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![
                col("id", ColumnType::Integer),
                col("name", ColumnType::Text),
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
                column: col("name", ColumnType::Text),
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
            vec![col("id", ColumnType::Integer)],
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
            vec![col("id", ColumnType::Integer)],
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
                columns: vec![col("id", ColumnType::Integer)],
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
            vec![col("id", ColumnType::Integer), col("name", ColumnType::Text)],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
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
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Text)],
            vec![],
            vec![],
        )],
        vec![MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "id".into(),
            new_type: ColumnType::Text,
        }]
    )]
    #[case::remove_index(
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![IndexDef {
                name: "idx_users_id".into(),
                columns: vec!["id".into()],
                unique: false,
            }],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
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
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![],
        )],
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
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
        use vespertide_core::{StrOrBool, TableConstraint};

        fn col_with_pk(name: &str, ty: ColumnType) -> ColumnDef {
            ColumnDef {
                name: name.to_string(),
                r#type: ty,
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(true),
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
                unique: Some(StrOrBool::Bool(true)),
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
                index: Some(StrOrBool::Bool(true)),
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
                foreign_key: Some(ForeignKeyDef {
                    ref_table: ref_table.to_string(),
                    ref_columns: vec![ref_col.to_string()],
                    on_delete: None,
                    on_update: None,
                }),
            }
        }

        #[test]
        fn create_table_with_inline_pk() {
            let plan = diff_schemas(
                &[],
                &[table(
                    "users",
                    vec![
                        col_with_pk("id", ColumnType::Integer),
                        col("name", ColumnType::Text),
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
                    TableConstraint::PrimaryKey { columns } if columns == &["id".to_string()]
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
                        col("id", ColumnType::Integer),
                        col_with_unique("email", ColumnType::Text),
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
                        col("id", ColumnType::Integer),
                        col_with_index("name", ColumnType::Text),
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
                        col("id", ColumnType::Integer),
                        col_with_fk("user_id", ColumnType::Integer, "users", "id"),
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
                        col("id", ColumnType::Integer),
                        col("name", ColumnType::Text),
                    ],
                    vec![],
                    vec![],
                )],
                &[table(
                    "users",
                    vec![
                        col("id", ColumnType::Integer),
                        col_with_index("name", ColumnType::Text),
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
            let mut id_col = col("id", ColumnType::Integer);
            id_col.primary_key = Some(true);
            id_col.nullable = false;

            let mut email_col = col("email", ColumnType::Text);
            email_col.unique = Some(StrOrBool::Bool(true));

            let mut name_col = col("name", ColumnType::Text);
            name_col.index = Some(StrOrBool::Bool(true));

            let mut org_id_col = col("org_id", ColumnType::Integer);
            org_id_col.foreign_key = Some(ForeignKeyDef {
                ref_table: "orgs".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            });

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
    }
}
