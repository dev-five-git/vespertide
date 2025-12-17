use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, MigrationAction, MigrationPlan, SimpleColumnType,
    TableDef,
};
use vespertide_query::{DatabaseBackend, build_plan_queries};

#[test]
fn test_enum_value_change_generates_correct_sql() {
    // Simulate migration 0003: changing enum from ["active", "inactive"] to ["active", "inactive", "pending"]
    let plan = MigrationPlan {
        comment: Some("Fix enum".into()),
        created_at: Some("2025-12-17T07:57:14Z".into()),
        version: 3,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "user".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: vec!["active".into(), "inactive".into(), "pending".into()],
            }),
        }],
    };

    // Baseline schema after migration 0002 (with 2-value enum)
    let baseline_schema = vec![TableDef {
        name: "user".into(),
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: Some("gen_random_uuid()".into()),
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: vec!["active".into(), "inactive".into()],
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    }];

    let queries = build_plan_queries(&plan, &baseline_schema).unwrap();

    assert_eq!(queries.len(), 1, "Should have 1 action");

    let postgres_queries = &queries[0].postgres;
    println!("\n=== PostgreSQL SQL for enum migration ===");
    for (i, q) in postgres_queries.iter().enumerate() {
        let sql = q.build(DatabaseBackend::Postgres);
        println!("{}: {}", i + 1, sql);
    }

    // Should have 4 queries for the temp type approach
    assert_eq!(
        postgres_queries.len(),
        4,
        "Should generate 4 SQL statements for enum value change"
    );

    let sql0 = postgres_queries[0].build(DatabaseBackend::Postgres);
    let sql1 = postgres_queries[1].build(DatabaseBackend::Postgres);
    let sql2 = postgres_queries[2].build(DatabaseBackend::Postgres);
    let sql3 = postgres_queries[3].build(DatabaseBackend::Postgres);

    // 1. CREATE TYPE status_new
    assert!(
        sql0.contains("CREATE TYPE \"status_new\""),
        "Step 1 should create temp type"
    );
    assert!(
        sql0.contains("'active', 'inactive', 'pending'"),
        "Should include all 3 enum values"
    );

    // 2. ALTER TABLE with USING
    assert!(
        sql1.contains("ALTER TABLE \"user\""),
        "Step 2 should alter table"
    );
    assert!(
        sql1.contains("ALTER COLUMN \"status\" TYPE \"status_new\""),
        "Should change column type to temp"
    );
    assert!(
        sql1.contains("USING \"status\"::text::\"status_new\""),
        "Should use USING clause"
    );

    // 3. DROP TYPE status
    assert!(
        sql2.contains("DROP TYPE \"status\""),
        "Step 3 should drop old type"
    );

    // 4. RENAME TYPE
    assert!(
        sql3.contains("ALTER TYPE \"status_new\" RENAME TO \"status\""),
        "Step 4 should rename temp type back"
    );
}
