use vespertide_core::{MigrationPlan, TableDef};

use crate::apply::apply_action;
use crate::error::PlannerError;

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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, IndexDef, MigrationAction, SimpleColumnType, TableConstraint,
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

    #[rstest]
    #[case::create_only(
        vec![MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
            }],
        }],
        table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
            vec![],
        )
    )]
    #[case::create_and_add_column(
        vec![
            MigrationPlan {
                comment: None,
                created_at: None,
                version: 1,
                actions: vec![MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
                }],
            },
            MigrationPlan {
                comment: None,
                created_at: None,
                version: 2,
                actions: vec![MigrationAction::AddColumn {
                    table: "users".into(),
                    column: col("name", ColumnType::Simple(SimpleColumnType::Text)),
                    fill_with: None,
                }],
            },
        ],
        table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
            vec![],
        )
    )]
    #[case::create_add_column_and_index(
        vec![
            MigrationPlan {
                comment: None,
                created_at: None,
                version: 1,
                actions: vec![MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
                }],
            },
            MigrationPlan {
                comment: None,
                created_at: None,
                version: 2,
                actions: vec![MigrationAction::AddColumn {
                    table: "users".into(),
                    column: col("name", ColumnType::Simple(SimpleColumnType::Text)),
                    fill_with: None,
                }],
            },
            MigrationPlan {
                comment: None,
                created_at: None,
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
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
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
}
