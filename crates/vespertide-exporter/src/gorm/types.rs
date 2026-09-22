use std::collections::HashMap;

use super::render::exported_go_name;
use crate::utils::common::is_jsonb_custom_type;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};

/// Track which Go imports are actually used to generate minimal import statements.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent import-presence flags; enum would add verbosity without clarity"
)]
#[derive(Default)]
pub(super) struct UsedImports {
    pub(super) needs_time: bool,
    pub(super) needs_uuid: bool,
    pub(super) needs_datatypes: bool,
    pub(super) needs_decimal: bool,
}

impl UsedImports {
    pub(super) fn add_column_type(&mut self, col_type: &ColumnType) {
        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::Date
                | SimpleColumnType::Time
                | SimpleColumnType::Timestamp
                | SimpleColumnType::Timestamptz => {
                    self.needs_time = true;
                }
                SimpleColumnType::Uuid => {
                    self.needs_uuid = true;
                }
                SimpleColumnType::Json => {
                    self.needs_datatypes = true;
                }
                _ => {}
            },
            ColumnType::Complex(ty) => {
                if let ComplexColumnType::Numeric { .. } = ty {
                    self.needs_decimal = true;
                }
                if let ComplexColumnType::Custom { custom_type } = ty
                    && is_jsonb_custom_type(custom_type)
                {
                    self.needs_datatypes = true;
                }
            }
        }
    }
}

pub(super) fn go_type_for_column_mapped(
    col_type: &ColumnType,
    nullable: bool,
    enum_map: &HashMap<&str, String>,
) -> String {
    let base = match col_type {
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => enum_map
            .get(name.as_str())
            .cloned()
            .unwrap_or_else(|| exported_go_name(name)),
        _ => go_base_type(col_type),
    };
    if nullable { format!("*{base}") } else { base }
}

/// Whether the column's Go field is a `string`, or an enum type declared over
/// one.
pub(super) fn is_go_string(col_type: &ColumnType) -> bool {
    match col_type {
        ColumnType::Complex(ComplexColumnType::Enum { values, .. }) => {
            matches!(values, EnumValues::String(_))
        }
        _ => go_base_type(col_type) == "string",
    }
}

fn go_base_type(col_type: &ColumnType) -> String {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "int16".to_string(),
            SimpleColumnType::Integer => "int32".to_string(),
            SimpleColumnType::BigInt => "int64".to_string(),
            SimpleColumnType::Real => "float32".to_string(),
            SimpleColumnType::DoublePrecision => "float64".to_string(),
            SimpleColumnType::Text
            | SimpleColumnType::Xml
            | SimpleColumnType::Inet
            | SimpleColumnType::Cidr
            | SimpleColumnType::Macaddr
            | SimpleColumnType::Interval => "string".to_string(),
            SimpleColumnType::Boolean => "bool".to_string(),
            SimpleColumnType::Date
            | SimpleColumnType::Time
            | SimpleColumnType::Timestamp
            | SimpleColumnType::Timestamptz => "time.Time".to_string(),
            SimpleColumnType::Bytea => "[]byte".to_string(),
            SimpleColumnType::Uuid => "uuid.UUID".to_string(),
            SimpleColumnType::Json => "datatypes.JSON".to_string(),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                "string".to_string()
            }
            ComplexColumnType::Custom { custom_type } => {
                if is_jsonb_custom_type(custom_type) {
                    "datatypes.JSON".to_string()
                } else {
                    "string".to_string()
                }
            }
            ComplexColumnType::Numeric { .. } => "decimal.Decimal".to_string(),
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;
    use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};

    use super::go_type_for_column_mapped;

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
    fn column_types_map_to_go_types(
        #[case] col_type: ColumnType,
        #[case] nullable: bool,
        #[case] expected: &str,
    ) {
        assert_eq!(
            go_type_for_column_mapped(&col_type, nullable, &HashMap::new()),
            expected
        );
    }
}
