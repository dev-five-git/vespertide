use std::collections::{BTreeSet, HashSet};

use super::render::to_pascal_case;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};

/// Track which types are actually used to generate minimal imports
#[derive(Default)]
pub(super) struct UsedTypes<'a> {
    pub(super) sa_types: HashSet<&'a str>,
    pub(super) datetime_types: BTreeSet<&'a str>,
    pub(super) needs_optional: bool,
    pub(super) needs_uuid: bool,
    pub(super) needs_decimal: bool,
}

impl UsedTypes<'_> {
    pub(super) fn merge(&mut self, other: Self) {
        self.sa_types.extend(other.sa_types);
        self.datetime_types.extend(other.datetime_types);
        self.needs_optional |= other.needs_optional;
        self.needs_uuid |= other.needs_uuid;
        self.needs_decimal |= other.needs_decimal;
    }

    pub(super) fn add_column_type(&mut self, col_type: &ColumnType, nullable: bool) {
        if nullable {
            self.needs_optional = true;
        }

        match col_type {
            ColumnType::Simple(ty) => self.add_simple_type(*ty),
            ColumnType::Complex(ty) => self.add_complex_type(ty),
        }
    }

    fn add_simple_type(&mut self, ty: SimpleColumnType) {
        match ty {
            SimpleColumnType::SmallInt => {
                self.sa_types.insert("SmallInteger");
            }
            SimpleColumnType::Integer => {
                self.sa_types.insert("Integer");
            }
            SimpleColumnType::BigInt => {
                self.sa_types.insert("BigInteger");
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => {
                self.sa_types.insert("Float");
            }
            SimpleColumnType::Text | SimpleColumnType::Xml => {
                self.sa_types.insert("Text");
            }
            SimpleColumnType::Boolean => {
                self.sa_types.insert("Boolean");
            }
            SimpleColumnType::Date => {
                self.sa_types.insert("Date");
                self.datetime_types.insert("date");
            }
            SimpleColumnType::Time => {
                self.sa_types.insert("Time");
                self.datetime_types.insert("time");
            }
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => {
                self.sa_types.insert("DateTime");
                self.datetime_types.insert("datetime");
            }
            SimpleColumnType::Interval => {
                self.sa_types.insert("Interval");
            }
            SimpleColumnType::Bytea => {
                self.sa_types.insert("LargeBinary");
            }
            SimpleColumnType::Uuid => {
                self.sa_types.insert("Uuid");
                self.needs_uuid = true;
            }
            SimpleColumnType::Json => {
                self.sa_types.insert("JSON");
            }
            SimpleColumnType::Inet | SimpleColumnType::Cidr | SimpleColumnType::Macaddr => {
                self.sa_types.insert("String");
            }
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        }
    }

    fn add_complex_type(&mut self, ty: &ComplexColumnType) {
        match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                self.sa_types.insert("String");
            }
            ComplexColumnType::Numeric { .. } => {
                self.sa_types.insert("Numeric");
                self.needs_decimal = true;
            }
            ComplexColumnType::Custom { .. } => {}
            ComplexColumnType::Enum { values, .. } => match values {
                EnumValues::String(_) => {
                    self.sa_types.insert("Enum");
                }
                EnumValues::Integer(_) => {
                    self.sa_types.insert("Integer");
                }
            },
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
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

pub(super) fn column_type_to_sqlalchemy(col_type: &ColumnType) -> String {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "SmallInteger".into(),
            SimpleColumnType::Integer => "Integer".into(),
            SimpleColumnType::BigInt => "BigInteger".into(),
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "Float".into(),
            SimpleColumnType::Text | SimpleColumnType::Xml => "Text".into(),
            SimpleColumnType::Boolean => "Boolean".into(),
            SimpleColumnType::Date => "Date".into(),
            SimpleColumnType::Time => "Time".into(),
            SimpleColumnType::Timestamp => "DateTime".into(),
            SimpleColumnType::Timestamptz => "DateTime(timezone=True)".into(),
            SimpleColumnType::Interval => "Interval".into(),
            SimpleColumnType::Bytea => "LargeBinary".into(),
            SimpleColumnType::Uuid => "Uuid".into(),
            SimpleColumnType::Json => "JSON".into(),
            SimpleColumnType::Inet | SimpleColumnType::Cidr | SimpleColumnType::Macaddr => {
                "String(255)".into()
            }
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Numeric { precision, scale } => {
                format!("Numeric({precision}, {scale})")
            }
            ComplexColumnType::Varchar { length } | ComplexColumnType::Char { length } => {
                format!("String({length})")
            }
            ComplexColumnType::Custom { custom_type } => format!("\"{custom_type}\""),
            ComplexColumnType::Enum { name, values } => {
                let class_name = to_pascal_case(name);
                match values {
                    EnumValues::String(_) => format!("Enum({class_name})"),
                    EnumValues::Integer(_) => "Integer".into(), // Integer enums stored as INTEGER
                }
            }
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    }
}
