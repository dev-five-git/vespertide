use vespertide_core::MigrationPlan;

use crate::error::QueryError;
use crate::sql::{BuiltQuery, build_action_queries};

pub fn build_plan_queries(plan: &MigrationPlan) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries: Vec<BuiltQuery> = Vec::new();
    for action in &plan.actions {
        queries.extend(build_action_queries(action)?);
    }
    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
        }
    }

    #[rstest]
    #[case::empty(
        MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![],
        },
        vec![]
    )]
    #[case::single_action(
        MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::DeleteTable {
                table: "users".into(),
            }],
        },
        vec![
            ("DROP TABLE $1;".to_string(), vec!["users".to_string()])
        ]
    )]
    #[case::multiple_actions(
        MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Integer)],
                    constraints: vec![],
                },
                MigrationAction::DeleteTable {
                    table: "posts".into(),
                },
            ],
        },
        vec![
            (
                "CREATE TABLE $1 ($2 INTEGER);".to_string(),
                vec!["users".to_string(), "id".to_string()]
            ),
            (
                "DROP TABLE $1;".to_string(),
                vec!["posts".to_string()]
            ),
        ]
    )]
    fn test_build_plan_queries(
        #[case] plan: MigrationPlan,
        #[case] expected: Vec<(String, Vec<String>)>,
    ) {
        let result = build_plan_queries(&plan).unwrap();
        assert_eq!(
            result.len(),
            expected.len(),
            "Expected {} queries, got {}",
            expected.len(),
            result.len()
        );

        for (i, (expected_sql, expected_binds)) in expected.iter().enumerate() {
            assert_eq!(result[i].sql, *expected_sql, "Query {} sql mismatch", i);
            assert_eq!(
                result[i].binds, *expected_binds,
                "Query {} binds mismatch",
                i
            );
        }
    }
}
