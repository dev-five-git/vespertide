use vespertide_core::{MigrationAction, MigrationPlan};

/// One applied migration whose `raw_sql` actions make baseline replay
/// incomplete.
///
/// [`crate::schema_from_plans`] cannot interpret raw SQL, so it skips those
/// actions entirely. When the SQL was pure DML that is harmless; when it was
/// DDL the reconstructed baseline permanently lacks that schema change and
/// `vespertide diff` reports the same already-applied changes on every run.
/// Nothing in the plan distinguishes the two cases, so every `raw_sql` in the
/// history is reported and the user decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSqlReplayHazard {
    /// Version of the applied migration containing the `raw_sql` action(s).
    pub version: u32,
    /// How many `raw_sql` actions that migration carries.
    pub count: usize,
}

/// Find applied migrations containing `raw_sql`, in version order.
///
/// Returns an empty vec when the history is fully replayable, which is the
/// common case — callers should stay silent then rather than emit a warning.
#[must_use]
pub fn find_raw_sql_replay_hazards(plans: &[MigrationPlan]) -> Vec<RawSqlReplayHazard> {
    plans
        .iter()
        .filter_map(|plan| {
            let count = plan
                .actions
                .iter()
                .filter(|action| matches!(action, MigrationAction::RawSql { .. }))
                .count();
            (count > 0).then_some(RawSqlReplayHazard {
                version: plan.version,
                count,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::{ColumnType, DataMigrationSql, SimpleColumnType};

    fn plan_of(version: u32, actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version,
            actions,
        }
    }

    fn raw(sql: &str) -> MigrationAction {
        MigrationAction::RawSql {
            sql: sql.to_string(),
        }
    }

    fn create_users() -> MigrationAction {
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![vespertide_core::ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![],
        }
    }

    #[test]
    fn clean_history_reports_no_hazard() {
        let plans = vec![plan_of(1, vec![create_users()])];
        assert!(find_raw_sql_replay_hazards(&plans).is_empty());
    }

    #[test]
    fn data_migration_is_not_a_replay_hazard() {
        let plans = vec![plan_of(
            1,
            vec![MigrationAction::DataMigration {
                sql: DataMigrationSql::Uniform("UPDATE users SET active = true".into()),
                description: None,
            }],
        )];
        assert!(
            find_raw_sql_replay_hazards(&plans).is_empty(),
            "data_migration is skipped by contract, so replay stays complete"
        );
    }

    #[test]
    fn raw_sql_migrations_are_reported_with_version_and_count() {
        let plans = vec![
            plan_of(1, vec![create_users()]),
            plan_of(3, vec![raw("CREATE INDEX ix ON users (id)")]),
            plan_of(4, vec![create_users()]),
            plan_of(
                7,
                vec![
                    raw("ALTER TABLE users ADD c int"),
                    raw("UPDATE users SET c = 1"),
                ],
            ),
        ];

        assert_eq!(
            find_raw_sql_replay_hazards(&plans),
            vec![
                RawSqlReplayHazard {
                    version: 3,
                    count: 1
                },
                RawSqlReplayHazard {
                    version: 7,
                    count: 2
                },
            ]
        );
    }

    #[test]
    fn mixed_migration_counts_only_its_raw_sql_actions() {
        let plans = vec![plan_of(
            2,
            vec![create_users(), raw("UPDATE users SET x = 1")],
        )];
        assert_eq!(
            find_raw_sql_replay_hazards(&plans),
            vec![RawSqlReplayHazard {
                version: 2,
                count: 1
            }]
        );
    }

    #[test]
    fn empty_history_reports_no_hazard() {
        assert!(find_raw_sql_replay_hazards(&[]).is_empty());
    }
}
