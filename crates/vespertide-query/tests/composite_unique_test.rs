use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType,
    schema::StrOrBoolOrArray,
};
use vespertide_query::{DatabaseBackend, build_plan_queries};

#[test]
fn test_composite_unique_constraint_generates_single_index() {
    // Test that multiple columns with the same unique constraint name
    // generate a single composite unique index, not separate indexes per column
    let plan = MigrationPlan {
        version: 1,
        comment: Some("Test composite unique".into()),
        created_at: None,
        actions: vec![MigrationAction::CreateTable {
            table: "user".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(
                        vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true),
                    ),
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "join_route".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: Some(StrOrBoolOrArray::Str("route_provider_id".into())),
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "provider_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: Some(StrOrBoolOrArray::Str("route_provider_id".into())),
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
        }],
    };

    let queries = build_plan_queries(&plan, &[]).unwrap();

    let postgres_sql = &queries[0].postgres;
    println!("\n=== PostgreSQL SQL ===");
    for (i, q) in postgres_sql.iter().enumerate() {
        let sql = q.build(DatabaseBackend::Postgres);
        println!("{}: {}", i + 1, sql);
    }

    // Should have 2 queries: CREATE TABLE and CREATE UNIQUE INDEX
    assert_eq!(
        postgres_sql.len(),
        2,
        "Should have CREATE TABLE and one CREATE UNIQUE INDEX"
    );

    let create_table_sql = postgres_sql[0].build(DatabaseBackend::Postgres);
    assert!(
        create_table_sql.contains("CREATE TABLE \"user\""),
        "Should create user table"
    );

    let create_unique_sql = postgres_sql[1].build(DatabaseBackend::Postgres);
    println!("\nGenerated unique index SQL: {}", create_unique_sql);

    // Should create a single composite unique index, not two separate ones
    assert!(
        create_unique_sql.contains("CREATE UNIQUE INDEX"),
        "Should create unique index"
    );
    assert!(
        create_unique_sql.contains("\"uq_user__route_provider_id\""),
        "Should use the named constraint. Got: {}",
        create_unique_sql
    );
    assert!(
        create_unique_sql.contains("(\"join_route\", \"provider_id\")"),
        "Should include both columns in composite index. Got: {}",
        create_unique_sql
    );

    println!("\n✅ Composite unique constraint correctly generates a single index!");
}
