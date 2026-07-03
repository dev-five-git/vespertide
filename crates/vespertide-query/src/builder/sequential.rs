use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};
use vespertide_planner::apply_action;

use super::{PlanQueries, action_target_table};
use crate::DatabaseBackend;
use crate::error::QueryError;
use crate::sql::build_action_queries_with_pending;

pub(super) fn build_plan_queries_sequentially(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    let mut queries: Vec<PlanQueries> = Vec::new();
    let mut evolving_schema = current_schema.to_vec();

    for (i, action) in plan.actions.iter().enumerate() {
        // For SQLite rebuilds, avoid recreating pending indexes that later actions create.
        let pending_constraints = pending_constraints_for_action(plan, i, action);

        let postgres_queries = build_action_queries_with_pending(
            DatabaseBackend::Postgres,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        let mysql_queries = build_action_queries_with_pending(
            DatabaseBackend::MySql,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        let sqlite_queries = build_action_queries_with_pending(
            DatabaseBackend::Sqlite,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
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

fn pending_constraints_for_action(
    plan: &MigrationPlan,
    action_index: usize,
    action: &MigrationAction,
) -> Vec<TableConstraint> {
    let Some(table) = action_target_table(action) else {
        return vec![];
    };

    plan.actions[action_index + 1..]
        .iter()
        .filter_map(|a| {
            if let MigrationAction::AddConstraint {
                table: t,
                constraint,
            } = a
                && t == table
                && matches!(
                    constraint,
                    TableConstraint::Index { .. } | TableConstraint::Unique { .. }
                )
            {
                Some(constraint.clone())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{build_plan_queries_sequentially, pending_constraints_for_action};
    use crate::DatabaseBackend;
    use crate::sql::BuiltQuery;
    use vespertide_core::{
        ColumnDef, ColumnType, ForeignKeyOrphanStrategy, MigrationAction, MigrationPlan,
        ReferenceAction, SimpleColumnType, TableConstraint, TableDef,
    };

    fn nn_col(name: &str, ty: SimpleColumnType) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(ty), false)
    }

    fn index(name: Option<&str>, column: &str) -> TableConstraint {
        TableConstraint::Index {
            name: name.map(Into::into),
            columns: vec![column.into()],
        }
    }

    fn foreign_key() -> TableConstraint {
        TableConstraint::ForeignKey {
            name: Some("fk_u__pk".into()),
            columns: vec!["pk".into()],
            ref_table: "other".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
            orphan_strategy: ForeignKeyOrphanStrategy::default(),
        }
    }

    fn table(name: &str, constraints: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: vec![nn_col("pk", SimpleColumnType::Integer)],
            constraints,
        }
    }

    fn schema_u_with_constraints(constraints: Vec<TableConstraint>) -> Vec<TableDef> {
        vec![table("u", constraints)]
    }

    fn schema_u_and_v_with_u_constraints(constraints: Vec<TableConstraint>) -> Vec<TableDef> {
        vec![table("u", constraints), table("v", vec![])]
    }

    fn add_required_column(table: &str, column: &str) -> MigrationAction {
        MigrationAction::AddColumn {
            table: table.into(),
            column: Box::new(nn_col(column, SimpleColumnType::Integer)),
            fill_with: None,
        }
    }

    fn sqlite_sql(queries: &[BuiltQuery]) -> String {
        queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The pending scan must start after the current action. If it starts at
    /// `i`, an `AddConstraint(Index)` sees itself as pending.
    #[test]
    fn pending_constraints_start_after_current_action() {
        let current_index = index(Some("ix_u__pk"), "pk");
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddConstraint {
                table: "u".into(),
                constraint: current_index,
            }],
        };

        let pending = pending_constraints_for_action(&plan, 0, &plan.actions[0]);
        assert!(
            pending.is_empty(),
            "current action must not be included in its own pending set: {pending:?}"
        );
    }

    /// Correct same-table pending detection makes the SQLite rebuild skip an
    /// index that a later `AddConstraint` will create. The `t == table` →
    /// `t != table` mutant leaves the index out of the pending set, so action 0
    /// wrongly recreates it.
    #[test]
    fn sqlite_rebuild_skips_later_same_table_pending_index() {
        let pending_index = index(None, "pk");
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                add_required_column("u", "extra"),
                MigrationAction::AddConstraint {
                    table: "u".into(),
                    constraint: pending_index.clone(),
                },
            ],
        };

        let result =
            build_plan_queries_sequentially(&plan, &schema_u_with_constraints(vec![pending_index]))
                .unwrap();
        assert_eq!(result.len(), 2);

        let sqlite_sql_0 = sqlite_sql(&result[0].sqlite);

        assert!(
            !sqlite_sql_0.contains("CREATE INDEX \"ix_u__pk\""),
            "same-table pending index must be deferred to action 1, got:\n{sqlite_sql_0}"
        );

        let sqlite_sql_1 = sqlite_sql(&result[1].sqlite);
        assert!(
            sqlite_sql_1.contains("CREATE INDEX \"ix_u__pk\""),
            "later AddConstraint must create the deferred index, got:\n{sqlite_sql_1}"
        );
    }

    /// A later matching index on another table is not pending for this rebuild.
    /// Because `TableConstraint::Index` does not carry the table name, the
    /// `&&` → `||` mutant collects the wrong-table index and skips recreating
    /// the existing `u.pk` index.
    #[test]
    fn sqlite_rebuild_recreates_existing_index_when_matching_later_index_is_other_table() {
        let existing_u_index = index(None, "pk");
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                add_required_column("u", "extra"),
                MigrationAction::AddConstraint {
                    table: "v".into(),
                    constraint: existing_u_index.clone(),
                },
            ],
        };

        let result = build_plan_queries_sequentially(
            &plan,
            &schema_u_and_v_with_u_constraints(vec![existing_u_index]),
        )
        .unwrap();
        assert_eq!(result.len(), 2);

        let sqlite_sql_0 = sqlite_sql(&result[0].sqlite);
        assert!(
            sqlite_sql_0.contains("CREATE INDEX \"ix_u__pk\" ON \"u\" (\"pk\")"),
            "wrong-table pending index must not suppress recreating u.pk, got:\n{sqlite_sql_0}"
        );
    }

    /// Same-table non-index constraints are excluded from the pending set. This
    /// is asserted directly because SQLite index recreation ignores non-index
    /// constraints either way, making this branch otherwise output-equivalent.
    #[test]
    fn pending_constraints_exclude_same_table_non_index_constraint() {
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                add_required_column("u", "extra"),
                MigrationAction::AddConstraint {
                    table: "u".into(),
                    constraint: foreign_key(),
                },
            ],
        };

        let pending = pending_constraints_for_action(&plan, 0, &plan.actions[0]);
        assert!(
            pending.is_empty(),
            "foreign keys must not be collected as pending indexes: {pending:?}"
        );
    }
}
