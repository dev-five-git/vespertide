use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan,
    SimpleColumnType, TableDef,
};
use vespertide_query::{build_plan_queries, DatabaseBackend};

#[test]
fn test_enum_value_change_generates_correct_sql() {
    // Simulate migration 0003: changing enum from ["active", "inactive"] to ["active", "inactive", "pending"]
    let plan = MigrationPlan {
        id: String::new(),
        comment: Some("Fix enum".into()),
        created_at: Some("2025-12-17T07:57:14Z".into()),
        version: 3,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "user".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec![
                    "active".into(),
                    "inactive".into(),
                    "pending".into(),
                ]),
            }),
            fill_with: None,
        }],
    };

    // Baseline schema after migration 0002 (with 2-value enum)
    let baseline_schema = vec![TableDef {
        name: "user".into(),
        description: None,
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
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
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

    // 1. CREATE TYPE user_status_new (table-prefixed)
    assert!(
        sql0.contains("CREATE TYPE \"user_status_new\""),
        "Step 1 should create temp type with table prefix. Got: {}",
        sql0
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
        sql1.contains("ALTER COLUMN \"status\" TYPE \"user_status_new\""),
        "Should change column type to temp with table prefix. Got: {}",
        sql1
    );
    assert!(
        sql1.contains("USING \"status\"::text::\"user_status_new\""),
        "Should use USING clause with table prefix. Got: {}",
        sql1
    );

    // 3. DROP TYPE user_status (table-prefixed)
    assert!(
        sql2.contains("DROP TYPE \"user_status\""),
        "Step 3 should drop old type with table prefix. Got: {}",
        sql2
    );

    // 4. RENAME TYPE user_status_new to user_status (table-prefixed)
    assert!(
        sql3.contains("ALTER TYPE \"user_status_new\" RENAME TO \"user_status\""),
        "Step 4 should rename temp type back with table prefix. Got: {}",
        sql3
    );
}
