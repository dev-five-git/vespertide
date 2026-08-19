pub mod check_violation_strategy;
pub mod column;
pub mod constraint;
pub mod fk_orphan_strategy;
pub mod foreign_key;
pub mod index;
pub mod names;
pub mod pk_addition_strategy;
pub mod primary_key;
pub mod reference;
pub mod str_or_bool;
pub mod table;
pub mod unique_strategy;

pub use check_violation_strategy::CheckViolationStrategy;
pub use column::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, NumValue, SimpleColumnKind,
    SimpleColumnType,
};
pub use constraint::{ConstraintKind, TableConstraint};
pub use fk_orphan_strategy::ForeignKeyOrphanStrategy;
pub use index::IndexDef;
pub use names::{ColumnName, IndexName, TableName};
pub use pk_addition_strategy::PrimaryKeyAdditionStrategy;
pub use primary_key::PrimaryKeyDef;
pub use reference::{ReferenceAction, ReferenceActionKind};
pub use str_or_bool::{DefaultValue, StrOrBoolOrArray, StringOrBool};
pub use table::{TableDef, TableValidationError};
pub use unique_strategy::{KeepPolicy, UniqueConstraintStrategy};

#[cfg(test)]
mod tests {
    mod column {
        use crate::schema::column::*;
        use crate::schema::primary_key::PrimaryKeySyntax;
        use crate::schema::str_or_bool::StrOrBoolOrArray;
        use rstest::rstest;

        #[test]
        fn new_creates_minimal_column() {
            let c = ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false);
            assert_eq!(c.name.as_str(), "id");
            assert!(!c.nullable);
            assert!(c.primary_key.is_none());
            assert!(c.unique.is_none());
            assert!(c.index.is_none());
            assert!(c.foreign_key.is_none());
            assert!(c.default.is_none());
            assert!(c.comment.is_none());
        }

        #[test]
        fn builder_chain_sets_fields() {
            let c = ColumnDef::new("email", ColumnType::Simple(SimpleColumnType::Text), false)
                .unique(StrOrBoolOrArray::Bool(true))
                .index(StrOrBoolOrArray::Bool(true))
                .comment("user email");
            assert!(c.unique.is_some());
            assert!(c.index.is_some());
            assert_eq!(c.comment.as_deref(), Some("user email"));
        }

        #[test]
        fn builder_preserves_round_trip_with_existing_literal() {
            // Equivalence: new(...) + setters == literal { ... } construction
            let from_builder =
                ColumnDef::new("name", ColumnType::Simple(SimpleColumnType::Text), false);
            let from_literal = ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            };
            assert_eq!(from_builder, from_literal);
        }

        #[test]
        fn primary_key_builder_accepts_existing_syntax_type() {
            let c = ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true));
            assert_eq!(c.primary_key, Some(PrimaryKeySyntax::Bool(true)));
        }

        #[rstest]
        #[case(SimpleColumnType::SmallInt, "i16")]
        #[case(SimpleColumnType::Integer, "i32")]
        #[case(SimpleColumnType::BigInt, "i64")]
        #[case(SimpleColumnType::Real, "f32")]
        #[case(SimpleColumnType::DoublePrecision, "f64")]
        #[case(SimpleColumnType::Text, "String")]
        #[case(SimpleColumnType::Boolean, "bool")]
        #[case(SimpleColumnType::Date, "Date")]
        #[case(SimpleColumnType::Time, "Time")]
        #[case(SimpleColumnType::Timestamp, "DateTime")]
        #[case(SimpleColumnType::Timestamptz, "DateTimeWithTimeZone")]
        #[case(SimpleColumnType::Interval, "String")]
        #[case(SimpleColumnType::Bytea, "Vec<u8>")]
        #[case(SimpleColumnType::Uuid, "Uuid")]
        #[case(SimpleColumnType::Json, "Json")]
        // #[case(SimpleColumnType::Jsonb, "Json")]
        #[case(SimpleColumnType::Inet, "String")]
        #[case(SimpleColumnType::Cidr, "String")]
        #[case(SimpleColumnType::Macaddr, "String")]
        #[case(SimpleColumnType::Xml, "String")]
        fn test_simple_column_type_to_rust_type_not_nullable(
            #[case] column_type: SimpleColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(
                ColumnType::Simple(column_type).to_rust_type(false),
                expected
            );
        }

        #[rstest]
        #[case(SimpleColumnType::SmallInt, "Option<i16>")]
        #[case(SimpleColumnType::Integer, "Option<i32>")]
        #[case(SimpleColumnType::BigInt, "Option<i64>")]
        #[case(SimpleColumnType::Real, "Option<f32>")]
        #[case(SimpleColumnType::DoublePrecision, "Option<f64>")]
        #[case(SimpleColumnType::Text, "Option<String>")]
        #[case(SimpleColumnType::Boolean, "Option<bool>")]
        #[case(SimpleColumnType::Date, "Option<Date>")]
        #[case(SimpleColumnType::Time, "Option<Time>")]
        #[case(SimpleColumnType::Timestamp, "Option<DateTime>")]
        #[case(SimpleColumnType::Timestamptz, "Option<DateTimeWithTimeZone>")]
        #[case(SimpleColumnType::Interval, "Option<String>")]
        #[case(SimpleColumnType::Bytea, "Option<Vec<u8>>")]
        #[case(SimpleColumnType::Uuid, "Option<Uuid>")]
        #[case(SimpleColumnType::Json, "Option<Json>")]
        // #[case(SimpleColumnType::Jsonb, "Option<Json>")]
        #[case(SimpleColumnType::Inet, "Option<String>")]
        #[case(SimpleColumnType::Cidr, "Option<String>")]
        #[case(SimpleColumnType::Macaddr, "Option<String>")]
        #[case(SimpleColumnType::Xml, "Option<String>")]
        fn test_simple_column_type_to_rust_type_nullable(
            #[case] column_type: SimpleColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(ColumnType::Simple(column_type).to_rust_type(true), expected);
        }

        #[rstest]
        #[case(ComplexColumnType::Varchar { length: 255 }, false, "String")]
        #[case(ComplexColumnType::Varchar { length: 50 }, false, "String")]
        #[case(ComplexColumnType::Numeric { precision: 10, scale: 2 }, false, "Decimal")]
        #[case(ComplexColumnType::Numeric { precision: 5, scale: 0 }, false, "Decimal")]
        #[case(ComplexColumnType::Char { length: 10 }, false, "String")]
        #[case(ComplexColumnType::Char { length: 1 }, false, "String")]
        #[case(ComplexColumnType::Custom { custom_type: "MONEY".into() }, false, "String")]
        #[case(ComplexColumnType::Custom { custom_type: "JSONB".into() }, false, "String")]
        #[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec!["active".into(), "inactive".into()]) }, false, "String")]
        fn test_complex_column_type_to_rust_type_not_nullable(
            #[case] column_type: ComplexColumnType,
            #[case] nullable: bool,
            #[case] expected: &str,
        ) {
            assert_eq!(
                ColumnType::Complex(column_type).to_rust_type(nullable),
                expected
            );
        }

        #[rstest]
        #[case(ComplexColumnType::Varchar { length: 255 }, "Option<String>")]
        #[case(ComplexColumnType::Varchar { length: 50 }, "Option<String>")]
        #[case(ComplexColumnType::Numeric { precision: 10, scale: 2 }, "Option<Decimal>")]
        #[case(ComplexColumnType::Numeric { precision: 5, scale: 0 }, "Option<Decimal>")]
        #[case(ComplexColumnType::Char { length: 10 }, "Option<String>")]
        #[case(ComplexColumnType::Char { length: 1 }, "Option<String>")]
        #[case(ComplexColumnType::Custom { custom_type: "MONEY".into() }, "Option<String>")]
        #[case(ComplexColumnType::Custom { custom_type: "JSONB".into() }, "Option<String>")]
        #[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec!["active".into(), "inactive".into()]) }, "Option<String>")]
        fn test_complex_column_type_to_rust_type_nullable(
            #[case] column_type: ComplexColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(
                ColumnType::Complex(column_type).to_rust_type(true),
                expected
            );
        }

        #[rstest]
        #[case(ComplexColumnType::Varchar { length: 255 })]
        #[case(ComplexColumnType::Numeric { precision: 10, scale: 2 })]
        #[case(ComplexColumnType::Char { length: 1 })]
        #[case(ComplexColumnType::Custom { custom_type: "SERIAL".into() })]
        #[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec![]) })]
        fn test_complex_column_type_does_not_support_auto_increment(
            #[case] column_type: ComplexColumnType,
        ) {
            // Complex types never support auto_increment
            assert!(!ColumnType::Complex(column_type).supports_auto_increment());
        }

        #[test]
        fn test_enum_values_is_string() {
            let string_vals = EnumValues::String(vec!["active".into()]);
            let int_vals = EnumValues::Integer(vec![NumValue {
                name: "Active".into(),
                value: 1,
            }]);
            assert!(string_vals.is_string());
            assert!(!int_vals.is_string());
        }

        #[test]
        fn test_enum_values_is_integer() {
            let string_vals = EnumValues::String(vec!["active".into()]);
            let int_vals = EnumValues::Integer(vec![NumValue {
                name: "Active".into(),
                value: 1,
            }]);
            assert!(!string_vals.is_integer());
            assert!(int_vals.is_integer());
        }

        #[test]
        fn test_enum_values_variant_names_string() {
            let vals = EnumValues::String(vec!["pending".into(), "active".into()]);
            assert_eq!(vals.variant_names(), vec!["pending", "active"]);
        }

        #[test]
        fn test_enum_values_variant_names_integer() {
            let vals = EnumValues::Integer(vec![
                NumValue {
                    name: "Low".into(),
                    value: 0,
                },
                NumValue {
                    name: "High".into(),
                    value: 10,
                },
            ]);
            assert_eq!(vals.variant_names(), vec!["Low", "High"]);
        }

        #[test]
        fn test_enum_values_len_and_is_empty() {
            // String variant
            let empty = EnumValues::String(vec![]);
            let non_empty = EnumValues::String(vec!["a".into()]);
            assert!(empty.is_empty());
            assert_eq!(empty.len(), 0);
            assert!(!non_empty.is_empty());
            assert_eq!(non_empty.len(), 1);

            // Integer variant
            let empty_int = EnumValues::Integer(vec![]);
            let non_empty_int = EnumValues::Integer(vec![
                NumValue {
                    name: "A".into(),
                    value: 0,
                },
                NumValue {
                    name: "B".into(),
                    value: 1,
                },
            ]);
            assert!(empty_int.is_empty());
            assert_eq!(empty_int.len(), 0);
            assert!(!non_empty_int.is_empty());
            assert_eq!(non_empty_int.len(), 2);
        }

        #[test]
        fn test_enum_values_to_sql_values_string() {
            let vals = EnumValues::String(vec!["active".into(), "pending".into()]);
            assert_eq!(vals.to_sql_values(), vec!["'active'", "'pending'"]);
        }

        #[test]
        fn to_sql_values_escapes_single_quotes() {
            let vals =
                EnumValues::String(vec!["O'Brien".into(), "Smith".into(), "'leading".into()]);
            let sql = vals.to_sql_values();

            assert!(
                sql.iter().any(|s| s == "'O''Brien'"),
                "single quote inside value must be doubled"
            );
            assert!(
                sql.iter().any(|s| s == "'''leading'"),
                "leading single quote must be doubled"
            );
            assert!(
                sql.iter().any(|s| s == "'Smith'"),
                "values without quotes are unchanged"
            );
        }

        #[test]
        fn test_enum_values_to_sql_values_integer() {
            let vals = EnumValues::Integer(vec![
                NumValue {
                    name: "Low".into(),
                    value: 0,
                },
                NumValue {
                    name: "High".into(),
                    value: 10,
                },
            ]);
            assert_eq!(vals.to_sql_values(), vec!["0", "10"]);
        }

        #[test]
        fn test_enum_values_from_vec_string() {
            let vals: EnumValues = vec!["a".to_string(), "b".to_string()].into();
            assert!(matches!(vals, EnumValues::String(_)));
        }

        #[test]
        fn test_enum_values_from_vec_str() {
            let vals: EnumValues = vec!["a", "b"].into();
            assert!(matches!(vals, EnumValues::String(_)));
        }

        #[rstest]
        #[case(SimpleColumnType::SmallInt, true)]
        #[case(SimpleColumnType::Integer, true)]
        #[case(SimpleColumnType::BigInt, true)]
        #[case(SimpleColumnType::Text, false)]
        #[case(SimpleColumnType::Boolean, false)]
        fn test_simple_column_type_supports_auto_increment(
            #[case] ty: SimpleColumnType,
            #[case] expected: bool,
        ) {
            assert_eq!(ty.supports_auto_increment(), expected);
        }

        #[rstest]
        #[case(SimpleColumnType::Integer, true)]
        #[case(SimpleColumnType::Text, false)]
        fn test_column_type_simple_supports_auto_increment(
            #[case] ty: SimpleColumnType,
            #[case] expected: bool,
        ) {
            assert_eq!(ColumnType::Simple(ty).supports_auto_increment(), expected);
        }

        #[test]
        fn test_requires_migration_integer_enum_values_changed() {
            // Integer enum values changed - should NOT require migration
            let from = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::Integer(vec![
                    NumValue {
                        name: "Pending".into(),
                        value: 0,
                    },
                    NumValue {
                        name: "Active".into(),
                        value: 1,
                    },
                ]),
            });
            let to = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::Integer(vec![
                    NumValue {
                        name: "Pending".into(),
                        value: 0,
                    },
                    NumValue {
                        name: "Active".into(),
                        value: 1,
                    },
                    NumValue {
                        name: "Completed".into(),
                        value: 100,
                    },
                ]),
            });
            assert!(!from.requires_migration(&to));
        }

        #[test]
        fn requires_migration_integer_enum_name_change_is_false() {
            let e1 = ColumnType::Complex(ComplexColumnType::Enum {
                name: "old_status".into(),
                values: EnumValues::Integer(vec![
                    NumValue {
                        name: "active".into(),
                        value: 0,
                    },
                    NumValue {
                        name: "inactive".into(),
                        value: 1,
                    },
                ]),
            });
            let e2 = ColumnType::Complex(ComplexColumnType::Enum {
                name: "new_status".into(),
                values: EnumValues::Integer(vec![
                    NumValue {
                        name: "active".into(),
                        value: 0,
                    },
                    NumValue {
                        name: "inactive".into(),
                        value: 1,
                    },
                ]),
            });

            assert!(
                !e1.requires_migration(&e2),
                "integer enum name change should NOT require migration (stored as INTEGER, no DB schema change)"
            );
        }

        #[test]
        fn test_requires_migration_integer_enum_name_changed() {
            // Integer enum name changed - should NOT require migration (DB type is always INTEGER)
            let from = ColumnType::Complex(ComplexColumnType::Enum {
                name: "old_status".into(),
                values: EnumValues::Integer(vec![NumValue {
                    name: "Pending".into(),
                    value: 0,
                }]),
            });
            let to = ColumnType::Complex(ComplexColumnType::Enum {
                name: "new_status".into(),
                values: EnumValues::Integer(vec![NumValue {
                    name: "Pending".into(),
                    value: 0,
                }]),
            });
            assert!(!from.requires_migration(&to));
        }

        #[test]
        fn test_requires_migration_string_enum_values_changed() {
            // String enum values changed - SHOULD require migration
            let from = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["pending".into(), "active".into()]),
            });
            let to = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec![
                    "pending".into(),
                    "active".into(),
                    "completed".into(),
                ]),
            });
            assert!(from.requires_migration(&to));
        }

        #[test]
        fn test_requires_migration_simple_types() {
            let int = ColumnType::Simple(SimpleColumnType::Integer);
            let text = ColumnType::Simple(SimpleColumnType::Text);
            assert!(int.requires_migration(&text));
            assert!(!int.requires_migration(&int));
        }

        #[test]
        fn test_requires_migration_mixed_enum_types() {
            // String enum to integer enum - SHOULD require migration
            let string_enum = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["pending".into()]),
            });
            let int_enum = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::Integer(vec![NumValue {
                    name: "Pending".into(),
                    value: 0,
                }]),
            });
            assert!(string_enum.requires_migration(&int_enum));
        }

        // Tests for to_display_string
        #[rstest]
        #[case(SimpleColumnType::SmallInt, "smallint")]
        #[case(SimpleColumnType::Integer, "integer")]
        #[case(SimpleColumnType::BigInt, "bigint")]
        #[case(SimpleColumnType::Real, "real")]
        #[case(SimpleColumnType::DoublePrecision, "double precision")]
        #[case(SimpleColumnType::Text, "text")]
        #[case(SimpleColumnType::Boolean, "boolean")]
        #[case(SimpleColumnType::Date, "date")]
        #[case(SimpleColumnType::Time, "time")]
        #[case(SimpleColumnType::Timestamp, "timestamp")]
        #[case(SimpleColumnType::Timestamptz, "timestamptz")]
        #[case(SimpleColumnType::Interval, "interval")]
        #[case(SimpleColumnType::Bytea, "bytea")]
        #[case(SimpleColumnType::Uuid, "uuid")]
        #[case(SimpleColumnType::Json, "json")]
        #[case(SimpleColumnType::Inet, "inet")]
        #[case(SimpleColumnType::Cidr, "cidr")]
        #[case(SimpleColumnType::Macaddr, "macaddr")]
        #[case(SimpleColumnType::Xml, "xml")]
        fn test_simple_column_type_to_display_string(
            #[case] column_type: SimpleColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(column_type.to_display_string(), expected);
        }

        #[rstest]
        #[case(SimpleColumnType::SmallInt, "SMALLINT")]
        #[case(SimpleColumnType::Integer, "INTEGER")]
        #[case(SimpleColumnType::BigInt, "BIGINT")]
        #[case(SimpleColumnType::Real, "REAL")]
        #[case(SimpleColumnType::DoublePrecision, "DOUBLE PRECISION")]
        #[case(SimpleColumnType::Text, "TEXT")]
        #[case(SimpleColumnType::Boolean, "BOOLEAN")]
        #[case(SimpleColumnType::Date, "DATE")]
        #[case(SimpleColumnType::Time, "TIME")]
        #[case(SimpleColumnType::Timestamp, "TIMESTAMP")]
        #[case(SimpleColumnType::Timestamptz, "TIMESTAMPTZ")]
        #[case(SimpleColumnType::Interval, "INTERVAL")]
        #[case(SimpleColumnType::Bytea, "BYTEA")]
        #[case(SimpleColumnType::Uuid, "UUID")]
        #[case(SimpleColumnType::Json, "JSON")]
        #[case(SimpleColumnType::Inet, "INET")]
        #[case(SimpleColumnType::Cidr, "CIDR")]
        #[case(SimpleColumnType::Macaddr, "MACADDR")]
        #[case(SimpleColumnType::Xml, "XML")]
        fn test_simple_column_type_sql_type(
            #[case] column_type: SimpleColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(column_type.sql_type(), expected);
        }

        #[rstest]
        #[case(ComplexColumnType::Varchar { length: 255 }, "VARCHAR")]
        #[case(ComplexColumnType::Numeric { precision: 10, scale: 2 }, "NUMERIC")]
        #[case(ComplexColumnType::Char { length: 5 }, "CHAR")]
        #[case(ComplexColumnType::Custom { custom_type: "MONEY".into() }, "CUSTOM")]
        #[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec!["active".into()]) }, "ENUM")]
        fn test_complex_column_type_sql_type(
            #[case] column_type: ComplexColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(column_type.sql_type(), expected);
        }

        #[test]
        fn test_complex_column_type_to_display_string_varchar() {
            let ty = ComplexColumnType::Varchar { length: 255 };
            assert_eq!(ty.to_display_string(), "varchar(255)");
        }

        #[test]
        fn test_complex_column_type_to_display_string_numeric() {
            let ty = ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            };
            assert_eq!(ty.to_display_string(), "numeric(10,2)");
        }

        #[test]
        fn test_complex_column_type_to_display_string_char() {
            let ty = ComplexColumnType::Char { length: 5 };
            assert_eq!(ty.to_display_string(), "char(5)");
        }

        #[test]
        fn test_complex_column_type_to_display_string_custom() {
            let ty = ComplexColumnType::Custom {
                custom_type: "TSVECTOR".into(),
            };
            assert_eq!(ty.to_display_string(), "tsvector");
        }

        #[test]
        fn test_complex_column_type_to_display_string_string_enum() {
            let ty = ComplexColumnType::Enum {
                name: "user_status".into(),
                values: EnumValues::String(vec!["active".into(), "inactive".into()]),
            };
            assert_eq!(ty.to_display_string(), "enum<user_status>");
        }

        #[test]
        fn test_complex_column_type_to_display_string_integer_enum() {
            let ty = ComplexColumnType::Enum {
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
            };
            assert_eq!(ty.to_display_string(), "enum<priority> (integer)");
        }

        #[test]
        fn test_column_type_to_display_string_simple() {
            let ty = ColumnType::Simple(SimpleColumnType::Integer);
            assert_eq!(ty.to_display_string(), "integer");
        }

        #[test]
        fn test_column_type_to_display_string_complex() {
            let ty = ColumnType::Complex(ComplexColumnType::Varchar { length: 100 });
            assert_eq!(ty.to_display_string(), "varchar(100)");
        }

        // Tests for default_fill_value
        #[rstest]
        #[case(SimpleColumnType::SmallInt, "0")]
        #[case(SimpleColumnType::Integer, "0")]
        #[case(SimpleColumnType::BigInt, "0")]
        #[case(SimpleColumnType::Real, "0.0")]
        #[case(SimpleColumnType::DoublePrecision, "0.0")]
        #[case(SimpleColumnType::Boolean, "false")]
        #[case(SimpleColumnType::Text, "''")]
        #[case(SimpleColumnType::Date, "'1970-01-01'")]
        #[case(SimpleColumnType::Time, "'00:00:00'")]
        #[case(SimpleColumnType::Timestamp, "CURRENT_TIMESTAMP")]
        #[case(SimpleColumnType::Timestamptz, "CURRENT_TIMESTAMP")]
        #[case(SimpleColumnType::Interval, "'0'")]
        #[case(SimpleColumnType::Bytea, "''")]
        #[case(SimpleColumnType::Uuid, "'00000000-0000-0000-0000-000000000000'")]
        #[case(SimpleColumnType::Json, "'{}'")]
        #[case(SimpleColumnType::Inet, "'0.0.0.0'")]
        #[case(SimpleColumnType::Cidr, "'0.0.0.0'")]
        #[case(SimpleColumnType::Macaddr, "'00:00:00:00:00:00'")]
        #[case(SimpleColumnType::Xml, "'<xml/>'")]
        fn test_simple_column_type_default_fill_value(
            #[case] column_type: SimpleColumnType,
            #[case] expected: &str,
        ) {
            assert_eq!(column_type.default_fill_value(), expected);
        }

        #[test]
        fn test_complex_column_type_default_fill_value_varchar() {
            let ty = ComplexColumnType::Varchar { length: 255 };
            assert_eq!(ty.default_fill_value(), "''");
        }

        #[test]
        fn test_complex_column_type_default_fill_value_char() {
            let ty = ComplexColumnType::Char { length: 1 };
            assert_eq!(ty.default_fill_value(), "''");
        }

        #[test]
        fn test_complex_column_type_default_fill_value_numeric() {
            let ty = ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            };
            assert_eq!(ty.default_fill_value(), "0");
        }

        #[test]
        fn test_complex_column_type_default_fill_value_custom() {
            let ty = ComplexColumnType::Custom {
                custom_type: "MONEY".into(),
            };
            assert_eq!(ty.default_fill_value(), "''");
        }

        #[test]
        fn test_complex_column_type_default_fill_value_enum() {
            let ty = ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["active".into()]),
            };
            assert_eq!(ty.default_fill_value(), "''");
        }

        #[test]
        fn test_column_type_default_fill_value_simple() {
            let ty = ColumnType::Simple(SimpleColumnType::Integer);
            assert_eq!(ty.default_fill_value(), "0");
        }

        #[test]
        fn test_column_type_default_fill_value_complex() {
            let ty = ColumnType::Complex(ComplexColumnType::Varchar { length: 100 });
            assert_eq!(ty.default_fill_value(), "''");
        }

        // Tests for enum_variant_names
        #[test]
        fn test_enum_variant_names_simple_type_returns_none() {
            let ty = ColumnType::Simple(SimpleColumnType::Integer);
            assert_eq!(ty.enum_variant_names(), None);
        }

        #[test]
        fn test_enum_variant_names_complex_non_enum_returns_none() {
            let ty = ColumnType::Complex(ComplexColumnType::Varchar { length: 255 });
            assert_eq!(ty.enum_variant_names(), None);
        }

        #[test]
        fn test_enum_variant_names_complex_numeric_returns_none() {
            let ty = ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            });
            assert_eq!(ty.enum_variant_names(), None);
        }

        #[test]
        fn test_enum_variant_names_complex_char_returns_none() {
            let ty = ColumnType::Complex(ComplexColumnType::Char { length: 1 });
            assert_eq!(ty.enum_variant_names(), None);
        }

        #[test]
        fn test_enum_variant_names_complex_custom_returns_none() {
            let ty = ColumnType::Complex(ComplexColumnType::Custom {
                custom_type: "TSVECTOR".into(),
            });
            assert_eq!(ty.enum_variant_names(), None);
        }

        #[test]
        fn test_enum_variant_names_string_enum() {
            let ty = ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec![
                    "active".into(),
                    "inactive".into(),
                    "pending".into(),
                ]),
            });
            assert_eq!(
                ty.enum_variant_names(),
                Some(vec![
                    "active".to_string(),
                    "inactive".to_string(),
                    "pending".to_string()
                ])
            );
        }

        #[test]
        fn test_enum_variant_names_integer_enum() {
            let ty = ColumnType::Complex(ComplexColumnType::Enum {
                name: "priority".into(),
                values: EnumValues::Integer(vec![
                    NumValue {
                        name: "Low".into(),
                        value: 0,
                    },
                    NumValue {
                        name: "Medium".into(),
                        value: 5,
                    },
                    NumValue {
                        name: "High".into(),
                        value: 10,
                    },
                ]),
            });
            assert_eq!(
                ty.enum_variant_names(),
                Some(vec![
                    "Low".to_string(),
                    "Medium".to_string(),
                    "High".to_string()
                ])
            );
        }

        #[test]
        fn test_enum_variant_names_empty_string_enum() {
            let ty = ColumnType::Complex(ComplexColumnType::Enum {
                name: "empty".into(),
                values: EnumValues::String(vec![]),
            });
            assert_eq!(ty.enum_variant_names(), Some(vec![]));
        }

        #[test]
        fn test_enum_variant_names_empty_integer_enum() {
            let ty = ColumnType::Complex(ComplexColumnType::Enum {
                name: "empty".into(),
                values: EnumValues::Integer(vec![]),
            });
            assert_eq!(ty.enum_variant_names(), Some(vec![]));
        }

        #[rstest]
        #[case(SimpleColumnType::SmallInt, SimpleColumnKind::SmallInt)]
        #[case(SimpleColumnType::Integer, SimpleColumnKind::Integer)]
        #[case(SimpleColumnType::BigInt, SimpleColumnKind::BigInt)]
        #[case(SimpleColumnType::Real, SimpleColumnKind::Real)]
        #[case(SimpleColumnType::DoublePrecision, SimpleColumnKind::DoublePrecision)]
        #[case(SimpleColumnType::Text, SimpleColumnKind::Text)]
        #[case(SimpleColumnType::Boolean, SimpleColumnKind::Boolean)]
        #[case(SimpleColumnType::Date, SimpleColumnKind::Date)]
        #[case(SimpleColumnType::Time, SimpleColumnKind::Time)]
        #[case(SimpleColumnType::Timestamp, SimpleColumnKind::Timestamp)]
        #[case(SimpleColumnType::Timestamptz, SimpleColumnKind::Timestamptz)]
        #[case(SimpleColumnType::Interval, SimpleColumnKind::Interval)]
        #[case(SimpleColumnType::Bytea, SimpleColumnKind::Bytea)]
        #[case(SimpleColumnType::Uuid, SimpleColumnKind::Uuid)]
        #[case(SimpleColumnType::Json, SimpleColumnKind::Json)]
        #[case(SimpleColumnType::Inet, SimpleColumnKind::Inet)]
        #[case(SimpleColumnType::Cidr, SimpleColumnKind::Cidr)]
        #[case(SimpleColumnType::Macaddr, SimpleColumnKind::Macaddr)]
        #[case(SimpleColumnType::Xml, SimpleColumnKind::Xml)]
        fn test_simple_column_kind_from_matches_variant(
            #[case] ty: SimpleColumnType,
            #[case] expected: SimpleColumnKind,
        ) {
            assert_eq!(SimpleColumnKind::from(ty), expected);
        }
    }
}
