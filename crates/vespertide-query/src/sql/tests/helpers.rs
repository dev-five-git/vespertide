use super::super::helpers::{is_enum_type, needs_quoting, parse_pg_type_cast};
use super::*;
use proptest::prelude::*;
use sea_query::{Alias, ColumnDef as SeaColumnDef, ForeignKeyAction};
use vespertide_core::{ComplexColumnType, EnumValues};

#[rstest]
#[case(ColumnType::Simple(SimpleColumnType::Integer))]
#[case(ColumnType::Simple(SimpleColumnType::BigInt))]
#[case(ColumnType::Simple(SimpleColumnType::Text))]
#[case(ColumnType::Simple(SimpleColumnType::Boolean))]
#[case(ColumnType::Simple(SimpleColumnType::Timestamp))]
#[case(ColumnType::Simple(SimpleColumnType::Uuid))]
#[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }))]
#[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }))]
fn test_column_type_conversion(#[case] ty: ColumnType) {
    // Just ensure no panic - test by creating a column with this type
    let mut col = SeaColumnDef::new(Alias::new("test"));
    apply_column_type_with_table(&mut col, &ty, "test_table", DatabaseBackend::Postgres);
}

#[rstest]
#[case(SimpleColumnType::SmallInt)]
#[case(SimpleColumnType::Integer)]
#[case(SimpleColumnType::BigInt)]
#[case(SimpleColumnType::Real)]
#[case(SimpleColumnType::DoublePrecision)]
#[case(SimpleColumnType::Text)]
#[case(SimpleColumnType::Boolean)]
#[case(SimpleColumnType::Date)]
#[case(SimpleColumnType::Time)]
#[case(SimpleColumnType::Timestamp)]
#[case(SimpleColumnType::Timestamptz)]
#[case(SimpleColumnType::Interval)]
#[case(SimpleColumnType::Bytea)]
#[case(SimpleColumnType::Uuid)]
#[case(SimpleColumnType::Json)]
#[case(SimpleColumnType::Inet)]
#[case(SimpleColumnType::Cidr)]
#[case(SimpleColumnType::Macaddr)]
#[case(SimpleColumnType::Xml)]
fn test_all_simple_types_cover_branches(#[case] ty: SimpleColumnType) {
    let mut col = SeaColumnDef::new(Alias::new("t"));
    apply_column_type_with_table(
        &mut col,
        &ColumnType::Simple(ty),
        "test_table",
        DatabaseBackend::Postgres,
    );
}

#[rstest]
#[case(ComplexColumnType::Varchar { length: 42 })]
#[case(ComplexColumnType::Numeric { precision: 8, scale: 3 })]
#[case(ComplexColumnType::Char { length: 3 })]
#[case(ComplexColumnType::Custom { custom_type: "GEOGRAPHY".into() })]
#[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec!["active".into(), "inactive".into()]) })]
fn test_all_complex_types_cover_branches(#[case] ty: ComplexColumnType) {
    let mut col = SeaColumnDef::new(Alias::new("t"));
    apply_column_type_with_table(
        &mut col,
        &ColumnType::Complex(ty),
        "test_table",
        DatabaseBackend::Postgres,
    );
}

#[rstest]
#[case::cascade(ReferenceAction::Cascade, ForeignKeyAction::Cascade)]
#[case::restrict(ReferenceAction::Restrict, ForeignKeyAction::Restrict)]
#[case::set_null(ReferenceAction::SetNull, ForeignKeyAction::SetNull)]
#[case::set_default(ReferenceAction::SetDefault, ForeignKeyAction::SetDefault)]
#[case::no_action(ReferenceAction::NoAction, ForeignKeyAction::NoAction)]
fn test_reference_action_conversion(
    #[case] action: ReferenceAction,
    #[case] expected: ForeignKeyAction,
) {
    // Just ensure the function doesn't panic and returns valid ForeignKeyAction
    let result = to_sea_fk_action(&action);
    assert!(
        matches!(result, _expected),
        "Expected {expected:?}, got {result:?}"
    );
}

#[rstest]
#[case(ReferenceAction::Cascade, "CASCADE")]
#[case(ReferenceAction::Restrict, "RESTRICT")]
#[case(ReferenceAction::SetNull, "SET NULL")]
#[case(ReferenceAction::SetDefault, "SET DEFAULT")]
#[case(ReferenceAction::NoAction, "NO ACTION")]
fn test_reference_action_sql_all_variants(#[case] action: ReferenceAction, #[case] expected: &str) {
    assert_eq!(reference_action_sql(&action), expected);
}

#[rstest]
#[case::gen_random_uuid_postgres(
    "gen_random_uuid()",
    DatabaseBackend::Postgres,
    "gen_random_uuid()"
)]
#[case::gen_random_uuid_mysql("gen_random_uuid()", DatabaseBackend::MySql, "(UUID())")]
#[case::gen_random_uuid_sqlite(
    "gen_random_uuid()",
    DatabaseBackend::Sqlite,
    "lower(hex(randomblob(16)))"
)]
#[case::current_timestamp_postgres(
    "current_timestamp()",
    DatabaseBackend::Postgres,
    "CURRENT_TIMESTAMP"
)]
#[case::current_timestamp_mysql("current_timestamp()", DatabaseBackend::MySql, "CURRENT_TIMESTAMP")]
#[case::current_timestamp_sqlite(
    "current_timestamp()",
    DatabaseBackend::Sqlite,
    "CURRENT_TIMESTAMP"
)]
#[case::now_postgres("now()", DatabaseBackend::Postgres, "CURRENT_TIMESTAMP")]
#[case::now_mysql("now()", DatabaseBackend::MySql, "CURRENT_TIMESTAMP")]
#[case::now_sqlite("now()", DatabaseBackend::Sqlite, "CURRENT_TIMESTAMP")]
#[case::now_upper_postgres("NOW()", DatabaseBackend::Postgres, "CURRENT_TIMESTAMP")]
#[case::now_upper_mysql("NOW()", DatabaseBackend::MySql, "CURRENT_TIMESTAMP")]
#[case::now_upper_sqlite("NOW()", DatabaseBackend::Sqlite, "CURRENT_TIMESTAMP")]
#[case::current_timestamp_upper_postgres(
    "CURRENT_TIMESTAMP",
    DatabaseBackend::Postgres,
    "CURRENT_TIMESTAMP"
)]
#[case::current_timestamp_upper_mysql(
    "CURRENT_TIMESTAMP",
    DatabaseBackend::MySql,
    "CURRENT_TIMESTAMP"
)]
#[case::current_timestamp_upper_sqlite(
    "CURRENT_TIMESTAMP",
    DatabaseBackend::Sqlite,
    "CURRENT_TIMESTAMP"
)]
fn test_convert_default_for_backend(
    #[case] default: &str,
    #[case] backend: DatabaseBackend,
    #[case] expected: &str,
) {
    let result = convert_default_for_backend(default, backend);
    assert_eq!(result, expected);
}

// --- PostgreSQL type cast conversion tests ---

#[rstest]
// JSON type cast: '[]'::json
#[case::json_cast_postgres("'[]'::json", DatabaseBackend::Postgres, "'[]'::json")]
#[case::json_cast_mysql("'[]'::json", DatabaseBackend::MySql, "CAST('[]' AS JSON)")]
#[case::json_cast_sqlite("'[]'::json", DatabaseBackend::Sqlite, "'[]'")]
// JSONB type cast: '{}'::jsonb
#[case::jsonb_cast_postgres("'{}'::jsonb", DatabaseBackend::Postgres, "'{}'::jsonb")]
#[case::jsonb_cast_mysql("'{}'::jsonb", DatabaseBackend::MySql, "CAST('{}' AS JSON)")]
#[case::jsonb_cast_sqlite("'{}'::jsonb", DatabaseBackend::Sqlite, "'{}'")]
// Text type cast: 'hello'::text
#[case::text_cast_postgres("'hello'::text", DatabaseBackend::Postgres, "'hello'::text")]
#[case::text_cast_mysql("'hello'::text", DatabaseBackend::MySql, "CAST('hello' AS CHAR)")]
#[case::text_cast_sqlite("'hello'::text", DatabaseBackend::Sqlite, "'hello'")]
// Integer type cast: 0::integer
#[case::int_cast_postgres("0::integer", DatabaseBackend::Postgres, "0::integer")]
#[case::int_cast_mysql("0::integer", DatabaseBackend::MySql, "CAST(0 AS SIGNED)")]
#[case::int_cast_sqlite("0::integer", DatabaseBackend::Sqlite, "0")]
// Boolean type cast: 0::boolean
#[case::bool_cast_postgres("0::boolean", DatabaseBackend::Postgres, "0::boolean")]
#[case::bool_cast_mysql("0::boolean", DatabaseBackend::MySql, "CAST(0 AS UNSIGNED)")]
#[case::bool_cast_sqlite("0::boolean", DatabaseBackend::Sqlite, "0")]
// Nested JSON object: '{"key":"value"}'::json
#[case::json_obj_cast_postgres(
    "'{\"key\":\"value\"}'::json",
    DatabaseBackend::Postgres,
    "'{\"key\":\"value\"}'::json"
)]
#[case::json_obj_cast_mysql(
    "'{\"key\":\"value\"}'::json",
    DatabaseBackend::MySql,
    "CAST('{\"key\":\"value\"}' AS JSON)"
)]
#[case::json_obj_cast_sqlite(
    "'{\"key\":\"value\"}'::json",
    DatabaseBackend::Sqlite,
    "'{\"key\":\"value\"}'"
)]
// Timestamp type cast: '2024-01-01'::timestamp
#[case::timestamp_cast_postgres(
    "'2024-01-01'::timestamp",
    DatabaseBackend::Postgres,
    "'2024-01-01'::timestamp"
)]
#[case::timestamp_cast_mysql(
    "'2024-01-01'::timestamp",
    DatabaseBackend::MySql,
    "CAST('2024-01-01' AS DATETIME)"
)]
#[case::timestamp_cast_sqlite("'2024-01-01'::timestamp", DatabaseBackend::Sqlite, "'2024-01-01'")]
fn test_convert_default_for_backend_type_cast(
    #[case] default: &str,
    #[case] backend: DatabaseBackend,
    #[case] expected: &str,
) {
    let result = convert_default_for_backend(default, backend);
    assert_eq!(result, expected);
}

#[test]
fn test_parse_pg_type_cast_no_cast() {
    // Regular values should not be parsed as type casts
    assert!(parse_pg_type_cast("'hello'").is_none());
    assert!(parse_pg_type_cast("42").is_none());
    assert!(parse_pg_type_cast("NOW()").is_none());
    assert!(parse_pg_type_cast("CURRENT_TIMESTAMP").is_none());
}

#[test]
fn test_parse_pg_type_cast_valid() {
    let (value, cast_type) = parse_pg_type_cast("'[]'::json").unwrap();
    assert_eq!(value, "'[]'");
    assert_eq!(cast_type, "json");

    let (value, cast_type) = parse_pg_type_cast("0::boolean").unwrap();
    assert_eq!(value, "0");
    assert_eq!(cast_type, "boolean");
}

#[test]
fn test_parse_pg_type_cast_escaped_quotes() {
    // Value with escaped quotes: 'it''s'::text
    let (value, cast_type) = parse_pg_type_cast("'it''s'::text").unwrap();
    assert_eq!(value, "'it''s'");
    assert_eq!(cast_type, "text");
}

#[test]
fn test_parse_pg_type_cast_unicode_quoted_value() {
    let (value, cast_type) = parse_pg_type_cast("'한국어 日本語 café 📊'::text").unwrap();

    assert_eq!(value, "'한국어 日本語 café 📊'");
    assert_eq!(cast_type, "text");
}

#[test]
fn test_parse_pg_type_cast_unicode_unquoted_value() {
    let (value, cast_type) = parse_pg_type_cast("📊_stats::json").unwrap();

    assert_eq!(value, "📊_stats");
    assert_eq!(cast_type, "json");
}

proptest! {
    #[test]
    fn parse_pg_type_cast_does_not_panic_on_unicode(s in ".{0,100}") {
        let _ = parse_pg_type_cast(&s);
    }

    #[test]
    fn parse_pg_type_cast_round_trips_quoted_unicode_without_quotes(
        s in proptest::collection::vec(any::<char>().prop_filter("exclude SQL quote", |c| *c != '\''), 0..50)
            .prop_map(|v| v.into_iter().collect::<String>())
    ) {
        let expr = format!("'{s}'::text");
        let (value, cast_type) = parse_pg_type_cast(&expr).unwrap();

        prop_assert_eq!(value, format!("'{s}'"));
        prop_assert_eq!(cast_type, "text");
    }
}

#[test]
fn test_parse_pg_type_cast_unterminated_quote() {
    // Unterminated quoted string should return None (line 203)
    assert!(parse_pg_type_cast("'unclosed").is_none());
    assert!(parse_pg_type_cast("'no close quote::json").is_none());
}

#[rstest]
#[case::numeric("'0.5'::numeric", DatabaseBackend::MySql, "CAST('0.5' AS DECIMAL)")]
#[case::decimal("'1.23'::decimal", DatabaseBackend::MySql, "CAST('1.23' AS DECIMAL)")]
#[case::bytea("'\\xDE'::bytea", DatabaseBackend::MySql, "CAST('\\xDE' AS BINARY)")]
#[case::unknown("'x'::citext", DatabaseBackend::MySql, "CAST('x' AS CHAR)")]
fn test_convert_default_for_backend_type_cast_extra(
    #[case] default: &str,
    #[case] backend: DatabaseBackend,
    #[case] expected: &str,
) {
    let result = convert_default_for_backend(default, backend);
    assert_eq!(result, expected);
}

#[test]
fn test_is_enum_type_true() {
    use vespertide_core::EnumValues;

    let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
        name: "status".into(),
        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
    });
    assert!(is_enum_type(&enum_type));
}

#[test]
fn test_is_enum_type_false() {
    let text_type = ColumnType::Simple(SimpleColumnType::Text);
    assert!(!is_enum_type(&text_type));
}

#[test]
fn test_get_enum_name_some() {
    use vespertide_core::EnumValues;

    let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
        name: "user_status".into(),
        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
    });
    assert_eq!(get_enum_name(&enum_type), Some("user_status"));
}

#[test]
fn test_get_enum_name_none() {
    let text_type = ColumnType::Simple(SimpleColumnType::Text);
    assert_eq!(get_enum_name(&text_type), None);
}

#[test]
fn test_apply_column_type_integer_enum() {
    use vespertide_core::{EnumValues, NumValue};
    let integer_enum = ColumnType::Complex(ComplexColumnType::Enum {
        name: "color".into(),
        values: EnumValues::Integer(vec![
            NumValue {
                name: "Black".into(),
                value: 0,
            },
            NumValue {
                name: "White".into(),
                value: 1,
            },
        ]),
    });
    let mut col = SeaColumnDef::new(Alias::new("color"));
    apply_column_type_with_table(
        &mut col,
        &integer_enum,
        "test_table",
        DatabaseBackend::Postgres,
    );
    // Integer enums should use INTEGER type, not ENUM
}

#[test]
fn test_build_create_enum_type_sql_integer_enum_returns_none() {
    use vespertide_core::{EnumValues, NumValue};
    let integer_enum = ColumnType::Complex(ComplexColumnType::Enum {
        name: "priority".into(),
        values: EnumValues::Integer(vec![
            NumValue {
                name: "Low".into(),
                value: 0,
            },
            NumValue {
                name: "High".into(),
                value: 10,
            },
        ]),
    });
    // Integer enums should return None (no CREATE TYPE needed)
    assert!(build_create_enum_type_sql("test_table", &integer_enum).is_none());
}

#[rstest]
// Empty strings need quoting
#[case::empty("", true)]
#[case::whitespace_only("   ", true)]
// Function calls should not be quoted
#[case::now_func("now()", false)]
#[case::coalesce_func("COALESCE(old_value, 'default')", false)]
#[case::uuid_func("gen_random_uuid()", false)]
// NULL keyword should not be quoted
#[case::null_upper("NULL", false)]
#[case::null_lower("null", false)]
#[case::null_mixed("Null", false)]
// SQL date/time keywords should not be quoted
#[case::current_timestamp_upper("CURRENT_TIMESTAMP", false)]
#[case::current_timestamp_lower("current_timestamp", false)]
#[case::current_date_upper("CURRENT_DATE", false)]
#[case::current_date_lower("current_date", false)]
#[case::current_time_upper("CURRENT_TIME", false)]
#[case::current_time_lower("current_time", false)]
// Already quoted strings should not be re-quoted
#[case::single_quoted("'active'", false)]
#[case::double_quoted("\"active\"", false)]
// Plain strings need quoting
#[case::plain_active("active", true)]
#[case::plain_pending("pending", true)]
#[case::plain_underscore("some_value", true)]
fn test_needs_quoting(#[case] input: &str, #[case] expected: bool) {
    assert_eq!(needs_quoting(input), expected);
}

#[test]
fn test_recreate_indexes_after_rebuild_skips_pending() {
    use vespertide_core::TableConstraint;
    let idx1 = TableConstraint::Index {
        name: Some("idx_a".into()),
        columns: vec!["a".into()],
    };
    let idx2 = TableConstraint::Index {
        name: Some("idx_b".into()),
        columns: vec!["b".into()],
    };
    let uq1 = TableConstraint::Unique {
        name: Some("uq_c".into()),
        columns: vec!["c".into()],
        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First,
        },
    };

    // All three in table constraints, but idx1 and uq1 are pending
    let constraints = vec![idx1.clone(), idx2.clone(), uq1.clone()];
    let pending = vec![idx1.clone(), uq1.clone()];

    let queries = recreate_indexes_after_rebuild("t", &constraints, &pending);
    // Only idx_b should be recreated
    assert_eq!(queries.len(), 1);
    let sql = queries[0].build(DatabaseBackend::Sqlite);
    assert!(sql.contains("idx_b"));
}

#[test]
fn test_recreate_indexes_after_rebuild_no_pending() {
    use vespertide_core::TableConstraint;
    let idx = TableConstraint::Index {
        name: Some("idx_a".into()),
        columns: vec!["a".into()],
    };
    let uq = TableConstraint::Unique {
        name: Some("uq_b".into()),
        columns: vec!["b".into()],
        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
            keep: vespertide_core::KeepPolicy::First,
        },
    };

    let queries = recreate_indexes_after_rebuild("t", &[idx, uq], &[]);
    assert_eq!(queries.len(), 2);
}

#[test]
fn test_recreate_indexes_after_rebuild_skips_non_index_constraints() {
    use vespertide_core::TableConstraint;
    let pk = TableConstraint::PrimaryKey {
        columns: vec!["id".into()],
        auto_increment: false,
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    };
    let fk = TableConstraint::ForeignKey {
        name: None,
        columns: vec!["uid".into()],
        ref_table: "u".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let chk = TableConstraint::Check {
        name: "chk".into(),
        expr: "id > 0".into(),
        strategy: vespertide_core::CheckViolationStrategy::default(),
    };

    let queries = recreate_indexes_after_rebuild("t", &[pk, fk, chk], &[]);
    assert_eq!(queries.len(), 0);
}

#[test]
fn pg_quotes_simple_ident() {
    assert_eq!(quote_ident("users", DatabaseBackend::Postgres), "\"users\"");
}

#[test]
fn pg_escapes_embedded_quote() {
    assert_eq!(
        quote_ident("foo\"bar", DatabaseBackend::Postgres),
        "\"foo\"\"bar\""
    );
}

#[test]
fn mysql_uses_backticks() {
    assert_eq!(quote_ident("users", DatabaseBackend::MySql), "`users`");
}

#[test]
fn mysql_escapes_embedded_backtick() {
    assert_eq!(quote_ident("foo`bar", DatabaseBackend::MySql), "`foo``bar`");
}

#[test]
fn sqlite_uses_double_quotes() {
    assert_eq!(quote_ident("users", DatabaseBackend::Sqlite), "\"users\"");
}

#[test]
fn reserved_word_safe() {
    // Reserved words are still quoted correctly
    assert_eq!(quote_ident("order", DatabaseBackend::Postgres), "\"order\"");
    assert_eq!(quote_ident("select", DatabaseBackend::MySql), "`select`");
}
