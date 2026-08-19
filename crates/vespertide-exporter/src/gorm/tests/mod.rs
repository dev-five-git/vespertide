use std::collections::HashMap;

use insta::assert_snapshot;
use rstest::rstest;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, DefaultValue, NumValue, ReferenceAction, TableDef};

use super::{
    GormExporterWithConfig, go_type_for_column_mapped, infer_relation_field_name,
    needs_table_name_method, render_entity, render_entity_with_schema, to_go_field_name,
};

mod relations;

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

// -----------------------------------------------------------------------
// Type mapping unit tests
// -----------------------------------------------------------------------

#[rstest]
#[case(ColumnType::Simple(SimpleColumnType::SmallInt), false, "int16")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), false, "int32")]
#[case(ColumnType::Simple(SimpleColumnType::BigInt), false, "int64")]
#[case(ColumnType::Simple(SimpleColumnType::Real), false, "float32")]
#[case(
    ColumnType::Simple(SimpleColumnType::DoublePrecision),
    false,
    "float64"
)]
#[case(ColumnType::Simple(SimpleColumnType::Text), false, "string")]
#[case(ColumnType::Simple(SimpleColumnType::Boolean), false, "bool")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamptz), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Date), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Time), false, "time.Time")]
#[case(ColumnType::Simple(SimpleColumnType::Uuid), false, "uuid.UUID")]
#[case(ColumnType::Simple(SimpleColumnType::Json), false, "datatypes.JSON")]
#[case(ColumnType::Simple(SimpleColumnType::Bytea), false, "[]byte")]
#[case(ColumnType::Simple(SimpleColumnType::Inet), false, "string")]
#[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }), false, "string")]
#[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }), false, "decimal.Decimal")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "JSONB".into() }), false, "datatypes.JSON")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "jsonb".into() }), false, "datatypes.JSON")]
#[case(ColumnType::Complex(ComplexColumnType::Custom { custom_type: "TEXT".into() }), false, "string")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), true, "*int32")]
#[case(ColumnType::Simple(SimpleColumnType::Text), true, "*string")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), true, "*time.Time")]
fn test_go_type_mapping(
    #[case] col_type: ColumnType,
    #[case] nullable: bool,
    #[case] expected: &str,
) {
    assert_eq!(
        go_type_for_column_mapped(&col_type, nullable, &HashMap::new()),
        expected
    );
}

#[rstest]
#[case("user_id", "UserID")]
#[case("id", "ID")]
#[case("created_at", "CreatedAt")]
#[case("profile_image", "ProfileImage")]
#[case("media_id", "MediaID")]
fn test_to_go_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_go_field_name(input), expected);
}

#[rstest]
#[case("user_id", "User")]
#[case("author_id", "Author")]
#[case("parent_id", "Parent")]
#[case("node", "Node")]
fn test_infer_relation_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(infer_relation_field_name(input), expected);
}

#[rstest]
#[case("User", "user", true)]
#[case("User", "users", false)]
#[case("OrderItem", "order_items", false)]
#[case("OrderItem", "order_item", true)]
fn test_needs_table_name_method(
    #[case] struct_name: &str,
    #[case] table_name: &str,
    #[case] expected: bool,
) {
    assert_eq!(needs_table_name_method(table_name, struct_name), expected);
}

// -----------------------------------------------------------------------
// Snapshot tests (a) simple table - columns only
// -----------------------------------------------------------------------

#[test]
fn test_basic_table() {
    let table = TableDef {
        name: "users".into(),
        description: Some("User accounts".into()),
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: Some("Primary key".into()),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "active".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some(DefaultValue::Bool(true)),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Snapshot tests (b) FK
// -----------------------------------------------------------------------

#[test]
fn test_table_with_foreign_key() {
    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "author_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::Index {
                name: Some("ix_posts__author_id".into()),
                columns: vec!["author_id".into()],
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Snapshot tests (c) enums
// -----------------------------------------------------------------------

#[test]
fn test_table_with_string_enum() {
    let table = TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "shipped".into(),
                        "delivered".into(),
                    ]),
                }),
                nullable: false,
                default: Some(DefaultValue::String("'pending'".into())),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

#[test]
fn test_table_with_integer_enum() {
    let table = TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "medium".into(),
                            value: 10,
                        },
                        NumValue {
                            name: "high".into(),
                            value: 20,
                        },
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
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Snapshot tests (d) composite PK + nullable
// -----------------------------------------------------------------------

#[test]
fn test_composite_pk_nullable() {
    let table = TableDef {
        name: "order_items".into(),
        description: None,
        columns: vec![
            col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "quantity".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "note".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["order_id".into(), "product_id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["order_id".into()],
                ref_table: "orders".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["product_id".into()],
                ref_table: "products".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::Unique {
                name: Some("uq_order_items__order_product".into()),
                columns: vec!["order_id".into(), "product_id".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// All simple types
// -----------------------------------------------------------------------

#[test]
fn test_all_simple_types() {
    let table = TableDef {
        name: "type_test".into(),
        description: None,
        columns: vec![
            col(
                "col_smallint",
                ColumnType::Simple(SimpleColumnType::SmallInt),
            ),
            col("col_integer", ColumnType::Simple(SimpleColumnType::Integer)),
            col("col_bigint", ColumnType::Simple(SimpleColumnType::BigInt)),
            col("col_real", ColumnType::Simple(SimpleColumnType::Real)),
            col(
                "col_double",
                ColumnType::Simple(SimpleColumnType::DoublePrecision),
            ),
            col("col_text", ColumnType::Simple(SimpleColumnType::Text)),
            col("col_boolean", ColumnType::Simple(SimpleColumnType::Boolean)),
            col("col_date", ColumnType::Simple(SimpleColumnType::Date)),
            col("col_time", ColumnType::Simple(SimpleColumnType::Time)),
            col(
                "col_timestamp",
                ColumnType::Simple(SimpleColumnType::Timestamp),
            ),
            col(
                "col_timestamptz",
                ColumnType::Simple(SimpleColumnType::Timestamptz),
            ),
            col(
                "col_interval",
                ColumnType::Simple(SimpleColumnType::Interval),
            ),
            col("col_bytea", ColumnType::Simple(SimpleColumnType::Bytea)),
            col("col_uuid", ColumnType::Simple(SimpleColumnType::Uuid)),
            col("col_json", ColumnType::Simple(SimpleColumnType::Json)),
            col("col_inet", ColumnType::Simple(SimpleColumnType::Inet)),
            col("col_cidr", ColumnType::Simple(SimpleColumnType::Cidr)),
            col("col_macaddr", ColumnType::Simple(SimpleColumnType::Macaddr)),
            col("col_xml", ColumnType::Simple(SimpleColumnType::Xml)),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["col_integer".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Snapshot tests (e) JSONB custom type
// -----------------------------------------------------------------------

#[test]
fn test_table_with_jsonb_column() {
    let table = TableDef {
        name: "documents".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "data",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "JSONB".into(),
                }),
            ),
            col("meta", ColumnType::Simple(SimpleColumnType::Json)),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Snapshot tests (f) server-side default skipped
// -----------------------------------------------------------------------

#[test]
fn test_server_default_skipped() {
    let table = TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "created_at".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                nullable: false,
                default: Some(DefaultValue::String("NOW()".into())),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "count".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: Some(DefaultValue::Integer(0)),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    // Server-side function calls must not appear as GORM default tags
    assert!(!result.contains("default:NOW()"));
    // Literal integer defaults are still included
    assert!(result.contains("default:0"));
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Conflicting enum names across tables → qualified Go type name
// -----------------------------------------------------------------------

#[test]
fn test_conflicting_enum_names_qualified() {
    let orders = TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec!["pending".into(), "done".into()]),
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let tasks = TableDef {
        name: "tasks".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status".into(),
                    values: EnumValues::String(vec!["open".into(), "closed".into()]),
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let schema = vec![orders.clone(), tasks];
    let result = render_entity_with_schema(&orders, &schema).unwrap();
    assert!(
        result.contains("OrdersStatus"),
        "Expected qualified enum name 'OrdersStatus' in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Char column → type:char + size in GORM tag
// -----------------------------------------------------------------------

#[test]
fn test_char_type_column() {
    let table = TableDef {
        name: "codes".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "code",
                ColumnType::Complex(ComplexColumnType::Char { length: 3 }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("type:char"),
        "Expected type:char in GORM tag"
    );
    assert!(result.contains("size:3"), "Expected size:3 in GORM tag");
}

// -----------------------------------------------------------------------
// FK field name collision: infer == go_field → disambiguate with ref struct
// -----------------------------------------------------------------------

#[test]
fn test_fk_relation_field_name_collision() {
    // Column "user" (no _id suffix): infer→"User", go_field→"User" → same → "UserUsers"
    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("UserUsers"),
        "Expected disambiguated relation name 'UserUsers' in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Reverse relation disambiguation: two FKs to same target → ByField suffix
// -----------------------------------------------------------------------

#[test]
fn test_reverse_relation_disambiguation() {
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let events = TableDef {
        name: "events".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("creator_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("attendee_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["creator_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["attendee_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let schema = vec![users.clone(), events];
    let result = render_entity_with_schema(&users, &schema).unwrap();
    assert!(
        result.contains("EventsByCreatorID"),
        "Expected 'EventsByCreatorID' in:\n{result}"
    );
    assert!(
        result.contains("EventsByAttendeeID"),
        "Expected 'EventsByAttendeeID' in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Snapshot tests (g) HasMany with on_delete/on_update constraint
// -----------------------------------------------------------------------

#[test]
fn test_has_many_with_constraint() {
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let schema = vec![users.clone(), posts.clone()];
    let result = render_entity_with_schema(&users, &schema).unwrap();
    assert!(result.contains("OnDelete:CASCADE"));
    assert!(result.contains("OnUpdate:RESTRICT"));
    assert_snapshot!(result);
}

// -----------------------------------------------------------------------
// Numeric column: add_column_type needs_decimal, build_gorm_tag Numeric, decimal import
// -----------------------------------------------------------------------

#[test]
fn test_numeric_column_gorm() {
    let table = TableDef {
        name: "prices".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            ),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("type:numeric(10,2)"),
        "expected numeric GORM tag"
    );
    assert!(
        result.contains("decimal.Decimal"),
        "expected decimal.Decimal type"
    );
    assert!(
        result.contains("github.com/shopspring/decimal"),
        "expected decimal import"
    );
}

// -----------------------------------------------------------------------
// Unnamed Index: build_gorm_tag unnamed index tag + collect_index_info inner body
// -----------------------------------------------------------------------

#[test]
fn test_unnamed_index_gorm() {
    let table = TableDef {
        name: "searches".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("query", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["query".into()],
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(result.contains(";index\""), "expected unnamed index tag");
}

// -----------------------------------------------------------------------
// Named Index: build_gorm_tag named index tag + collect_index_info name closure
// -----------------------------------------------------------------------

#[test]
fn test_named_index_gorm() {
    let table = TableDef {
        name: "searches".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("query", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Index {
                name: Some("ix_searches__query".into()),
                columns: vec!["query".into()],
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("index:ix_searches__query"),
        "expected named index tag"
    );
}

// -----------------------------------------------------------------------
// Unnamed composite unique: collect_composite_unique_info auto-name (uq_{cols})
// -----------------------------------------------------------------------

#[test]
fn test_unnamed_composite_unique_gorm() {
    let table = TableDef {
        name: "order_lines".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "sku",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
            ),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["order_id".into(), "sku".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("uniqueIndex:uq_order_id_sku"),
        "expected auto-generated uniqueIndex name"
    );
}

// -----------------------------------------------------------------------
// Singular source table name: find_reverse_relations appends 's' for non-plural
// -----------------------------------------------------------------------

#[test]
fn test_singular_source_table_name() {
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let comment = TableDef {
        name: "comment".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let schema = vec![user.clone(), comment];
    let result = render_entity_with_schema(&user, &schema).unwrap();
    // "comment" → pascal "Comment" → doesn't end with 's' → appended 's' → "Comments"
    assert!(
        result.contains("Comments"),
        "expected 'Comments' plural for singular 'comment' table"
    );
}

// -----------------------------------------------------------------------
// FK with on_update + nullable column: render_fk_relation_field lines 547, 559-560
// -----------------------------------------------------------------------

#[test]
fn test_fk_with_on_update_and_nullable() {
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "author_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let result = render_entity(&posts).unwrap();
    assert!(
        result.contains("OnUpdate:RESTRICT"),
        "expected OnUpdate constraint"
    );
    assert!(
        result.contains("*Users"),
        "expected nullable FK pointer type"
    );
}

// -----------------------------------------------------------------------
// reference_action_str: SetNull, SetDefault, NoAction (via FK on_delete)
// -----------------------------------------------------------------------

#[rstest]
#[case(ReferenceAction::SetNull, "SET NULL")]
#[case(ReferenceAction::SetDefault, "SET DEFAULT")]
#[case(ReferenceAction::NoAction, "NO ACTION")]
fn test_gorm_fk_on_delete_actions(#[case] action: ReferenceAction, #[case] expected: &str) {
    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(action),
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains(&format!("OnDelete:{expected}")),
        "expected OnDelete:{expected} in:\n{result}"
    );
}

// -----------------------------------------------------------------------
// to_pascal_case: None arm via double-underscore table name
// -----------------------------------------------------------------------

#[test]
fn test_gorm_double_underscore_table_name() {
    // "order__item" splits into ["order", "", "item"] → empty word hits None => String::new()
    let table = TableDef {
        name: "order__item".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("type OrderItem struct"),
        "expected pascal-cased struct with double underscore"
    );
}

// -----------------------------------------------------------------------
// collect_composite_unique_info: named branch (|n| n.as_str().to_owned())
// -----------------------------------------------------------------------

#[test]
fn test_named_composite_unique_gorm() {
    let table = TableDef {
        name: "tenants".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Unique {
                name: Some("uq_tenant_name".into()),
                columns: vec!["tenant_id".into(), "name".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("uniqueIndex:uq_tenant_name"),
        "expected named uniqueIndex tag in GORM output"
    );
}

// -----------------------------------------------------------------------
// GormExporterWithConfig: package_name reaches the `package` declaration
// -----------------------------------------------------------------------

fn simple_table() -> TableDef {
    TableDef {
        name: "users".into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    }
}

#[test]
fn test_default_package_name_is_models() {
    let table = simple_table();
    let exporter = GormExporterWithConfig::new("models");
    let result = exporter.render_entity(&table).unwrap();
    assert!(
        result.starts_with("package models\n"),
        "expected default 'package models', got:\n{result}"
    );
}

#[test]
fn test_custom_package_name_from_config() {
    let table = simple_table();
    let exporter = GormExporterWithConfig::new("entities");
    let result = exporter.render_entity(&table).unwrap();
    assert!(
        result.starts_with("package entities\n"),
        "expected 'package entities', got:\n{result}"
    );
}

#[test]
fn test_custom_package_name_with_schema_context() {
    let table = simple_table();
    let schema = vec![table.clone()];
    let exporter = GormExporterWithConfig::new("entities");
    let result = exporter.render_entity_with_schema(&table, &schema).unwrap();
    assert!(
        result.starts_with("package entities\n"),
        "expected 'package entities', got:\n{result}"
    );
}
