use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{
    foreign_key::ForeignKeySyntax, names::ColumnName, primary_key::PrimaryKeySyntax,
    str_or_bool::StrOrBoolOrArray,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub r#type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
    pub primary_key: Option<PrimaryKeySyntax>,
    pub unique: Option<StrOrBoolOrArray>,
    pub index: Option<StrOrBoolOrArray>,
    pub foreign_key: Option<ForeignKeySyntax>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum ColumnType {
    Simple(SimpleColumnType),
    Complex(ComplexColumnType),
}

impl ColumnType {
    /// Convert column type to Rust type string (for SeaORM entity generation)
    pub fn to_rust_type(&self, nullable: bool) -> String {
        let base = match self {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::SmallInt => "i16".to_string(),
                SimpleColumnType::Integer => "i32".to_string(),
                SimpleColumnType::BigInt => "i64".to_string(),
                SimpleColumnType::Real => "f32".to_string(),
                SimpleColumnType::DoublePrecision => "f64".to_string(),
                SimpleColumnType::Text => "String".to_string(),
                SimpleColumnType::Boolean => "bool".to_string(),
                SimpleColumnType::Date => "Date".to_string(),
                SimpleColumnType::Time => "Time".to_string(),
                SimpleColumnType::Timestamp => "DateTime".to_string(),
                SimpleColumnType::Timestamptz => "DateTimeWithTimeZone".to_string(),
                SimpleColumnType::Interval => "String".to_string(),
                SimpleColumnType::Bytea => "Vec<u8>".to_string(),
                SimpleColumnType::Uuid => "Uuid".to_string(),
                SimpleColumnType::Json | SimpleColumnType::Jsonb => "Json".to_string(),
                SimpleColumnType::Inet | SimpleColumnType::Cidr => "String".to_string(),
                SimpleColumnType::Macaddr => "String".to_string(),
                SimpleColumnType::Xml => "String".to_string(),
            },
            ColumnType::Complex(ty) => match ty {
                ComplexColumnType::Varchar { .. } => "String".to_string(),
                ComplexColumnType::Numeric { .. } => "Decimal".to_string(),
                ComplexColumnType::Char { .. } => "String".to_string(),
                ComplexColumnType::Custom { .. } => "String".to_string(), // Default for custom types
                ComplexColumnType::Enum { .. } => "String".to_string(),
            },
        };

        if nullable {
            format!("Option<{}>", base)
        } else {
            base
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimpleColumnType {
    SmallInt,
    Integer,
    BigInt,
    Real,
    DoublePrecision,

    // Text types
    Text,

    // Boolean type
    Boolean,

    // Date/Time types
    Date,
    Time,
    Timestamp,
    Timestamptz,
    Interval,

    // Binary type
    Bytea,

    // UUID type
    Uuid,

    // JSON types
    Json,
    Jsonb,

    // Network types
    Inet,
    Cidr,
    Macaddr,

    // XML type
    Xml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ComplexColumnType {
    Varchar { length: u32 },
    Numeric { precision: u32, scale: u32 },
    Char { length: u32 },
    Custom { custom_type: String },
    Enum { name: String, values: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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
    #[case(SimpleColumnType::Jsonb, "Json")]
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
    #[case(SimpleColumnType::Jsonb, "Option<Json>")]
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
    #[case(ComplexColumnType::Enum { name: "status".into(), values: vec!["active".into(), "inactive".into()] }, false, "String")]
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
    #[case(ComplexColumnType::Enum { name: "status".into(), values: vec!["active".into(), "inactive".into()] }, "Option<String>")]
    fn test_complex_column_type_to_rust_type_nullable(
        #[case] column_type: ComplexColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(
            ColumnType::Complex(column_type).to_rust_type(true),
            expected
        );
    }
}
