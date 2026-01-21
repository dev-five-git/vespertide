#[cfg(test)]
mod test_utils {
    use vespertide_core::{
        ColumnDef, ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan,
        SimpleColumnType,
    };
    use vespertide_query::{DatabaseBackend, build_plan_queries};
    #[test]
    fn test_table_prefixed_enum_names() {
        // Test that enum types are created with table-prefixed names to avoid conflicts
        let plan = MigrationPlan {
            version: 1,
            comment: Some("Test enum naming".into()),
            created_at: None,
            actions: vec![
                // Create users table with status enum
                MigrationAction::CreateTable {
                    table: "users".into(),
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
                            name: "status".into(),
                            r#type: ColumnType::Complex(ComplexColumnType::Enum {
                                name: "status".into(),
                                values: EnumValues::String(vec![
                                    "active".into(),
                                    "inactive".into(),
                                ]),
                            }),
                            nullable: false,
                            default: None,
                            comment: None,
                            primary_key: None,
                            unique: None,
                            index: None,
                            foreign_key: None,
                        },
                    ],
                    constraints: vec![],
                },
                // Create orders table with status enum (same name, different table)
                MigrationAction::CreateTable {
                    table: "orders".into(),
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
                            name: "status".into(),
                            r#type: ColumnType::Complex(ComplexColumnType::Enum {
                                name: "status".into(),
                                values: EnumValues::String(vec![
                                    "pending".into(),
                                    "shipped".into(),
                                    "delivered".into(),
                                ]),
                            }),
                            nullable: false,
                            default: None,
                            comment: None,
                            primary_key: None,
                            unique: None,
                            index: None,
                            foreign_key: None,
                        },
                    ],
                    constraints: vec![],
                },
            ],
        };

        let queries = build_plan_queries(&plan, &[]).unwrap();

        // Check users table enum type
        let users_sql = &queries[0].postgres;
        let create_users_enum = users_sql[0].build(DatabaseBackend::Postgres);
        assert!(
            create_users_enum.contains("CREATE TYPE \"users_status\""),
            "Should create users_status enum type. Got: {}",
            create_users_enum
        );
        assert!(
            create_users_enum.contains("'active', 'inactive'"),
            "Should include user status values"
        );

        let create_users_table = users_sql[1].build(DatabaseBackend::Postgres);
        assert!(
            create_users_table.contains("users_status"),
            "Users table should use users_status type. Got: {}",
            create_users_table
        );

        // Check orders table enum type
        let orders_sql = &queries[1].postgres;
        let create_orders_enum = orders_sql[0].build(DatabaseBackend::Postgres);
        assert!(
            create_orders_enum.contains("CREATE TYPE \"orders_status\""),
            "Should create orders_status enum type. Got: {}",
            create_orders_enum
        );
        assert!(
            create_orders_enum.contains("'pending', 'shipped', 'delivered'"),
            "Should include order status values"
        );

        let create_orders_table = orders_sql[1].build(DatabaseBackend::Postgres);
        assert!(
            create_orders_table.contains("orders_status"),
            "Orders table should use orders_status type. Got: {}",
            create_orders_table
        );

        println!("\n=== Users Table SQL ===");
        for (i, q) in users_sql.iter().enumerate() {
            println!("{}: {}", i + 1, q.build(DatabaseBackend::Postgres));
        }

        println!("\n=== Orders Table SQL ===");
        for (i, q) in orders_sql.iter().enumerate() {
            println!("{}: {}", i + 1, q.build(DatabaseBackend::Postgres));
        }

        println!("\n✅ Table-prefixed enum names successfully prevent naming conflicts!");
    }
}
