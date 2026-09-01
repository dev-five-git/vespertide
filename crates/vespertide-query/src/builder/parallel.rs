use rayon::prelude::*;
use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};
use vespertide_planner::apply_action;

use super::{PlanQueries, pending_constraints_for_action};
use crate::DatabaseBackend;
use crate::error::QueryError;
use crate::parallel_config::PLAN_QUERY_PAR_ACTION_MIN_LEN;
use crate::sql::build_action_queries_with_pending;

struct PreparedAction {
    action: MigrationAction,
    schema: Vec<TableDef>,
    pending_constraints: Vec<TableConstraint>,
}

pub(super) fn build_plan_queries_in_parallel(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    let prepared_actions = prepare_actions(plan, current_schema);

    prepared_actions
        .par_iter()
        .with_min_len(PLAN_QUERY_PAR_ACTION_MIN_LEN)
        .map(build_prepared_action_queries)
        .collect()
}

fn prepare_actions(plan: &MigrationPlan, current_schema: &[TableDef]) -> Vec<PreparedAction> {
    let mut prepared_actions = Vec::with_capacity(plan.actions.len());
    let mut evolving_schema = current_schema.to_vec();

    for (i, action) in plan.actions.iter().enumerate() {
        prepared_actions.push(PreparedAction {
            action: action.clone(),
            schema: evolving_schema.clone(),
            pending_constraints: pending_constraints_for_action(plan, i, action),
        });

        let _ = apply_action(&mut evolving_schema, action);
    }

    prepared_actions
}

fn build_prepared_action_queries(prepared: &PreparedAction) -> Result<PlanQueries, QueryError> {
    let postgres_queries = build_action_queries_with_pending(
        DatabaseBackend::Postgres,
        &prepared.action,
        &prepared.schema,
        &prepared.pending_constraints,
    )?;
    let mysql_queries = build_action_queries_with_pending(
        DatabaseBackend::MySql,
        &prepared.action,
        &prepared.schema,
        &prepared.pending_constraints,
    )?;
    let sqlite_queries = build_action_queries_with_pending(
        DatabaseBackend::Sqlite,
        &prepared.action,
        &prepared.schema,
        &prepared.pending_constraints,
    )?;

    Ok(PlanQueries {
        action: prepared.action.clone(),
        postgres: postgres_queries,
        mysql: mysql_queries,
        sqlite: sqlite_queries,
    })
}

#[cfg(test)]
#[expect(
    unsafe_code,
    reason = "tests that drive the parallel builder path must set/unset VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD via std::env::{set_var, remove_var}; serialized via #[serial] to avoid cross-test races"
)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::builder::build_plan_queries;
    use serial_test::serial;
    use vespertide_core::SimpleColumnType;

    /// The parallel `build_plan_queries_in_parallel` path activates when the
    /// plan size meets `VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD`. The env override is
    /// process-wide so this test is `#[serial]` to avoid colliding with other
    /// builder tests.
    #[test]
    #[serial]
    fn build_plan_queries_uses_parallel_path_under_threshold_override() {
        // SAFETY: serialized via #[serial]; this is a per-process env var that
        // the parallel_config helper re-reads on every call.
        unsafe {
            std::env::set_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD", "1");
        }

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "a".into(),
                    columns: vec![nn_col("id", SimpleColumnType::Integer)],
                    constraints: vec![],
                },
                MigrationAction::CreateTable {
                    table: "b".into(),
                    columns: vec![nn_col("id", SimpleColumnType::Integer)],
                    constraints: vec![],
                },
                MigrationAction::AddConstraint {
                    table: "a".into(),
                    constraint: TableConstraint::Index {
                        name: None,
                        columns: vec!["id".into()],
                    },
                },
            ],
        };
        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(result.len(), 3);

        // SAFETY: same as above - serialized via #[serial].
        unsafe {
            std::env::remove_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD");
        }
    }

    /// The pending scan must start after the current action. If it starts at
    /// `i`, an `AddConstraint(Index)` sees itself as pending.
    #[test]
    fn pending_constraints_start_after_current_action_parallel() {
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
            "current action must not be included in its own pending set (parallel): {pending:?}"
        );
    }

    /// Correct same-table pending detection makes the SQLite rebuild skip an
    /// index that a later `AddConstraint` will create.
    #[test]
    fn sqlite_rebuild_skips_later_same_table_pending_index_parallel() {
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
            build_plan_queries_in_parallel(&plan, &schema_u_with_constraints(vec![pending_index]))
                .unwrap();
        assert_eq!(result.len(), 2);

        let sqlite_sql_0 = sqlite_sql(&result[0].sqlite);

        assert!(
            !sqlite_sql_0.contains("CREATE INDEX \"ix_u__pk\""),
            "same-table pending index must be deferred to action 1 (parallel), got:\n{sqlite_sql_0}"
        );

        let sqlite_sql_1 = sqlite_sql(&result[1].sqlite);
        assert!(
            sqlite_sql_1.contains("CREATE INDEX \"ix_u__pk\""),
            "later AddConstraint must create the deferred index (parallel), got:\n{sqlite_sql_1}"
        );
    }

    /// A later matching index on another table is not pending for this rebuild.
    /// Because `TableConstraint::Index` does not carry the table name, collecting
    /// wrong-table indexes would skip recreating the existing `u.pk` index.
    #[test]
    fn sqlite_rebuild_recreates_existing_index_when_matching_later_index_is_other_table_parallel() {
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

        let result = build_plan_queries_in_parallel(
            &plan,
            &schema_u_and_v_with_u_constraints(vec![existing_u_index]),
        )
        .unwrap();
        assert_eq!(result.len(), 2);

        let sqlite_sql_0 = sqlite_sql(&result[0].sqlite);
        assert!(
            sqlite_sql_0.contains("CREATE INDEX \"ix_u__pk\" ON \"u\" (\"pk\")"),
            "wrong-table pending index must not suppress recreating u.pk (parallel), got:\n{sqlite_sql_0}"
        );
    }

    /// Same-table non-index constraints are excluded from the pending set. This
    /// is asserted directly because SQLite index recreation ignores non-index
    /// constraints either way, making this branch otherwise output-equivalent.
    #[test]
    fn pending_constraints_exclude_same_table_non_index_constraint_parallel() {
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
            "foreign keys must not be collected as pending indexes (parallel): {pending:?}"
        );
    }

    /// The parallel preparer must skip pending-constraint collection for actions
    /// without a target table (RawSql / RenameTable). Drives the
    /// `action_target_table -> None` branch inside `pending_constraints_for_action`.
    #[test]
    #[serial]
    fn build_plan_queries_parallel_path_skips_no_target_actions() {
        // SAFETY: serialized via #[serial].
        unsafe {
            std::env::set_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD", "1");
        }

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::RawSql {
                    sql: "SELECT 1;".into(),
                },
                MigrationAction::RenameTable {
                    from: "a".into(),
                    to: "b".into(),
                },
            ],
        };
        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(result.len(), 2);

        // SAFETY: same as above.
        unsafe {
            std::env::remove_var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD");
        }
    }
}
