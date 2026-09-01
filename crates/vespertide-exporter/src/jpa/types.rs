use std::collections::HashSet;

use vespertide_core::ColumnDef;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};

use crate::jpa::render::to_pascal_case;

/// Track which Java imports are actually used to generate minimal import statements.
#[derive(Default)]
pub(super) struct UsedImports {
    pub(super) java_time_types: HashSet<&'static str>,
    pub(super) needs_uuid: bool,
    pub(super) needs_big_decimal: bool,
}

impl UsedImports {
    pub(super) fn add_column_type(&mut self, col_type: &ColumnType) {
        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::Date => {
                    self.java_time_types.insert("LocalDate");
                }
                SimpleColumnType::Time => {
                    self.java_time_types.insert("LocalTime");
                }
                SimpleColumnType::Timestamp => {
                    self.java_time_types.insert("LocalDateTime");
                }
                SimpleColumnType::Timestamptz => {
                    self.java_time_types.insert("OffsetDateTime");
                }
                SimpleColumnType::Uuid => {
                    self.needs_uuid = true;
                }
                _ => {}
            },
            ColumnType::Complex(ty) => {
                if let ComplexColumnType::Numeric { .. } = ty {
                    self.needs_big_decimal = true;
                }
            }
        }
    }
}

pub(super) fn column_type_to_java(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "Short",
            SimpleColumnType::Integer => "Integer",
            SimpleColumnType::BigInt => "Long",
            SimpleColumnType::Real => "Float",
            SimpleColumnType::DoublePrecision => "Double",
            SimpleColumnType::Boolean => "Boolean",
            SimpleColumnType::Text
            | SimpleColumnType::Xml
            | SimpleColumnType::Interval
            | SimpleColumnType::Json
            | SimpleColumnType::Inet
            | SimpleColumnType::Cidr
            | SimpleColumnType::Macaddr => "String",
            SimpleColumnType::Date => "LocalDate",
            SimpleColumnType::Time => "LocalTime",
            SimpleColumnType::Timestamp => "LocalDateTime",
            SimpleColumnType::Timestamptz => "OffsetDateTime",
            SimpleColumnType::Bytea => "byte[]",
            SimpleColumnType::Uuid => "UUID",
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Numeric { .. } => "BigDecimal",
            // Integer enums are stored as Integer in JPA
            ComplexColumnType::Enum {
                values: EnumValues::Integer(_),
                ..
            } => "Integer",
            // String enums use the generated enum class — handled separately
            ComplexColumnType::Varchar { .. }
            | ComplexColumnType::Char { .. }
            | ComplexColumnType::Custom { .. }
            | ComplexColumnType::Enum {
                values: EnumValues::String(_),
                ..
            } => "String", // placeholder, overridden in render_field
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    }
}

/// Get the Java type for a column, including enum class names.
pub(super) fn java_type_for_column(col: &ColumnDef) -> String {
    if let ColumnType::Complex(ComplexColumnType::Enum {
        name,
        values: EnumValues::String(_),
    }) = &col.r#type
    {
        to_pascal_case(name)
    } else {
        column_type_to_java(&col.r#type).to_string()
    }
}
