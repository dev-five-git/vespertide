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
                constraints: vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
            }],
        }],
        table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
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
                    constraints: vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
                }],
            },
            MigrationPlan {
                comment: None,
                created_at: None,
                version: 2,
                actions: vec![MigrationAction::AddColumn {
                    table: "users".into(),
                    column: Box::new(col("name", ColumnType::Simple(SimpleColumnType::Text))),
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
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
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
                    constraints: vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
                }],
            },
            MigrationPlan {
                comment: None,
                created_at: None,
                version: 2,
                actions: vec![MigrationAction::AddColumn {
                    table: "users".into(),
                    column: Box::new(col("name", ColumnType::Simple(SimpleColumnType::Text))),
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
                        name: "ix_users__name".into(),
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
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
            vec![IndexDef {
                name: "ix_users__name".into(),
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

    /// Test that RemoveConstraint works when table was created with both
    /// inline unique column AND table-level unique constraint for the same column
    #[test]
    fn remove_constraint_with_inline_and_table_level_unique() {
        use vespertide_core::StrOrBoolOrArray;

        // Simulate migration 0001: CreateTable with both inline unique and table-level constraint
        let create_plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: Some(StrOrBoolOrArray::Bool(true)), // inline unique
                    index: None,
                    foreign_key: None,
                }],
                constraints: vec![TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                }], // table-level unique (duplicate!)
            }],
        };

        // Migration 0002: RemoveConstraint
        let remove_plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 2,
            actions: vec![MigrationAction::RemoveConstraint {
                table: "users".into(),
                constraint: TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                },
            }],
        };

        let schema = schema_from_plans(&[create_plan, remove_plan]).unwrap();
        let users = schema.iter().find(|t| t.name == "users").unwrap();

        println!("Constraints after apply: {:?}", users.constraints);
        println!("Column unique field: {:?}", users.columns[0].unique);

        // After apply_action:
        // - constraints is empty (RemoveConstraint removed the table-level one)
        // - but column still has unique: Some(Bool(true))!

        // Now simulate what diff_schemas does - it normalizes the baseline
        let normalized = users.clone().normalize().unwrap();
        println!("Constraints after normalize: {:?}", normalized.constraints);

        // After normalize:
        // - inline unique (column.unique = true) is converted to table-level constraint
        // - So we'd still have one unique constraint!

        // This is the bug: diff_schemas normalizes both baseline and target,
        // but the baseline still has inline unique that gets re-added.
        assert!(
            normalized.constraints.is_empty(),
            "Expected no constraints after normalize, but got: {:?}",
            normalized.constraints
        );
    }
}
