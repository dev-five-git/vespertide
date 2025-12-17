use vespertide_core::{MigrationAction, MigrationPlan, TableDef};
use vespertide_planner::apply_action;

use crate::DatabaseBackend;
use crate::error::QueryError;
use crate::sql::{BuiltQuery, build_action_queries};

pub struct PlanQueries {
    pub action: MigrationAction,
    pub postgres: Vec<BuiltQuery>,
    pub mysql: Vec<BuiltQuery>,
    pub sqlite: Vec<BuiltQuery>,
}

pub fn build_plan_queries(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    let mut queries: Vec<PlanQueries> = Vec::new();
    // Clone the schema so we can mutate it as we apply actions
    let mut evolving_schema = current_schema.to_vec();

    for action in &plan.actions {
        // Build queries with the current state of the schema
        let postgres_queries =
            build_action_queries(&DatabaseBackend::Postgres, action, &evolving_schema)?;
        let mysql_queries = build_action_queries(&DatabaseBackend::MySql, action, &evolving_schema)?;
        let sqlite_queries =
            build_action_queries(&DatabaseBackend::Sqlite, action, &evolving_schema)?;
        queries.push(PlanQueries {
            action: action.clone(),
            postgres: postgres_queries,
            mysql: mysql_queries,
            sqlite: sqlite_queries,
        });

        // Apply the action to update the schema for the next iteration
        // Note: We ignore errors here because some actions (like DeleteTable) may reference
        // tables that don't exist in the provided current_schema. This is OK for SQL generation
        // purposes - we still generate the correct SQL, and the schema evolution is best-effort.
        let _ = apply_action(&mut evolving_schema, action);
    }
    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::DatabaseBackend;
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
        let result = build_plan_queries(&plan, &[]).unwrap();
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

        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(result.len(), 2);

        // Test PostgreSQL output
        let sql1 = result[0]
            .postgres
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql1.contains("CREATE TABLE"));
        assert!(sql1.contains("\"users\""));
        assert!(sql1.contains("\"id\""));

        let sql2 = result[1]
            .postgres
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql2.contains("DROP TABLE"));
        assert!(sql2.contains("\"posts\""));

        // Test MySQL output
        let sql1_mysql = result[0]
            .mysql
            .iter()
            .map(|q| q.build(DatabaseBackend::MySql))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql1_mysql.contains("`users`"));

        let sql2_mysql = result[1]
            .mysql
            .iter()
            .map(|q| q.build(DatabaseBackend::MySql))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql2_mysql.contains("`posts`"));
    }
}
