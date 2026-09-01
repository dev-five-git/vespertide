use super::*;
use rstest::rstest;
use std::collections::HashSet;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, TableConstraint,
    TableDef,
};

#[test]
fn test_render_indexes() {
    let mut lines = Vec::new();
    let constraints = vec![
        TableConstraint::Index {
            name: Some("idx_users_email".into()),
            columns: vec!["email".into()],
        },
        TableConstraint::Index {
            name: Some("idx_users_name_email".into()),
            columns: vec!["name".into(), "email".into()],
        },
    ];
    render_indexes_and_uniques(&mut lines, &constraints);
    assert!(!lines.is_empty());
    assert!(lines.iter().any(|l| l.contains("idx_users_email")));
    assert!(lines.iter().any(|l| l.contains("idx_users_name_email")));
}

#[test]
fn test_render_indexes_empty() {
    let mut lines = Vec::new();
    render_indexes_and_uniques(&mut lines, &[]);
    assert_eq!(lines.len(), 0);
}

#[rstest]
#[case(ColumnType::Simple(SimpleColumnType::SmallInt), false, "i16")]
#[case(ColumnType::Simple(SimpleColumnType::SmallInt), true, "Option<i16>")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), false, "i32")]
#[case(ColumnType::Simple(SimpleColumnType::Integer), true, "Option<i32>")]
#[case(ColumnType::Simple(SimpleColumnType::BigInt), false, "i64")]
#[case(ColumnType::Simple(SimpleColumnType::BigInt), true, "Option<i64>")]
#[case(ColumnType::Simple(SimpleColumnType::Real), false, "f32")]
#[case(ColumnType::Simple(SimpleColumnType::DoublePrecision), false, "f64")]
#[case(ColumnType::Simple(SimpleColumnType::Text), false, "String")]
#[case(ColumnType::Simple(SimpleColumnType::Text), true, "Option<String>")]
#[case(ColumnType::Simple(SimpleColumnType::Boolean), false, "bool")]
#[case(ColumnType::Simple(SimpleColumnType::Boolean), true, "Option<bool>")]
#[case(ColumnType::Simple(SimpleColumnType::Date), false, "Date")]
#[case(ColumnType::Simple(SimpleColumnType::Time), false, "Time")]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp), false, "DateTime")]
#[case(
    ColumnType::Simple(SimpleColumnType::Timestamp),
    true,
    "Option<DateTime>"
)]
#[case(
    ColumnType::Simple(SimpleColumnType::Timestamptz),
    false,
    "DateTimeWithTimeZone"
)]
#[case(
    ColumnType::Simple(SimpleColumnType::Timestamptz),
    true,
    "Option<DateTimeWithTimeZone>"
)]
#[case(ColumnType::Simple(SimpleColumnType::Bytea), false, "Vec<u8>")]
#[case(ColumnType::Simple(SimpleColumnType::Uuid), false, "Uuid")]
#[case(ColumnType::Simple(SimpleColumnType::Json), false, "Json")]
#[case(ColumnType::Simple(SimpleColumnType::Inet), false, "String")]
#[case(ColumnType::Simple(SimpleColumnType::Cidr), false, "String")]
#[case(ColumnType::Simple(SimpleColumnType::Macaddr), false, "String")]
#[case(ColumnType::Simple(SimpleColumnType::Interval), false, "String")]
#[case(ColumnType::Simple(SimpleColumnType::Xml), false, "String")]
#[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }), false, "Decimal")]
#[case(ColumnType::Complex(ComplexColumnType::Char { length: 10 }), false, "String")]
fn test_rust_type(#[case] col_type: ColumnType, #[case] nullable: bool, #[case] expected: &str) {
    assert_eq!(col_type.to_rust_type(nullable), expected);
}

#[rstest]
#[case("normal_name", "normal_name")]
// `_123name` would make `DeriveEntityModel` build the `Column` variant
// `123name`, which is not a valid identifier, so the escape is a letter.
#[case("123name", "x123name")]
#[case("name-with-dash", "name_with_dash")]
#[case("name.with.dot", "name_with_dot")]
#[case("name with space", "name_with_space")]
#[case("name  with  multiple  spaces", "name__with__multiple__spaces")]
#[case(" name_with_leading_space", "x_name_with_leading_space")]
#[case("name_with_trailing_space ", "name_with_trailing_space_")]
#[case("", "_col")]
#[case("a", "a")]
// Reserved keywords should be prefixed with r#
#[case("type", "r#type")]
#[case("ref", "r#ref")]
#[case("mod", "r#mod")]
#[case("fn", "r#fn")]
#[case("let", "r#let")]
#[case("mut", "r#mut")]
#[case("pub", "r#pub")]
#[case("self", "r#self")]
#[case("Self", "r#Self")]
#[case("match", "r#match")]
#[case("async", "r#async")]
#[case("await", "r#await")]
#[case("abstract", "r#abstract")]
// Non-reserved words should not be prefixed
#[case("types", "types")]
#[case("reference", "reference")]
#[case("module", "module")]
fn test_sanitize_field_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(sanitize_field_name(input), expected);
}

#[test]
fn test_unique_name() {
    let mut used = std::collections::HashSet::new();
    assert_eq!(unique_name("test", &mut used), "test");
    assert_eq!(unique_name("test", &mut used), "test_1");
    assert_eq!(unique_name("test", &mut used), "test_2");
    assert_eq!(unique_name("other", &mut used), "other");
    assert_eq!(unique_name("other", &mut used), "other_1");
}

#[test]
fn test_unique_relation_enum_name_preferred_available() {
    let used = HashSet::new();
    let result = unique_relation_enum_name("User".into(), "post", "User", &used);
    assert_eq!(result, "User");
}

#[test]
fn test_unique_relation_enum_name_source_prefixed() {
    let mut used = HashSet::new();
    used.insert("User".into());
    let result = unique_relation_enum_name("User".into(), "post", "User", &used);
    assert_eq!(result, "PostUser");
}

#[test]
fn test_unique_relation_enum_name_numbered_fallback() {
    let mut used = HashSet::new();
    used.insert("User".into());
    used.insert("PostUser".into());
    let result = unique_relation_enum_name("User".into(), "post", "User", &used);
    assert_eq!(result, "PostUser2");
}

#[test]
fn test_unique_relation_enum_name_numbered_fallback_skips_taken() {
    let mut used = HashSet::new();
    used.insert("User".into());
    used.insert("PostUser".into());
    used.insert("PostUser2".into());
    let result = unique_relation_enum_name("User".into(), "post", "User", &used);
    assert_eq!(result, "PostUser3");
}

#[rstest]
#[case(vec!["creator_user_id".into()], "CreatorUser")]
#[case(vec!["used_by_user_id".into()], "UsedByUser")]
#[case(vec!["user_id".into()], "User")]
#[case(vec!["org_id".into()], "Org")]
#[case(vec!["org_id".into(), "user_id".into()], "Org")]
#[case(vec!["author_id".into()], "Author")]
// FK column WITHOUT _id suffix (coverage for line 428)
#[case(vec!["creator_user".into()], "CreatorUser")]
#[case(vec!["user".into()], "User")]
fn test_generate_relation_enum_name(#[case] columns: Vec<String>, #[case] expected: &str) {
    assert_eq!(generate_relation_enum_name(&columns), expected);
}

/// The name becomes a `Relation` enum variant, so a non-ASCII column has to
/// come back as something Rust can actually declare.
#[test]
fn test_generate_relation_enum_name_escapes_non_ascii() {
    assert_eq!(generate_relation_enum_name(&["📊_stats_id"]), "X_Stats");
}

#[rstest]
// FK column ends with table name -> use the FK column name
#[case("creator_user_id", "user", "id", "creator_user")]
#[case("used_by_user_id", "user", "id", "used_by_user")]
#[case("author_user_id", "user", "id", "author_user")]
// FK column is same as table -> fall back to table name
#[case("user_id", "user", "id", "user")]
#[case("org_id", "org", "id", "org")]
#[case("post_id", "post", "id", "post")]
// FK column doesn't end with table name -> use FK column name
#[case("author_id", "user", "id", "author")]
#[case("owner_id", "user", "id", "owner")]
// FK column WITHOUT _id suffix (coverage for line 450)
#[case("creator_user", "user", "id", "creator_user")]
#[case("user", "user", "id", "user")]
#[case("username", "user", "name", "user")]
#[case("username", "admin", "username", "admin")]
// FK column exactly matches table name with _id (coverage for line 464)
#[case("customer_id", "customer", "id", "customer")]
#[case("product_id", "product", "id", "product")]
// Test with different "to" suffixes (e.g., _idx instead of _id)
#[case("creator_user_idx", "user", "idx", "creator_user")]
#[case("user_idx", "user", "idx", "user")]
#[case("author_pk", "user", "pk", "author")]
// FK column keeps *_id naming while target column is not "id"
#[case("order_id", "order", "order_number", "order")]
#[case("creator_order_id", "order", "order_number", "creator_order")]
// FK column keeps *_idx naming while target column is not "idx"
#[case("order_idx", "order", "order_number", "order")]
#[case("creator_order_idx", "order", "order_number", "creator_order")]
fn test_infer_field_name_from_fk_column(
    #[case] fk_column: &str,
    #[case] table_name: &str,
    #[case] to: &str,
    #[case] expected: &str,
) {
    assert_eq!(
        infer_field_name_from_fk_column(fk_column, table_name, to),
        expected
    );
}

#[test]
fn test_column_type_supports_eq() {
    assert!(column_type_supports_eq(&ColumnType::Simple(
        SimpleColumnType::Integer
    )));
    assert!(column_type_supports_eq(&ColumnType::Simple(
        SimpleColumnType::Text
    )));
    assert!(!column_type_supports_eq(&ColumnType::Simple(
        SimpleColumnType::Real
    )));
    assert!(!column_type_supports_eq(&ColumnType::Simple(
        SimpleColumnType::DoublePrecision
    )));
    assert!(column_type_supports_eq(&ColumnType::Complex(
        ComplexColumnType::Numeric {
            precision: 10,
            scale: 2
        }
    )));
}

#[rstest]
#[case("hello_world", "HelloWorld")]
#[case("order_status", "OrderStatus")]
#[case("hello-world", "HelloWorld")]
#[case("info-level", "InfoLevel")]
#[case("HelloWorld", "HelloWorld")]
#[case("hello", "Hello")]
#[case("pending", "Pending")]
#[case("hello_world-test", "HelloWorldTest")]
#[case("HELLO_WORLD", "HELLOWORLD")]
#[case("ERROR_LEVEL", "ERRORLEVEL")]
#[case("level_1", "Level1")]
#[case("1_critical", "1Critical")]
#[case("", "")]
fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_pascal_case(input), expected);
}

#[rstest]
#[case("CreatorUser", "creator_user")]
#[case("UsedByUser", "used_by_user")]
#[case("PreferredUser", "preferred_user")]
#[case("BackupUser", "backup_user")]
#[case("User", "user")]
#[case("ID", "i_d")]
fn test_to_snake_case(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(to_snake_case(input), expected);
}

#[rstest]
#[case("pending", "Pending")]
#[case("in_stock", "InStock")]
#[case("info-level", "InfoLevel")]
#[case("ACTIVE", "ACTIVE")]
#[case("ERROR_LEVEL", "ERRORLEVEL")]
#[case("1critical", "N1critical")]
#[case("123abc", "N123abc")]
#[case("1_critical", "N1Critical")]
#[case("", "Value")]
fn test_enum_variant_name(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(enum_variant_name(input), expected);
}

#[test]
fn test_render_enum_uses_screaming_snake_serde_for_uppercase_values() {
    let mut lines = Vec::new();
    let config = SeaOrmConfig::default();
    let values = EnumValues::String(vec!["PENDING".into(), "IN_PROGRESS".into()]);

    render_enum(&mut lines, "orders", "order_status", &values, &config);

    let result = lines.join("\n");
    assert!(result.contains("#[serde(rename_all = \"SCREAMING_SNAKE_CASE\")]"));
    assert!(result.contains("    #[sea_orm(string_value = \"PENDING\")]\n    Pending,"));
    assert!(result.contains("    #[sea_orm(string_value = \"IN_PROGRESS\")]\n    InProgress,"));
}

#[test]
fn test_is_screaming_snake_value_rejects_invalid_symbol() {
    assert!(!is_screaming_snake_value("PENDING-REVIEW"));
}

#[test]
fn test_screaming_snake_to_pascal_case_ignores_empty_segments() {
    assert_eq!(
        screaming_snake_to_pascal_case("PENDING__REVIEW"),
        "PendingReview"
    );
}

#[rstest]
#[case("___")]
#[case("")]
#[case("_")]
fn test_screaming_snake_to_pascal_case_all_empty_segments_returns_identifier(#[case] input: &str) {
    assert_eq!(screaming_snake_to_pascal_case(input), "Value");
}

// `render_enum` line/branch coverage is provided by:
//   * Every cross-ORM `orm_cases!` enum scenario in `crate::tests::mod`
//     (e.g. `table_with_enum`, `enum_special_values`, `enum_with_default`,
//     `table_with_integer_enum`, `integer_enum_with_default`,
//     `integer_enum_with_variant_default`, `integer_enum_all_variant_types`,
//     `nullable_enum`, `enum_multiple_columns`, `enum_shared`). Each renders
//     a `TableDef` with an enum column via `SeaOrmExporter::render_entity`,
//     which calls `seaorm::enums::render_enum` and emits the rendered enum
//     lines into the cross-ORM snapshot — so every code path inside
//     `render_enum` is exercised AND locked under
//     `src/tests/snapshots/<scenario>_SeaOrm.snap`.
//   * The substring-asserting unit test
//     `test_render_enum_uses_screaming_snake_serde_for_uppercase_values`
//     (kept below) for the SCREAMING_SNAKE serde-rename path.
//   * The parametric helper tests `test_enum_variant_name`,
//     `test_to_pascal_case`, `test_to_snake_case`,
//     `test_is_screaming_snake_value_rejects_invalid_symbol`, and
//     `test_screaming_snake_to_pascal_case_*` for the per-branch helpers
//     `render_enum` calls into.
// The previous SeaORM-only `test_render_enum_snapshots` rstest (4 cases →
// 4 `.snap` files under `src/seaorm/tests/snapshots/`) was redundant with
// the cross-ORM matrix and violated the "ALL exporter snapshots in
// `src/tests/snapshots/`" rule — it has been removed.

#[test]
fn test_resolve_fk_target_no_schema() {
    // Without schema context, should return original ref_table
    let (table, columns) = resolve_fk_target("article", &["media_id".into()], &[]);
    assert_eq!(table, "article");
    assert_eq!(columns, vec!["media_id"]);
}

#[test]
fn test_resolve_fk_target_no_chain() {
    use vespertide_core::{ColumnType, SimpleColumnType};
    // media table without FK chain
    let media = TableDef {
        name: "media".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    let schema = vec![media];
    let (table, columns) = resolve_fk_target("media", &["id".into()], &schema);
    assert_eq!(table, "media");
    assert_eq!(columns, vec!["id"]);
}

#[test]
fn test_resolve_fk_target_with_chain() {
    use vespertide_core::{ColumnType, SimpleColumnType};
    // media table
    let media = TableDef {
        name: "media".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    // article table with FK to media
    let article = TableDef {
        name: "article".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "media_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                nullable: false,
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
                columns: vec!["media_id".into(), "id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["media_id".into()],
                ref_table: "media".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![media, article];
    // Resolving article.media_id should follow FK chain to media.id
    let (table, columns) = resolve_fk_target("article", &["media_id".into()], &schema);
    assert_eq!(table, "media");
    assert_eq!(columns, vec!["id"]);
}

#[test]
fn test_resolve_fk_target_table_not_in_schema() {
    use vespertide_core::{ColumnType, SimpleColumnType};
    let media = TableDef {
        name: "media".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let schema = vec![media];
    // article is not in schema, should return original
    let (table, columns) = resolve_fk_target("article", &["media_id".into()], &schema);
    assert_eq!(table, "article");
    assert_eq!(columns, vec!["media_id"]);
}

#[test]
fn test_resolve_fk_target_composite_fk() {
    // Composite FK should return as-is (not follow chain)
    let (table, columns) = resolve_fk_target("article", &["media_id".into(), "id".into()], &[]);
    assert_eq!(table, "article");
    assert_eq!(columns, vec!["media_id", "id"]);
}

#[test]
fn test_render_entity_with_schema_fk_chain() {
    use vespertide_core::{ColumnType, SimpleColumnType};

    // media table
    let media = TableDef {
        name: "media".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    // article table with FK to media
    let article = TableDef {
        name: "article".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "media_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                nullable: false,
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
                columns: vec!["media_id".into(), "id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["media_id".into()],
                ref_table: "media".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    // article_user table with FK to article.media_id
    let article_user = TableDef {
        name: "article_user".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "article_media_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
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
                columns: vec!["article_media_id".into(), "user_id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["article_media_id".into()],
                ref_table: "article".into(),
                ref_columns: vec!["media_id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![media, article.clone(), article_user.clone()];

    // Render article_user with schema context
    let rendered = render_entity_with_schema(&article_user, &schema);

    // Should resolve to media, not article
    assert!(rendered.contains("super::media::Entity"));
    assert!(!rendered.contains("super::article::Entity"));
    // The from should still be article_media_id, but to should be id
    assert!(rendered.contains("from = \"article_media_id\""));
    assert!(rendered.contains("to = \"id\""));
}

#[test]
fn test_pluralize() {
    assert_eq!(pluralize("user"), "users");
    assert_eq!(pluralize("post"), "posts");
    assert_eq!(pluralize("category"), "categories");
    assert_eq!(pluralize("entity"), "entities");
    assert_eq!(pluralize("users"), "users"); // already plural
    assert_eq!(pluralize("day"), "days"); // 'ay' ending
    assert_eq!(pluralize("key"), "keys"); // 'ey' ending
    assert_eq!(pluralize("café_category"), "café_categories");
}

#[test]
fn test_resolve_fk_target_deep_chain() {
    use vespertide_core::{ColumnType, SimpleColumnType};

    // 3-level chain: level_c.b_id -> level_b.a_id -> level_a.id
    // level_a (root)
    let level_a = TableDef {
        name: "level_a".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    // level_b with FK to level_a
    let level_b = TableDef {
        name: "level_b".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "a_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["a_id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["a_id".into()],
                ref_table: "level_a".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    // level_c with FK to level_b
    let level_c = TableDef {
        name: "level_c".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "b_id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["b_id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["b_id".into()],
                ref_table: "level_b".into(),
                ref_columns: vec!["a_id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![level_a, level_b, level_c];
    // Resolving level_b.a_id should follow chain to level_a.id
    let (table, columns) = resolve_fk_target("level_b", &["a_id".into()], &schema);
    assert_eq!(table, "level_a");
    assert_eq!(columns, vec!["id"]);
}

#[test]
fn test_render_entity_with_schema_cyclic_fk_chain_returns_current_target() {
    use vespertide_core::{ColumnType, SimpleColumnType};

    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
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
                name: "fav_post_id".into(),
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
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["fav_post_id".into()],
                ref_table: "posts".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
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
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["fav_post_id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![users.clone(), posts];
    let rendered = render_entity_with_schema(&users, &schema);

    assert!(rendered.contains("super::users::Entity"));
    assert!(rendered.contains("from = \"fav_post_id\""));
    assert!(rendered.contains("to = \"fav_post_id\""));
}
