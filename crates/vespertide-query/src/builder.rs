use vespertide_core::MigrationPlan;

use crate::error::QueryError;
use crate::sql::{BuiltQuery, build_action_queries};

#[cfg(test)]
use crate::sql::DatabaseBackend;

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
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType,
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

    #[rstest]
    #[case::empty(
        MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![],
        },
        0
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
        1
    )]
    #[case::multiple_actions(
        MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![],
                },
                MigrationAction::DeleteTable {
                    table: "posts".into(),
                },
            ],
        },
        2
    )]
    fn test_build_plan_queries(#[case] plan: MigrationPlan, #[case] expected_count: usize) {
        let result = build_plan_queries(&plan).unwrap();
        assert_eq!(
            result.len(),
            expected_count,
            "Expected {} queries, got {}",
            expected_count,
            result.len()
        );
    }

    #[test]
    fn test_build_plan_queries_sql_content() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![],
                },
                MigrationAction::DeleteTable {
                    table: "posts".into(),
                },
            ],
        };

        let result = build_plan_queries(&plan).unwrap();
        assert_eq!(result.len(), 2);

        // Test PostgreSQL output
        let sql1 = result[0].build(DatabaseBackend::Postgres);
        assert!(sql1.contains("CREATE TABLE"));
        assert!(sql1.contains("\"users\""));
        assert!(sql1.contains("\"id\""));

        let sql2 = result[1].build(DatabaseBackend::Postgres);
        assert!(sql2.contains("DROP TABLE"));
        assert!(sql2.contains("\"posts\""));

        // Test MySQL output
        let sql1_mysql = result[0].build(DatabaseBackend::MySql);
        assert!(sql1_mysql.contains("`users`"));

        let sql2_mysql = result[1].build(DatabaseBackend::MySql);
        assert!(sql2_mysql.contains("`posts`"));
    }
}
