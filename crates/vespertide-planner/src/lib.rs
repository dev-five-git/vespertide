use std::collections::HashMap;

use thiserror::Error;
use vespertide_core::{IndexDef, MigrationAction, MigrationPlan, TableConstraint, TableDef};

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("table already exists: {0}")]
    TableExists(String),
    #[error("table not found: {0}")]
    TableNotFound(String),
    #[error("column already exists: {0}.{1}")]
    ColumnExists(String, String),
    #[error("column not found: {0}.{1}")]
    ColumnNotFound(String, String),
    #[error("index not found: {0}.{1}")]
    IndexNotFound(String, String),
}

/// Build the next migration plan given the current schema and existing plans.
/// The baseline schema is reconstructed from already-applied plans and then
/// diffed against the target `current` schema.
pub fn plan_next_migration(
    current: &[TableDef],
    applied_plans: &[MigrationPlan],
) -> Result<MigrationPlan, PlannerError> {
    let baseline = schema_from_plans(applied_plans)?;
    let mut plan = diff_schemas(&baseline, current)?;

    let next_version = applied_plans
        .iter()
        .map(|p| p.version)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    plan.version = next_version;
    Ok(plan)
}

/// Derive a schema snapshot from existing migration plans.
pub fn schema_from_plans(plans: &[MigrationPlan]) -> Result<Vec<TableDef>, PlannerError> {
    let mut schema: Vec<TableDef> = Vec::new();
    for plan in plans {
        for action in &plan.actions {
            apply_action(&mut schema, action)?;
        }
    }
    Ok(schema)
}

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
        version: 0,
        actions,
    })
}

/// Apply a single migration action to an in-memory schema snapshot.
pub fn apply_action(
    schema: &mut Vec<TableDef>,
    action: &MigrationAction,
) -> Result<(), PlannerError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => {
            if schema.iter().any(|t| t.name == *table) {
                return Err(PlannerError::TableExists(table.clone()));
            }
            schema.push(TableDef {
                name: table.clone(),
                columns: columns.clone(),
                constraints: constraints.clone(),
                indexes: Vec::new(),
            });
            Ok(())
        }
        MigrationAction::DeleteTable { table } => {
            let before = schema.len();
            schema.retain(|t| t.name != *table);
            if schema.len() == before {
                return Err(PlannerError::TableNotFound(table.clone()));
            }
            Ok(())
        }
        MigrationAction::AddColumn {
            table,
            column,
            fill_with: _,
        } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            if tbl.columns.iter().any(|c| c.name == column.name) {
                return Err(PlannerError::ColumnExists(
                    table.clone(),
                    column.name.clone(),
                ));
            }
            tbl.columns.push(column.clone());
            Ok(())
        }
        MigrationAction::RenameColumn { table, from, to } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let col = tbl
                .columns
                .iter_mut()
                .find(|c| c.name == *from)
                .ok_or_else(|| PlannerError::ColumnNotFound(table.clone(), from.clone()))?;
            col.name = to.clone();
            rename_column_in_constraints(&mut tbl.constraints, from, to);
            rename_column_in_indexes(&mut tbl.indexes, from, to);
            Ok(())
        }
        MigrationAction::DeleteColumn { table, column } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let before = tbl.columns.len();
            tbl.columns.retain(|c| c.name != *column);
            if tbl.columns.len() == before {
                return Err(PlannerError::ColumnNotFound(table.clone(), column.clone()));
            }
            drop_column_from_constraints(&mut tbl.constraints, column);
            drop_column_from_indexes(&mut tbl.indexes, column);
            Ok(())
        }
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let col = tbl
                .columns
                .iter_mut()
                .find(|c| c.name == *column)
                .ok_or_else(|| PlannerError::ColumnNotFound(table.clone(), column.clone()))?;
            col.r#type = new_type.clone();
            Ok(())
        }
        MigrationAction::AddIndex { table, index } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            tbl.indexes.push(index.clone());
            Ok(())
        }
        MigrationAction::RemoveIndex { table, name } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let before = tbl.indexes.len();
            tbl.indexes.retain(|i| i.name != *name);
            if tbl.indexes.len() == before {
                return Err(PlannerError::IndexNotFound(table.clone(), name.clone()));
            }
            Ok(())
        }
        MigrationAction::RenameTable { from, to } => {
            if schema.iter().any(|t| t.name == *to) {
                return Err(PlannerError::TableExists(to.clone()));
            }
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *from)
                .ok_or_else(|| PlannerError::TableNotFound(from.clone()))?;
            tbl.name = to.clone();
            Ok(())
        }
    }
}

fn rename_column_in_constraints(constraints: &mut [TableConstraint], from: &str, to: &str) {
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey(cols) => {
                for c in cols.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
            }
            TableConstraint::Unique { columns, .. } => {
                for c in columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
            }
            TableConstraint::ForeignKey {
                columns,
                ref_columns,
                ..
            } => {
                for c in columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
                for c in ref_columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
            }
            TableConstraint::Check { .. } => {}
        }
    }
}

fn rename_column_in_indexes(indexes: &mut [IndexDef], from: &str, to: &str) {
    for idx in indexes {
        for c in idx.columns.iter_mut() {
            if c == from {
                *c = to.to_string();
            }
        }
    }
}

fn drop_column_from_constraints(constraints: &mut Vec<TableConstraint>, column: &str) {
    constraints.retain_mut(|c| match c {
        TableConstraint::PrimaryKey(cols) => {
            cols.retain(|c| c != column);
            !cols.is_empty()
        }
        TableConstraint::Unique { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        TableConstraint::ForeignKey {
            columns,
            ref_columns,
            ..
        } => {
            columns.retain(|c| c != column);
            ref_columns.retain(|c| c != column);
            !columns.is_empty() && !ref_columns.is_empty()
        }
        TableConstraint::Check { .. } => true,
    });
}

fn drop_column_from_indexes(indexes: &mut Vec<IndexDef>, column: &str) {
    indexes.retain(|idx| !idx.columns.iter().any(|c| c == column));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, IndexDef, TableConstraint};

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
        constraints: Vec<TableConstraint>,
        indexes: Vec<IndexDef>,
    ) -> TableDef {
        TableDef {
            name: name.to_string(),
            columns,
            constraints,
            indexes,
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum ErrKind {
        TableExists,
        TableNotFound,
        ColumnExists,
        ColumnNotFound,
        IndexNotFound,
    }

    fn assert_err_kind(err: PlannerError, kind: ErrKind) {
        match (err, kind) {
            (PlannerError::TableExists(_), ErrKind::TableExists) => {}
            (PlannerError::TableNotFound(_), ErrKind::TableNotFound) => {}
            (PlannerError::ColumnExists(_, _), ErrKind::ColumnExists) => {}
            (PlannerError::ColumnNotFound(_, _), ErrKind::ColumnNotFound) => {}
            (PlannerError::IndexNotFound(_, _), ErrKind::IndexNotFound) => {}
            (other, expected) => panic!("unexpected error {other:?}, expected {:?}", expected),
        }
    }

    #[rstest]
    #[case::create_only(
        vec![MigrationPlan {
            comment: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Integer)],
                constraints: vec![TableConstraint::PrimaryKey(vec!["id".into()])],
            }],
        }],
        table(
            "users",
            vec![col("id", ColumnType::Integer)],
            vec![TableConstraint::PrimaryKey(vec!["id".into()])],
            vec![],
        )
    )]
    #[case::create_and_add_column(
        vec![
            MigrationPlan {
                comment: None,
                version: 1,
                actions: vec![MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Integer)],
                    constraints: vec![TableConstraint::PrimaryKey(vec!["id".into()])],
                }],
            },
            MigrationPlan {
                comment: None,
                version: 2,
                actions: vec![MigrationAction::AddColumn {
                    table: "users".into(),
                    column: col("name", ColumnType::Text),
                    fill_with: None,
                }],
            },
        ],
        table(
            "users",
            vec![
                col("id", ColumnType::Integer),
                col("name", ColumnType::Text),
            ],
            vec![TableConstraint::PrimaryKey(vec!["id".into()])],
            vec![],
        )
    )]
    #[case::create_add_column_and_index(
        vec![
            MigrationPlan {
                comment: None,
                version: 1,
                actions: vec![MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Integer)],
                    constraints: vec![TableConstraint::PrimaryKey(vec!["id".into()])],
                }],
            },
            MigrationPlan {
                comment: None,
                version: 2,
                actions: vec![MigrationAction::AddColumn {
                    table: "users".into(),
                    column: col("name", ColumnType::Text),
                    fill_with: None,
                }],
            },
            MigrationPlan {
                comment: None,
                version: 3,
                actions: vec![MigrationAction::AddIndex {
                    table: "users".into(),
                    index: IndexDef {
                        name: "idx_users_name".into(),
                        columns: vec!["name".into()],
                        unique: false,
                    },
                }],
            },
        ],
        table(
            "users",
            vec![
                col("id", ColumnType::Integer),
                col("name", ColumnType::Text),
            ],
            vec![TableConstraint::PrimaryKey(vec!["id".into()])],
            vec![IndexDef {
                name: "idx_users_name".into(),
                columns: vec!["name".into()],
                unique: false,
            }],
        )
    )]
    fn schema_from_plans_applies_actions(
        #[case] plans: Vec<MigrationPlan>,
        #[case] expected_users: TableDef,
    ) {
        let schema = schema_from_plans(&plans).unwrap();
        let users = schema.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users, &expected_users);
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

    #[rstest]
    fn plan_next_migration_sets_next_version() {
        let applied = vec![MigrationPlan {
            comment: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Integer)],
                constraints: vec![],
            }],
        }];

        let target_schema = vec![table(
            "users",
            vec![
                col("id", ColumnType::Integer),
                col("name", ColumnType::Text),
            ],
            vec![],
            vec![],
        )];

        let plan = plan_next_migration(&target_schema, &applied).unwrap();
        assert_eq!(plan.version, 2);
        assert!(plan.actions.iter().any(
            |a| matches!(a, MigrationAction::AddColumn { column, .. } if column.name == "name")
        ));
    }

    #[rstest]
    #[case(
        vec![table("users", vec![], vec![], vec![])],
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![],
            constraints: vec![],
        },
        ErrKind::TableExists
    )]
    #[case(
        vec![],
        MigrationAction::DeleteTable {
            table: "users".into()
        },
        ErrKind::TableNotFound
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![]
        )],
        MigrationAction::AddColumn {
            table: "users".into(),
            column: col("id", ColumnType::Integer),
            fill_with: None,
        },
        ErrKind::ColumnExists
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![]
        )],
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "missing".into()
        },
        ErrKind::ColumnNotFound
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![]
        )],
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx".into()
        },
        ErrKind::IndexNotFound
    )]
    fn apply_action_reports_errors(
        #[case] mut schema: Vec<TableDef>,
        #[case] action: MigrationAction,
        #[case] expected: ErrKind,
    ) {
        let err = apply_action(&mut schema, &action).unwrap_err();
        assert_err_kind(err, expected);
    }
}
