use std::collections::HashSet;

use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

use super::enums::enum_identifier;

/// Maps a vespertide column type to a Prisma scalar type.
///
/// The output is backend-neutral: no `@db.*` native attributes are emitted, so
/// the same model body is valid under every Prisma provider. Physical column
/// types are owned by vespertide's own DDL generation, not the Prisma schema.
///
/// `table` and `ambiguous` resolve enum columns to the same identifier the enum
/// block is emitted under; see [`enum_identifier`].
pub(super) fn column_type_to_prisma(
    ty: &ColumnType,
    nullable: bool,
    table: &str,
    ambiguous: &HashSet<String>,
) -> String {
    let q = if nullable { "?" } else { "" };

    match ty {
        ColumnType::Simple(simple) => {
            let base = match simple {
                SimpleColumnType::SmallInt | SimpleColumnType::Integer => "Int",
                SimpleColumnType::BigInt => "BigInt",
                SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "Float",
                SimpleColumnType::Boolean => "Boolean",
                SimpleColumnType::Date
                | SimpleColumnType::Time
                | SimpleColumnType::Timestamp
                | SimpleColumnType::Timestamptz => "DateTime",
                SimpleColumnType::Bytea => "Bytes",
                SimpleColumnType::Json => "Json",
                SimpleColumnType::Text
                | SimpleColumnType::Uuid
                | SimpleColumnType::Interval
                | SimpleColumnType::Inet
                | SimpleColumnType::Cidr
                | SimpleColumnType::Macaddr
                | SimpleColumnType::Xml => "String",
                _ => unreachable!(
                    "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
                ),
            };
            format!("{base}{q}")
        }
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                format!("String{q}")
            }
            ComplexColumnType::Numeric { .. } => format!("Decimal{q}"),
            ComplexColumnType::Custom { custom_type } => {
                format!("Unsupported(\"{custom_type}\"){q}")
            }
            ComplexColumnType::Enum { name, .. } => {
                let ident = enum_identifier(table, name, ambiguous);
                format!("{ident}{q}")
            }
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::{
        ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
    };

    use super::*;

    /// Exhaustive oracle for every simple type. Each row is independent test
    /// data, so flipping any production mapping arm fails here immediately.
    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "Int")]
    #[case::integer(SimpleColumnType::Integer, "Int")]
    #[case::big_int(SimpleColumnType::BigInt, "BigInt")]
    #[case::real(SimpleColumnType::Real, "Float")]
    #[case::double_precision(SimpleColumnType::DoublePrecision, "Float")]
    #[case::boolean(SimpleColumnType::Boolean, "Boolean")]
    #[case::date(SimpleColumnType::Date, "DateTime")]
    #[case::time(SimpleColumnType::Time, "DateTime")]
    #[case::timestamp(SimpleColumnType::Timestamp, "DateTime")]
    #[case::timestamptz(SimpleColumnType::Timestamptz, "DateTime")]
    #[case::bytea(SimpleColumnType::Bytea, "Bytes")]
    #[case::json(SimpleColumnType::Json, "Json")]
    #[case::text(SimpleColumnType::Text, "String")]
    #[case::uuid(SimpleColumnType::Uuid, "String")]
    #[case::interval(SimpleColumnType::Interval, "String")]
    #[case::inet(SimpleColumnType::Inet, "String")]
    #[case::cidr(SimpleColumnType::Cidr, "String")]
    #[case::macaddr(SimpleColumnType::Macaddr, "String")]
    #[case::xml(SimpleColumnType::Xml, "String")]
    fn simple_types_map_to_neutral_scalars(#[case] simple: SimpleColumnType, #[case] scalar: &str) {
        assert_eq!(
            column_type_to_prisma(
                &ColumnType::Simple(simple),
                false,
                "any_table",
                &HashSet::new()
            ),
            scalar
        );
    }

    #[test]
    fn nullable_appends_question_mark() {
        let ty = ColumnType::Simple(SimpleColumnType::Timestamptz);
        assert_eq!(
            column_type_to_prisma(&ty, true, "any_table", &HashSet::new()),
            "DateTime?"
        );
    }

    #[rstest]
    #[case::varchar(ComplexColumnType::Varchar { length: 255 }, false, "String")]
    #[case::char(ComplexColumnType::Char { length: 3 }, false, "String")]
    #[case::numeric_nullable(
        ComplexColumnType::Numeric { precision: 10, scale: 2 },
        true,
        "Decimal?"
    )]
    #[case::custom(
        ComplexColumnType::Custom { custom_type: "ltree".into() },
        false,
        "Unsupported(\"ltree\")"
    )]
    #[case::enum_type(
        ComplexColumnType::Enum {
            name: "order_status".into(),
            values: EnumValues::String(vec!["open".into(), "closed".into()]),
        },
        false,
        "OrderStatus"
    )]
    fn complex_types_map_to_neutral_scalars(
        #[case] complex: ComplexColumnType,
        #[case] nullable: bool,
        #[case] expected: &str,
    ) {
        assert_eq!(
            column_type_to_prisma(
                &ColumnType::Complex(complex),
                nullable,
                "any_table",
                &HashSet::new()
            ),
            expected
        );
    }

    #[test]
    fn ambiguous_enum_column_uses_the_table_qualified_identifier() {
        let ty = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["new".into()]),
        });
        let ambiguous = HashSet::from(["Status".to_string()]);
        assert_eq!(
            column_type_to_prisma(&ty, false, "orders", &ambiguous),
            "OrdersStatus"
        );
    }
}
