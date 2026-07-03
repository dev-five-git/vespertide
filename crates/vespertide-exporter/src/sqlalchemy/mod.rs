mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub use render::{export, render_entity};

pub struct SqlAlchemyExporter;

impl OrmExporter for SqlAlchemyExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

/// Test-only accessor for the `SQLAlchemy` `to_pascal_case` helper.
///
/// Allows the cross-ORM consolidation test in `crate::tests` to exercise
/// the `SQLAlchemy` naming helper without making it `pub(crate)` for the
/// entire crate.
#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    render::to_pascal_case(s)
}

#[cfg(test)]
mod tests {
    use super::render::to_screaming_snake_case;
    use super::types::UsedTypes;
    use vespertide_core::schema::column::{
        ColumnType, ComplexColumnType, EnumValues, NumValue, SimpleColumnType,
    };

    #[rstest::rstest]
    #[case("pending", "PENDING")]
    #[case("inProgress", "IN_PROGRESS")]
    #[case("order-status", "ORDER_STATUS")]
    fn test_to_screaming_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_screaming_snake_case(input), expected);
    }

    #[test]
    fn test_used_types_all_branches_comprehensive() {
        let mut used = UsedTypes::default();
        for ty in [
            SimpleColumnType::SmallInt,
            SimpleColumnType::Integer,
            SimpleColumnType::BigInt,
            SimpleColumnType::Real,
            SimpleColumnType::DoublePrecision,
            SimpleColumnType::Text,
            SimpleColumnType::Boolean,
            SimpleColumnType::Date,
            SimpleColumnType::Time,
            SimpleColumnType::Timestamp,
            SimpleColumnType::Timestamptz,
            SimpleColumnType::Interval,
            SimpleColumnType::Bytea,
            SimpleColumnType::Uuid,
            SimpleColumnType::Json,
            SimpleColumnType::Inet,
            SimpleColumnType::Cidr,
            SimpleColumnType::Macaddr,
            SimpleColumnType::Xml,
        ] {
            used.add_column_type(&ColumnType::Simple(ty), false);
        }
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
            false,
        );
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
            false,
        );
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            }),
            false,
        );
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Custom {
                custom_type: "FOO".into(),
            }),
            false,
        );
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["a".into()]),
            }),
            false,
        );
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "priority".into(),
                values: EnumValues::Integer(vec![NumValue {
                    name: "Low".into(),
                    value: 0,
                }]),
            }),
            false,
        );

        assert!(used.sa_types.contains("SmallInteger"));
        assert!(used.sa_types.contains("Integer"));
        assert!(used.sa_types.contains("BigInteger"));
        assert!(used.sa_types.contains("Float"));
        assert!(used.sa_types.contains("Text"));
        assert!(used.sa_types.contains("Boolean"));
        assert!(used.sa_types.contains("Date"));
        assert!(used.sa_types.contains("Time"));
        assert!(used.sa_types.contains("DateTime"));
        assert!(used.sa_types.contains("Interval"));
        assert!(used.sa_types.contains("LargeBinary"));
        assert!(used.sa_types.contains("Uuid"));
        assert!(used.sa_types.contains("JSON"));
        assert!(used.sa_types.contains("String"));
        assert!(used.sa_types.contains("Numeric"));
        assert!(used.sa_types.contains("Enum"));
        assert!(used.needs_uuid);
        assert!(used.needs_decimal);
    }

    #[test]
    fn test_used_types_nullable_sets_optional() {
        let mut used = UsedTypes::default();
        assert!(!used.needs_optional);
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), true);
        assert!(used.needs_optional);
    }
}
