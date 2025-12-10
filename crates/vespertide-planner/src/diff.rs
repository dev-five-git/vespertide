use std::collections::HashMap;

use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

use crate::error::PlannerError;

/// Diff two schema snapshots into a migration plan.
pub fn diff_schemas(from: &[TableDef], to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    let mut actions: Vec<MigrationAction> = Vec::new();

    let from_map: HashMap<_, _> = from.iter().map(|t| (t.name.as_str(), t)).collect();
    let to_map: HashMap<_, _> = to.iter().map(|t| (t.name.as_str(), t)).collect();

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
                if let Some(from_def) = from_cols.get(col) {
                    if from_def.r#type != to_def.r#type {
                        actions.push(MigrationAction::ModifyColumnType {
                            table: (*name).to_string(),
                            column: (*col).to_string(),
                            new_type: to_def.r#type.clone(),
                        });
                    }
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
    fn diff_schemas_detects_additions(
        #[case] from_schema: Vec<TableDef>,
        #[case] to_schema: Vec<TableDef>,
        #[case] expected_actions: Vec<MigrationAction>,
    ) {
        let plan = diff_schemas(&from_schema, &to_schema).unwrap();
        assert_eq!(plan.actions, expected_actions);
    }
}

