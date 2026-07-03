use std::collections::BTreeSet;

use super::enums::to_pascal_case;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

/// Track which types are actually used to generate minimal imports
#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "import flags are independent generated-code needs"
)]
pub(super) struct UsedTypes<'a> {
    pub(super) datetime_types: BTreeSet<&'a str>,
    pub(super) needs_optional: bool,
    pub(super) needs_uuid: bool,
    pub(super) needs_decimal: bool,
    pub(super) needs_index: bool,
    pub(super) needs_unique_constraint: bool,
    pub(super) needs_foreign_key_constraint: bool,
    pub(super) needs_text: bool,
}

impl UsedTypes<'_> {
    pub(super) fn merge(&mut self, other: Self) {
        self.datetime_types.extend(other.datetime_types);
        self.needs_optional |= other.needs_optional;
        self.needs_uuid |= other.needs_uuid;
        self.needs_decimal |= other.needs_decimal;
        self.needs_index |= other.needs_index;
        self.needs_unique_constraint |= other.needs_unique_constraint;
        self.needs_foreign_key_constraint |= other.needs_foreign_key_constraint;
        self.needs_text |= other.needs_text;
    }

    pub(super) fn add_column_type(&mut self, col_type: &ColumnType, nullable: bool) {
        if nullable {
            self.needs_optional = true;
        }

        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::Date => {
                    self.datetime_types.insert("date");
                }
                SimpleColumnType::Time => {
                    self.datetime_types.insert("time");
                }
                SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => {
                    self.datetime_types.insert("datetime");
                }
                SimpleColumnType::Uuid => {
                    self.needs_uuid = true;
                }
                _ => {}
            },
            ColumnType::Complex(ty) => {
                if let ComplexColumnType::Numeric { .. } = ty {
                    self.needs_decimal = true;
                }
            }
        }
    }
}

pub(super) fn column_type_to_python(col_type: &ColumnType, nullable: bool) -> String {
    let base = match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => {
                "int"
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "float",
            SimpleColumnType::Text
            | SimpleColumnType::Interval
            | SimpleColumnType::Inet
            | SimpleColumnType::Cidr
            | SimpleColumnType::Macaddr
            | SimpleColumnType::Xml => "str",
            SimpleColumnType::Boolean => "bool",
            SimpleColumnType::Date => "date",
            SimpleColumnType::Time => "time",
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "datetime",
            SimpleColumnType::Bytea => "bytes",
            SimpleColumnType::Uuid => "UUID",
            SimpleColumnType::Json => "dict",
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Numeric { .. } => "Decimal",
            ComplexColumnType::Varchar { .. }
            | ComplexColumnType::Char { .. }
            | ComplexColumnType::Custom { .. } => "str",
            ComplexColumnType::Enum { name, .. } => {
                return if nullable {
                    format!("Optional[{}]", to_pascal_case(name))
                } else {
                    to_pascal_case(name)
                };
            }
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    };

    if nullable {
        format!("Optional[{base}]")
    } else {
        base.to_string()
    }
}
