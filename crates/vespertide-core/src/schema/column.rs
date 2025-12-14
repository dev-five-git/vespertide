use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{foreign_key::ForeignKeyDef, names::ColumnName, str_or_bool::StrOrBoolOrArray};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub r#type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
    pub primary_key: Option<bool>,
    pub unique: Option<StrOrBoolOrArray>,
    pub index: Option<StrOrBoolOrArray>,
    pub foreign_key: Option<ForeignKeyDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum ColumnType {
    Simple(SimpleColumnType),
    Complex(ComplexColumnType),
}

impl ColumnType {
    /// Convert column type to PostgreSQL SQL type string
    pub fn to_sql(&self) -> String {
        match self {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::SmallInt => "SMALLINT".into(),
                SimpleColumnType::Integer => "INTEGER".into(),
                SimpleColumnType::BigInt => "BIGINT".into(),
                SimpleColumnType::Real => "REAL".into(),
                SimpleColumnType::DoublePrecision => "DOUBLE PRECISION".into(),
                SimpleColumnType::Text => "TEXT".into(),
                SimpleColumnType::Boolean => "BOOLEAN".into(),
                SimpleColumnType::Date => "DATE".into(),
                SimpleColumnType::Time => "TIME".into(),
                SimpleColumnType::Timestamp => "TIMESTAMP".into(),
                SimpleColumnType::Timestamptz => "TIMESTAMPTZ".into(),
                SimpleColumnType::Interval => "INTERVAL".into(),
                SimpleColumnType::Bytea => "BYTEA".into(),
                SimpleColumnType::Uuid => "UUID".into(),
                SimpleColumnType::Json => "JSON".into(),
                SimpleColumnType::Jsonb => "JSONB".into(),
                SimpleColumnType::Inet => "INET".into(),
                SimpleColumnType::Cidr => "CIDR".into(),
                SimpleColumnType::Macaddr => "MACADDR".into(),
                SimpleColumnType::Xml => "XML".into(),
            },
            ColumnType::Complex(ty) => match ty {
                ComplexColumnType::Varchar { length } => format!("VARCHAR({})", length),
                ComplexColumnType::Numeric { precision, scale } => format!("NUMERIC({}, {})", precision, scale),
                ComplexColumnType::Char { length } => format!("CHAR({})", length),
                ComplexColumnType::Custom { custom_type } => custom_type.clone(),
            },
        }
    }

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
}

impl From<SimpleColumnType> for ColumnType {
    fn from(ty: SimpleColumnType) -> Self {
        ColumnType::Simple(ty)
    }
}

impl From<ComplexColumnType> for ColumnType {
    fn from(ty: ComplexColumnType) -> Self {
        ColumnType::Complex(ty)
    }
}
