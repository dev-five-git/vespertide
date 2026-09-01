use super::*;
use vespertide_core::DataMigrationSql;

fn plan_with(sql: DataMigrationSql) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::DataMigration {
            sql,
            description: None,
        }],
    }
}

#[rstest]
#[case::update("UPDATE user SET tier = 'pro' WHERE kind = 'internal'")]
#[case::conditional_backfill("UPDATE user SET a = 1, b = 2 WHERE type = 'legacy'")]
#[case::correlated_subquery(
    "UPDATE post SET author_id = (SELECT id FROM author a WHERE a.name = post.author_name) \
     WHERE (SELECT count(*) FROM author a WHERE a.name = post.author_name) = 1"
)]
#[case::insert("INSERT INTO audit (kind) SELECT 'backfill' FROM user")]
#[case::delete("DELETE FROM session WHERE expires_at < now()")]
#[case::with_cte("WITH stale AS (SELECT id FROM s) DELETE FROM s USING stale")]
fn data_only_sql_is_accepted(#[case] sql: &str) {
    assert!(validate_migration_plan(&plan_with(sql.into())).is_ok());
}

#[rstest]
#[case::create("CREATE TABLE t (id int)", "CREATE")]
#[case::alter("ALTER TABLE user ADD COLUMN c int", "ALTER")]
#[case::drop("DROP TABLE user", "DROP")]
#[case::truncate("TRUNCATE TABLE user", "TRUNCATE")]
#[case::lowercase("drop table user", "DROP")]
#[case::leading_comment("-- oops\nCREATE INDEX ix ON user (id)", "CREATE")]
#[case::block_comment("/* oops */ ALTER TABLE user ADD c int", "ALTER")]
fn ddl_sql_is_rejected_with_the_offending_keyword(
    #[case] sql: &str,
    #[case] expected_keyword: &str,
) {
    let err = validate_migration_plan(&plan_with(sql.into()))
        .expect_err("DDL inside data_migration must be rejected");

    match err {
        PlannerError::DataMigrationContainsDdl { keyword, statement } => {
            assert_eq!(keyword, expected_keyword);
            assert!(
                !statement.is_empty(),
                "the error must quote the offending statement"
            );
        }
        other => panic!("expected DataMigrationContainsDdl, got {other:?}"),
    }
}

#[test]
fn ddl_error_message_explains_the_replay_contract() {
    let err = validate_migration_plan(&plan_with("DROP TABLE user".into()))
        .expect_err("DDL must be rejected");
    let message = err.to_string();

    assert!(message.contains("DROP"), "{message}");
    assert!(message.contains("DROP TABLE user"), "{message}");
    assert!(message.contains("raw_sql"), "{message}");
}

#[test]
fn ddl_hidden_in_a_single_backend_branch_is_rejected() {
    let plan = plan_with(DataMigrationSql::PerBackend {
        postgres: "UPDATE user SET x = 1".into(),
        mysql: "UPDATE user SET x = 1".into(),
        sqlite: "DROP TABLE user".into(),
    });

    let err = validate_migration_plan(&plan).expect_err("per-backend DDL must be rejected");
    assert!(matches!(
        err,
        PlannerError::DataMigrationContainsDdl {
            keyword: "DROP",
            ..
        }
    ));
}

#[test]
fn portable_per_backend_data_sql_is_accepted() {
    let plan = plan_with(DataMigrationSql::PerBackend {
        postgres: "UPDATE t SET j = jsonb_build_object('ko', c)".into(),
        mysql: "UPDATE t SET j = JSON_OBJECT('ko', c)".into(),
        sqlite: "UPDATE t SET j = json_object('ko', c)".into(),
    });
    assert!(validate_migration_plan(&plan).is_ok());
}

#[test]
fn raw_sql_keeps_its_ddl_escape_hatch() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::RawSql {
            sql: "DROP TABLE user".to_string(),
        }],
    };
    assert!(
        validate_migration_plan(&plan).is_ok(),
        "the guard must constrain data_migration only"
    );
}

#[test]
fn every_offending_action_is_reported_not_just_the_first() {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::DataMigration {
                sql: "DROP TABLE a".into(),
                description: None,
            },
            MigrationAction::DataMigration {
                sql: "CREATE TABLE b (id int)".into(),
                description: None,
            },
        ],
    };

    let violations = find_plan_violations(&plan);
    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .all(|violation| matches!(violation, PlannerError::DataMigrationContainsDdl { .. }))
    );
}
