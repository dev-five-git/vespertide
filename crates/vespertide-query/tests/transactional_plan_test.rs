use rstest::rstest;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableDef,
};
use vespertide_query::{
    DatabaseBackend, PlanQueries, PlanQueriesOptions, build_plan_queries,
    build_plan_queries_with_options,
};

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

fn nullable_rebuild_plan() -> (MigrationPlan, Vec<TableDef>) {
    let schema = vec![TableDef {
        name: "article".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![],
    }];
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "article".into(),
            column: "title".into(),
            nullable: false,
            fill_with: Some("'untitled'".into()),
            delete_null_rows: None,
        }],
    };
    (plan, schema)
}

fn backend_sql(queries: &[PlanQueries], backend: DatabaseBackend) -> Vec<String> {
    queries
        .iter()
        .flat_map(|pq| match backend {
            DatabaseBackend::Postgres => &pq.postgres,
            DatabaseBackend::MySql => &pq.mysql,
            DatabaseBackend::Sqlite => &pq.sqlite,
        })
        .map(|q| q.build(backend))
        .collect()
}

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn build_plan_queries_with_options_wraps_temp_table_rebuild_in_transaction(
    #[case] backend: DatabaseBackend,
) {
    let (plan, schema) = nullable_rebuild_plan();
    let result = build_plan_queries_with_options(
        &plan,
        &schema,
        PlanQueriesOptions {
            wrap_in_transaction: true,
        },
    )
    .unwrap();
    let sql = backend_sql(&result, backend);

    assert!(sql.len() > 2, "expected multi-statement migration output");
    assert_eq!(sql.first().map(String::as_str), Some("BEGIN;"));
    assert_eq!(sql.last().map(String::as_str), Some("COMMIT;"));
}

#[test]
fn build_plan_queries_keeps_transaction_wrapping_opt_in() {
    let (plan, schema) = nullable_rebuild_plan();
    let result = build_plan_queries(&plan, &schema).unwrap();
    let sql = backend_sql(&result, DatabaseBackend::Sqlite);

    assert_ne!(sql.first().map(String::as_str), Some("BEGIN;"));
    assert_ne!(sql.last().map(String::as_str), Some("COMMIT;"));
}
