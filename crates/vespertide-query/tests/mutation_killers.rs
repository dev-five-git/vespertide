use rstest::rstest;
use vespertide_core::{
    CheckViolationStrategy, ColumnDef, ColumnType, ForeignKeyOrphanStrategy, KeepPolicy,
    MigrationAction, ReferenceAction, SimpleColumnType, TableConstraint, TableDef,
    UniqueConstraintStrategy,
};
use vespertide_query::{DatabaseBackend, build_action_queries};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

fn table(name: &str, columns: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

fn unique(name: Option<&str>, columns: &[&str]) -> TableConstraint {
    TableConstraint::Unique {
        name: name.map(Into::into),
        columns: columns.iter().map(|column| (*column).into()).collect(),
        strategy: UniqueConstraintStrategy::DeleteDuplicates {
            keep: KeepPolicy::First,
        },
    }
}

fn check(name: &str, expr: &str) -> TableConstraint {
    TableConstraint::Check {
        name: name.into(),
        expr: expr.into(),
        strategy: CheckViolationStrategy::default(),
    }
}

fn fk(name: &str) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: Some(name.into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(ReferenceAction::Cascade),
        on_update: None,
        orphan_strategy: ForeignKeyOrphanStrategy::DeleteOrphans,
    }
}

fn render_action(
    backend: DatabaseBackend,
    action: &MigrationAction,
    current_schema: &[TableDef],
) -> String {
    build_action_queries(backend, action, current_schema)
        .expect("build_action_queries should succeed")
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<_>>()
        .join("\n")
}

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn remove_composite_unique_keeps_overlapping_single_unique(#[case] backend: DatabaseBackend) {
    let single_email = unique(None, &["email"]);
    let composite = unique(None, &["email", "tenant_id"]);
    let schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![single_email, composite.clone()],
    )];
    let action = MigrationAction::RemoveConstraint {
        table: "users".into(),
        constraint: composite,
    };

    let sql = render_action(backend, &action, &schema);

    match backend {
        DatabaseBackend::Sqlite => {
            assert!(
                sql.contains("CREATE UNIQUE INDEX \"uq_users__email\" ON \"users\" (\"email\")"),
                "SQLite rebuild must preserve the single-column email unique; got: {sql}"
            );
            assert_eq!(
                sql.matches("CREATE UNIQUE INDEX").count(),
                1,
                "SQLite should recreate only the surviving single-column unique; got: {sql}"
            );
            assert!(
                !sql.contains("uq_users__email_tenant_id"),
                "SQLite rebuild must drop only the removed composite unique; got: {sql}"
            );
        }
        DatabaseBackend::Postgres => assert!(
            sql.contains("DROP INDEX \"uq_users__email_tenant_id\""),
            "Postgres must drop the composite unique; got: {sql}"
        ),
        DatabaseBackend::MySql => assert!(
            sql.contains("DROP INDEX `uq_users__email_tenant_id`"),
            "MySQL must drop the composite unique; got: {sql}"
        ),
    }
}

#[rstest]
#[case::postgres_unquoted(DatabaseBackend::Postgres, "age > 0")]
#[case::postgres_quoted(DatabaseBackend::Postgres, r#""age" > 0"#)]
#[case::mysql_unquoted(DatabaseBackend::MySql, "age > 0")]
#[case::mysql_quoted(DatabaseBackend::MySql, r#""age" > 0"#)]
#[case::sqlite_unquoted(DatabaseBackend::Sqlite, "age > 0")]
#[case::sqlite_quoted(DatabaseBackend::Sqlite, r#""age" > 0"#)]
fn delete_column_omits_check_referencing_deleted_column(
    #[case] backend: DatabaseBackend,
    #[case] check_expr: &str,
) {
    let schema = vec![table(
        "people",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("age", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![
            check("chk_age_positive", check_expr),
            check("chk_name_present", r#""name" <> ''"#),
        ],
    )];
    let action = MigrationAction::DeleteColumn {
        table: "people".into(),
        column: "age".into(),
    };

    let sql = render_action(backend, &action, &schema);

    if backend == DatabaseBackend::Sqlite {
        assert!(
            sql.contains("people_temp"),
            "SQLite must use temp-table rebuild for CHECK-bearing column drop; got: {sql}"
        );
        assert!(
            !sql.contains("chk_age_positive"),
            "SQLite rebuild must remove CHECK name for deleted age column; got: {sql}"
        );
        assert!(
            !sql.contains(check_expr),
            "SQLite rebuild must remove CHECK expression '{check_expr}'; got: {sql}"
        );
        assert!(
            sql.contains("chk_name_present"),
            "SQLite rebuild must preserve unrelated CHECK constraints; got: {sql}"
        );
    } else {
        assert!(
            sql.contains("DROP COLUMN"),
            "{backend:?} should use direct DROP COLUMN; got: {sql}"
        );
    }
}

#[rstest]
#[case::postgres_set(DatabaseBackend::Postgres, Some("hello"))]
#[case::postgres_drop(DatabaseBackend::Postgres, None)]
#[case::mysql_set(DatabaseBackend::MySql, Some("hello"))]
#[case::mysql_drop(DatabaseBackend::MySql, None)]
#[case::sqlite_set(DatabaseBackend::Sqlite, Some("hello"))]
#[case::sqlite_drop(DatabaseBackend::Sqlite, None)]
fn modify_column_comment_emits_exact_comment_literal(
    #[case] backend: DatabaseBackend,
    #[case] new_comment: Option<&str>,
) {
    let schema = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];
    let action = MigrationAction::ModifyColumnComment {
        table: "users".into(),
        column: "email".into(),
        new_comment: new_comment.map(Into::into),
    };

    let sql = render_action(backend, &action, &schema);

    match (backend, new_comment) {
        (DatabaseBackend::Sqlite, _) => assert!(
            sql.is_empty(),
            "SQLite does not support comments and should emit no SQL; got: {sql}"
        ),
        (DatabaseBackend::Postgres, Some(_)) => assert!(
            sql.contains("COMMENT ON COLUMN \"users\".\"email\" IS 'hello'"),
            "Postgres must emit exact comment literal `IS 'hello'`; got: {sql}"
        ),
        (DatabaseBackend::Postgres, None) => assert!(
            sql.contains("COMMENT ON COLUMN \"users\".\"email\" IS NULL"),
            "Postgres must emit exact comment removal `IS NULL`; got: {sql}"
        ),
        (DatabaseBackend::MySql, Some(_)) => {
            assert!(
                sql.contains("COMMENT 'hello'"),
                "MySQL must emit exact comment literal `COMMENT 'hello'`; got: {sql}"
            );
            assert!(
                !sql.contains("COMMENT ''"),
                "MySQL must not collapse comment to an empty literal; got: {sql}"
            );
        }
        (DatabaseBackend::MySql, None) => assert!(
            !sql.contains("COMMENT '"),
            "MySQL comment removal must not append a COMMENT literal; got: {sql}"
        ),
    }
}

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn add_constraint_rebuild_dedups_multiple_overlapping_constraints(
    #[case] backend: DatabaseBackend,
) {
    let custom_name = "user_id";
    let emitted_name = "fk_orders__user_id";
    let existing_fk = fk(custom_name);
    let new_fk = fk(custom_name);
    let schema = vec![table(
        "orders",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![existing_fk.clone(), existing_fk],
    )];
    let action = MigrationAction::AddConstraint {
        table: "orders".into(),
        constraint: new_fk,
    };

    let sql = render_action(backend, &action, &schema);

    match backend {
        DatabaseBackend::Postgres => assert_eq!(
            sql.matches("ADD CONSTRAINT \"fk_orders__user_id\"").count(),
            1,
            "Postgres must add the FK constraint once before validation; got: {sql}"
        ),
        DatabaseBackend::MySql => assert_eq!(
            sql.matches(emitted_name).count(),
            1,
            "MySQL must emit the FK constraint name exactly once; got: {sql}"
        ),
        DatabaseBackend::Sqlite => assert_eq!(
            sql.matches("FOREIGN KEY (\"user_id\")").count(),
            1,
            "SQLite rebuild must merge overlapping existing FKs into one emitted FK clause; got: {sql}"
        ),
    }
}
