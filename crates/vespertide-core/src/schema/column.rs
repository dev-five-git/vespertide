use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{
    foreign_key::ForeignKeySyntax,
    names::ColumnName,
    primary_key::PrimaryKeySyntax,
    str_or_bool::{StrOrBoolOrArray, StringOrBool},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub r#type: ColumnType,
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<StringOrBool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<PrimaryKeySyntax>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique: Option<StrOrBoolOrArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<StrOrBoolOrArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreign_key: Option<ForeignKeySyntax>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum ColumnType {
    Simple(SimpleColumnType),
    Complex(ComplexColumnType),
}

impl ColumnType {
    /// Returns true if this type supports auto_increment (integer types only)
    pub fn supports_auto_increment(&self) -> bool {
        match self {
            ColumnType::Simple(ty) => ty.supports_auto_increment(),
            ColumnType::Complex(_) => false,
        }
    }

    /// Check if two column types require a migration.
    /// For integer enums, no migration is ever needed because the underlying DB type is always INTEGER.
    /// The enum name and values only affect code generation (SeaORM entities), not the database schema.
    pub fn requires_migration(&self, other: &ColumnType) -> bool {
        match (self, other) {
            (
                ColumnType::Complex(ComplexColumnType::Enum {
                    values: values1, ..
                }),
                ColumnType::Complex(ComplexColumnType::Enum {
                    values: values2, ..
                }),
            ) => {
                // Both are integer enums - never require migration (DB type is always INTEGER)
                if values1.is_integer() && values2.is_integer() {
                    false
                } else {
                    // At least one is string enum - compare fully
                    self != other
                }
            }
            _ => self != other,
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
                SimpleColumnType::Json => "Json".to_string(),
                // SimpleColumnType::Jsonb => "Json".to_string(),
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

    /// Convert column type to human-readable display string (for CLI prompts)
    /// Examples: "integer", "text", "varchar(255)", "numeric(10,2)"
    pub fn to_display_string(&self) -> String {
        match self {
            ColumnType::Simple(ty) => ty.to_display_string(),
            ColumnType::Complex(ty) => ty.to_display_string(),
        }
    }

    /// Get the default fill value for this column type (for CLI prompts)
    /// Returns None if no sensible default exists for the type
    pub fn default_fill_value(&self) -> Option<&'static str> {
        match self {
            ColumnType::Simple(ty) => ty.default_fill_value(),
            ColumnType::Complex(ty) => ty.default_fill_value(),
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
    // Jsonb,

    // Network types
    Inet,
    Cidr,
    Macaddr,

    // XML type
    Xml,
}

impl SimpleColumnType {
    /// Returns true if this type supports auto_increment (integer types only)
    pub fn supports_auto_increment(&self) -> bool {
        matches!(
            self,
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt
        )
    }

    /// Convert to human-readable display string
    pub fn to_display_string(&self) -> String {
        match self {
            SimpleColumnType::SmallInt => "smallint".to_string(),
            SimpleColumnType::Integer => "integer".to_string(),
            SimpleColumnType::BigInt => "bigint".to_string(),
            SimpleColumnType::Real => "real".to_string(),
            SimpleColumnType::DoublePrecision => "double precision".to_string(),
            SimpleColumnType::Text => "text".to_string(),
            SimpleColumnType::Boolean => "boolean".to_string(),
            SimpleColumnType::Date => "date".to_string(),
            SimpleColumnType::Time => "time".to_string(),
            SimpleColumnType::Timestamp => "timestamp".to_string(),
            SimpleColumnType::Timestamptz => "timestamptz".to_string(),
            SimpleColumnType::Interval => "interval".to_string(),
            SimpleColumnType::Bytea => "bytea".to_string(),
            SimpleColumnType::Uuid => "uuid".to_string(),
            SimpleColumnType::Json => "json".to_string(),
            SimpleColumnType::Inet => "inet".to_string(),
            SimpleColumnType::Cidr => "cidr".to_string(),
            SimpleColumnType::Macaddr => "macaddr".to_string(),
            SimpleColumnType::Xml => "xml".to_string(),
        }
    }

    /// Get the default fill value for this type
    /// Returns None if no sensible default exists
    pub fn default_fill_value(&self) -> Option<&'static str> {
        match self {
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => {
                Some("0")
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => Some("0.0"),
            SimpleColumnType::Boolean => Some("false"),
            SimpleColumnType::Text => Some("''"),
            _ => None,
        }
    }
}

/// Integer enum variant with name and numeric value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NumValue {
    pub name: String,
    pub value: i32,
}

/// Enum values definition - either all string or all integer
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EnumValues {
    String(Vec<String>),
    Integer(Vec<NumValue>),
}

impl EnumValues {
    /// Check if this is a string enum
    pub fn is_string(&self) -> bool {
        matches!(self, EnumValues::String(_))
    }

    /// Check if this is an integer enum
    pub fn is_integer(&self) -> bool {
        matches!(self, EnumValues::Integer(_))
    }

    /// Get all variant names
    pub fn variant_names(&self) -> Vec<&str> {
        match self {
            EnumValues::String(values) => values.iter().map(|s| s.as_str()).collect(),
            EnumValues::Integer(values) => values.iter().map(|v| v.name.as_str()).collect(),
        }
    }

    /// Get the number of variants
    pub fn len(&self) -> usize {
        match self {
            EnumValues::String(values) => values.len(),
            EnumValues::Integer(values) => values.len(),
        }
    }

    /// Check if there are no variants
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get SQL values for CREATE TYPE ENUM (only for string enums)
    /// Returns quoted strings like 'value1', 'value2'
    pub fn to_sql_values(&self) -> Vec<String> {
        match self {
            EnumValues::String(values) => values
                .iter()
                .map(|s| format!("'{}'", s.replace('\'', "''")))
                .collect(),
            EnumValues::Integer(values) => values.iter().map(|v| v.value.to_string()).collect(),
        }
    }
}

impl From<Vec<String>> for EnumValues {
    fn from(values: Vec<String>) -> Self {
        EnumValues::String(values)
    }
}

impl From<Vec<&str>> for EnumValues {
    fn from(values: Vec<&str>) -> Self {
        EnumValues::String(values.into_iter().map(|s| s.to_string()).collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ComplexColumnType {
    Varchar { length: u32 },
    Numeric { precision: u32, scale: u32 },
    Char { length: u32 },
    Custom { custom_type: String },
    Enum { name: String, values: EnumValues },
}

impl ComplexColumnType {
    /// Convert to human-readable display string
    pub fn to_display_string(&self) -> String {
        match self {
            ComplexColumnType::Varchar { length } => format!("varchar({})", length),
            ComplexColumnType::Numeric { precision, scale } => {
                format!("numeric({},{})", precision, scale)
            }
            ComplexColumnType::Char { length } => format!("char({})", length),
            ComplexColumnType::Custom { custom_type } => custom_type.to_lowercase(),
            ComplexColumnType::Enum { name, values } => {
                if values.is_integer() {
                    format!("enum<{}> (integer)", name)
                } else {
                    format!("enum<{}>", name)
                }
            }
        }
    }

    /// Get the default fill value for this type
    /// Returns None if no sensible default exists
    pub fn default_fill_value(&self) -> Option<&'static str> {
        match self {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => Some("''"),
            ComplexColumnType::Numeric { .. } => Some("0"),
            _ => None,
        }
    }
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
            values: EnumValues::String(vec!["pending".into(), "active".into(), "completed".into()]),
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
    #[case(SimpleColumnType::SmallInt, Some("0"))]
    #[case(SimpleColumnType::Integer, Some("0"))]
    #[case(SimpleColumnType::BigInt, Some("0"))]
    #[case(SimpleColumnType::Real, Some("0.0"))]
    #[case(SimpleColumnType::DoublePrecision, Some("0.0"))]
    #[case(SimpleColumnType::Boolean, Some("false"))]
    #[case(SimpleColumnType::Text, Some("''"))]
    #[case(SimpleColumnType::Date, None)]
    #[case(SimpleColumnType::Time, None)]
    #[case(SimpleColumnType::Timestamp, None)]
    #[case(SimpleColumnType::Uuid, None)]
    fn test_simple_column_type_default_fill_value(
        #[case] column_type: SimpleColumnType,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(column_type.default_fill_value(), expected);
    }

    #[test]
    fn test_complex_column_type_default_fill_value_varchar() {
        let ty = ComplexColumnType::Varchar { length: 255 };
        assert_eq!(ty.default_fill_value(), Some("''"));
    }

    #[test]
    fn test_complex_column_type_default_fill_value_char() {
        let ty = ComplexColumnType::Char { length: 1 };
        assert_eq!(ty.default_fill_value(), Some("''"));
    }

    #[test]
    fn test_complex_column_type_default_fill_value_numeric() {
        let ty = ComplexColumnType::Numeric {
            precision: 10,
            scale: 2,
        };
        assert_eq!(ty.default_fill_value(), Some("0"));
    }

    #[test]
    fn test_complex_column_type_default_fill_value_custom() {
        let ty = ComplexColumnType::Custom {
            custom_type: "MONEY".into(),
        };
        assert_eq!(ty.default_fill_value(), None);
    }

    #[test]
    fn test_complex_column_type_default_fill_value_enum() {
        let ty = ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into()]),
        };
        assert_eq!(ty.default_fill_value(), None);
    }

    #[test]
    fn test_column_type_default_fill_value_simple() {
        let ty = ColumnType::Simple(SimpleColumnType::Integer);
        assert_eq!(ty.default_fill_value(), Some("0"));
    }

    #[test]
    fn test_column_type_default_fill_value_complex() {
        let ty = ColumnType::Complex(ComplexColumnType::Varchar { length: 100 });
        assert_eq!(ty.default_fill_value(), Some("''"));
    }
}
