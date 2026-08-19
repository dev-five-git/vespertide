use serde::{Deserialize, Serialize};

use crate::schema::{
    foreign_key::ForeignKeySyntax,
    names::ColumnName,
    primary_key::PrimaryKeySyntax,
    str_or_bool::{StrOrBoolOrArray, StringOrBool},
};

/// Definition of a single table column, including its type, nullability, and inline constraints.
///
/// Inline constraints (`primary_key`, `unique`, `index`, `foreign_key`) are the preferred way to
/// declare constraints in model JSON files. Call [`TableDef::normalize`] to convert them into
/// table-level [`TableConstraint`] entries before diffing or SQL generation.
///
/// Use [`ColumnDef::new`] to construct a column programmatically, then chain the setter methods
/// (`.primary_key()`, `.unique()`, `.index()`, `.foreign_key()`, `.default()`, `.comment()`) to
/// attach optional fields.
///
/// [`TableDef::normalize`]: crate::schema::TableDef::normalize
/// [`TableConstraint`]: crate::schema::TableConstraint
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// The SQL type of a column, either a parameter-free simple type or a parameterised complex type.
///
/// In JSON model files a simple type is written as a plain string (`"integer"`, `"text"`, etc.)
/// while a complex type is written as an object with a `"kind"` discriminant
/// (`{"kind": "varchar", "length": 255}`).
///
/// Always construct via the wrapped variants:
/// ```
/// use vespertide_core::{ColumnType, SimpleColumnType, ComplexColumnType};
/// let t1 = ColumnType::Simple(SimpleColumnType::Integer);
/// let t2 = ColumnType::Complex(ComplexColumnType::Varchar { length: 255 });
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", untagged)]
pub enum ColumnType {
    /// A parameter-free SQL type such as `INTEGER`, `TEXT`, or `UUID`.
    Simple(SimpleColumnType),
    /// A parameterised SQL type such as `VARCHAR(n)`, `NUMERIC(p,s)`, or a named enum.
    Complex(ComplexColumnType),
}

impl ColumnType {
    /// Returns true if this type supports `auto_increment` (integer types only)
    pub fn supports_auto_increment(&self) -> bool {
        match self {
            ColumnType::Simple(ty) => ty.supports_auto_increment(),
            ColumnType::Complex(_) => false,
        }
    }

    /// Check if two column types require a migration.
    /// For integer enums, no migration is ever needed because the underlying DB type is always INTEGER.
    /// The enum name and values only affect code generation (`SeaORM` entities), not the database schema.
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
                    // String enums: compare only values, not name.
                    // The enum name is a user-facing label; the actual DB type name
                    // is auto-generated with a table prefix at SQL generation time.
                    // Different labels with identical values don't require a migration.
                    values1 != values2
                }
            }
            _ => self != other,
        }
    }

    /// Convert column type to Rust type string (for `SeaORM` entity generation)
    pub fn to_rust_type(&self, nullable: bool) -> String {
        let base = match self {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::SmallInt => "i16".to_string(),
                SimpleColumnType::Integer => "i32".to_string(),
                SimpleColumnType::BigInt => "i64".to_string(),
                SimpleColumnType::Real => "f32".to_string(),
                SimpleColumnType::DoublePrecision => "f64".to_string(),
                SimpleColumnType::Text
                | SimpleColumnType::Interval
                | SimpleColumnType::Inet
                | SimpleColumnType::Cidr
                | SimpleColumnType::Macaddr
                | SimpleColumnType::Xml => "String".to_string(),
                SimpleColumnType::Boolean => "bool".to_string(),
                SimpleColumnType::Date => "Date".to_string(),
                SimpleColumnType::Time => "Time".to_string(),
                SimpleColumnType::Timestamp => "DateTime".to_string(),
                SimpleColumnType::Timestamptz => "DateTimeWithTimeZone".to_string(),
                SimpleColumnType::Bytea => "Vec<u8>".to_string(),
                SimpleColumnType::Uuid => "Uuid".to_string(),
                SimpleColumnType::Json => "Json".to_string(),
            },
            ColumnType::Complex(ty) => match ty {
                ComplexColumnType::Numeric { .. } => "Decimal".to_string(),
                ComplexColumnType::Varchar { .. }
                | ComplexColumnType::Char { .. }
                | ComplexColumnType::Custom { .. }
                | ComplexColumnType::Enum { .. } => "String".to_string(),
            },
        };

        if nullable {
            format!("Option<{base}>")
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
    pub fn default_fill_value(&self) -> &'static str {
        match self {
            ColumnType::Simple(ty) => ty.default_fill_value(),
            ColumnType::Complex(ty) => ty.default_fill_value(),
        }
    }

    /// Get enum variant names if this is an enum type
    /// Returns None if not an enum, Some(names) otherwise
    pub fn enum_variant_names(&self) -> Option<Vec<String>> {
        match self {
            ColumnType::Complex(ComplexColumnType::Enum { values, .. }) => Some(
                values
                    .variant_names()
                    .into_iter()
                    .map(String::from)
                    .collect(),
            ),
            _ => None,
        }
    }
}

impl ColumnDef {
    /// Construct a new column with required fields only.
    /// Use the `.primary_key()`, `.unique()`, `.index()`, `.foreign_key()`,
    /// `.default()`, `.comment()` setters to add optional fields.
    ///
    /// # Examples
    /// ```
    /// use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};
    /// let id = ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false);
    /// ```
    #[must_use]
    pub fn new(name: impl Into<ColumnName>, r#type: ColumnType, nullable: bool) -> Self {
        Self {
            name: name.into(),
            r#type,
            nullable,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    /// Mark this column as part of the primary key.
    #[must_use]
    pub fn primary_key(mut self, pk: PrimaryKeySyntax) -> Self {
        self.primary_key = Some(pk);
        self
    }

    /// Add a unique constraint to this column.
    #[must_use]
    pub fn unique(mut self, unique: StrOrBoolOrArray) -> Self {
        self.unique = Some(unique);
        self
    }

    /// Add an index on this column.
    #[must_use]
    pub fn index(mut self, index: StrOrBoolOrArray) -> Self {
        self.index = Some(index);
        self
    }

    /// Add a foreign key reference from this column.
    #[must_use]
    pub fn foreign_key(mut self, fk: ForeignKeySyntax) -> Self {
        self.foreign_key = Some(fk);
        self
    }

    /// Set the column default value.
    #[must_use]
    pub fn default(mut self, default: StringOrBool) -> Self {
        self.default = Some(default);
        self
    }

    /// Add a column comment.
    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}

/// Parameter-free SQL column types supported across all backends.
///
/// Each variant maps directly to a standard SQL type. Use these via
/// [`ColumnType::Simple`] when no length, precision, or scale is needed.
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SimpleColumnType {
    /// 16-bit signed integer (`SMALLINT`).
    SmallInt,
    /// 32-bit signed integer (`INTEGER`). Supports `auto_increment`.
    Integer,
    /// 64-bit signed integer (`BIGINT`). Supports `auto_increment`.
    BigInt,
    /// 32-bit floating-point number (`REAL`).
    Real,
    /// 64-bit floating-point number (`DOUBLE PRECISION`).
    DoublePrecision,

    // Text types
    /// Unbounded Unicode text (`TEXT`).
    Text,

    // Boolean type
    /// Boolean true/false value (`BOOLEAN`).
    Boolean,

    // Date/Time types
    /// Calendar date without time (`DATE`).
    Date,
    /// Time of day without date (`TIME`).
    Time,
    /// Date and time without timezone (`TIMESTAMP`).
    Timestamp,
    /// Date and time with timezone (`TIMESTAMPTZ`). Prefer this over `Timestamp`.
    Timestamptz,
    /// Time span / duration (`INTERVAL`).
    Interval,

    // Binary type
    /// Variable-length binary data (`BYTEA`).
    Bytea,

    // UUID type
    /// Universally unique identifier (`UUID`).
    Uuid,

    // JSON types
    /// JSON value stored as text (`JSON`). Cross-backend compatible; prefer over `jsonb`.
    Json,

    // Network types
    /// IPv4 or IPv6 host address (`INET`). PostgreSQL-specific.
    Inet,
    /// IPv4 or IPv6 network address (`CIDR`). PostgreSQL-specific.
    Cidr,
    /// MAC address (`MACADDR`). PostgreSQL-specific.
    Macaddr,

    // XML type
    /// XML document (`XML`). PostgreSQL-specific.
    Xml,
}

impl SimpleColumnType {
    /// Returns the SQL type name for this simple column type.
    #[must_use]
    pub fn sql_type(&self) -> &'static str {
        match self {
            SimpleColumnType::SmallInt => "SMALLINT",
            SimpleColumnType::Integer => "INTEGER",
            SimpleColumnType::BigInt => "BIGINT",
            SimpleColumnType::Real => "REAL",
            SimpleColumnType::DoublePrecision => "DOUBLE PRECISION",
            SimpleColumnType::Text => "TEXT",
            SimpleColumnType::Boolean => "BOOLEAN",
            SimpleColumnType::Date => "DATE",
            SimpleColumnType::Time => "TIME",
            SimpleColumnType::Timestamp => "TIMESTAMP",
            SimpleColumnType::Timestamptz => "TIMESTAMPTZ",
            SimpleColumnType::Interval => "INTERVAL",
            SimpleColumnType::Bytea => "BYTEA",
            SimpleColumnType::Uuid => "UUID",
            SimpleColumnType::Json => "JSON",
            SimpleColumnType::Inet => "INET",
            SimpleColumnType::Cidr => "CIDR",
            SimpleColumnType::Macaddr => "MACADDR",
            SimpleColumnType::Xml => "XML",
        }
    }

    /// Returns true if this type supports `auto_increment` (integer types only)
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
    pub fn default_fill_value(&self) -> &'static str {
        match self {
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => {
                "0"
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "0.0",
            SimpleColumnType::Boolean => "false",
            SimpleColumnType::Text | SimpleColumnType::Bytea => "''",
            SimpleColumnType::Date => "'1970-01-01'",
            SimpleColumnType::Time => "'00:00:00'",
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "CURRENT_TIMESTAMP",
            SimpleColumnType::Interval => "'0'",
            SimpleColumnType::Uuid => "'00000000-0000-0000-0000-000000000000'",
            SimpleColumnType::Json => "'{}'",
            SimpleColumnType::Inet | SimpleColumnType::Cidr => "'0.0.0.0'",
            SimpleColumnType::Macaddr => "'00:00:00:00:00:00'",
            SimpleColumnType::Xml => "'<xml/>'",
        }
    }
}

/// Closed, exhaustive mirror of [`SimpleColumnType`] for downstream crates that need to
/// `match` on it without a wildcard arm.
///
/// [`SimpleColumnType`] is `#[non_exhaustive]`, so every match on it made *outside*
/// `vespertide-core` must carry a `_` arm even when every current variant is already
/// handled — that arm is genuinely unreachable and shows up as a permanent 0-hit line
/// under coverage instrumentation. This type is deliberately **not** `#[non_exhaustive]`:
/// the conversion below is written inside the crate that owns `SimpleColumnType`, where
/// the non-exhaustiveness restriction doesn't apply, so it can be matched exhaustively
/// with no wildcard, here and in every downstream crate. If `SimpleColumnType` ever gains
/// a variant, `From<SimpleColumnType> for SimpleColumnKind` below fails to compile until
/// a matching variant is added here — a compile-time forcing function that replaces the
/// old pattern of a runtime `unreachable!()` guard that only a test could catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleColumnKind {
    SmallInt,
    Integer,
    BigInt,
    Real,
    DoublePrecision,
    Text,
    Boolean,
    Date,
    Time,
    Timestamp,
    Timestamptz,
    Interval,
    Bytea,
    Uuid,
    Json,
    Inet,
    Cidr,
    Macaddr,
    Xml,
}

impl From<SimpleColumnType> for SimpleColumnKind {
    fn from(ty: SimpleColumnType) -> Self {
        match ty {
            SimpleColumnType::SmallInt => Self::SmallInt,
            SimpleColumnType::Integer => Self::Integer,
            SimpleColumnType::BigInt => Self::BigInt,
            SimpleColumnType::Real => Self::Real,
            SimpleColumnType::DoublePrecision => Self::DoublePrecision,
            SimpleColumnType::Text => Self::Text,
            SimpleColumnType::Boolean => Self::Boolean,
            SimpleColumnType::Date => Self::Date,
            SimpleColumnType::Time => Self::Time,
            SimpleColumnType::Timestamp => Self::Timestamp,
            SimpleColumnType::Timestamptz => Self::Timestamptz,
            SimpleColumnType::Interval => Self::Interval,
            SimpleColumnType::Bytea => Self::Bytea,
            SimpleColumnType::Uuid => Self::Uuid,
            SimpleColumnType::Json => Self::Json,
            SimpleColumnType::Inet => Self::Inet,
            SimpleColumnType::Cidr => Self::Cidr,
            SimpleColumnType::Macaddr => Self::Macaddr,
            SimpleColumnType::Xml => Self::Xml,
        }
    }
}

/// A single variant of an integer-backed enum, pairing a Rust-friendly name with its stored value.
///
/// Used inside [`EnumValues::Integer`] to define enums that are stored as `INTEGER` in the
/// database. Leave gaps between values (e.g. 0, 10, 20) so new variants can be inserted later
/// without renumbering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NumValue {
    /// The variant name used in generated code (e.g. `"active"`).
    pub name: String,
    /// The integer value stored in the database column.
    pub value: i64,
}

/// The set of allowed values for an enum column, either string-based or integer-based.
///
/// **String enums** map to a native `PostgreSQL` `ENUM` type. Adding or removing values requires a
/// database migration (`ALTER TYPE`).
///
/// **Integer enums** are stored as `INTEGER`. New variants can be added to the model without any
/// database migration because the underlying column type never changes.
///
/// Choose integer enums for expandable value sets (roles, priorities) and string enums for
/// stable, human-readable status fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum EnumValues {
    /// String enum: each variant is a plain string stored in a native DB enum type.
    String(Vec<String>),
    /// Integer enum: each variant has an explicit numeric value stored as `INTEGER`.
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
            EnumValues::String(values) => values.iter().map(std::string::String::as_str).collect(),
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
        match self {
            EnumValues::String(values) => values.is_empty(),
            EnumValues::Integer(values) => values.is_empty(),
        }
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
        EnumValues::String(
            values
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
        )
    }
}

/// Parameterised SQL column types that require additional configuration beyond a simple keyword.
///
/// In JSON model files these are written as objects with a `"kind"` discriminant, for example
/// `{"kind": "varchar", "length": 255}` or `{"kind": "enum", "name": "status", "values": [...]}`.
///
/// Use these via [`ColumnType::Complex`].
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "kind")]
#[non_exhaustive]
pub enum ComplexColumnType {
    /// Variable-length character string with a maximum byte length (`VARCHAR(n)`).
    Varchar { length: u32 },
    /// Exact fixed-point number with configurable precision and scale (`NUMERIC(p, s)`).
    Numeric { precision: u32, scale: u32 },
    /// Fixed-length character string padded with spaces (`CHAR(n)`).
    Char { length: u32 },
    /// Escape hatch for database-specific types not covered by other variants.
    /// Breaks cross-database portability; avoid unless absolutely necessary.
    Custom { custom_type: String },
    /// Named enum type. String enums map to a native DB enum; integer enums store as `INTEGER`.
    /// See [`EnumValues`] for the distinction.
    Enum { name: String, values: EnumValues },
}

impl ComplexColumnType {
    /// Returns the base SQL type name for this complex column type, without parameters.
    #[must_use]
    pub fn sql_type(&self) -> &'static str {
        match self {
            ComplexColumnType::Varchar { .. } => "VARCHAR",
            ComplexColumnType::Numeric { .. } => "NUMERIC",
            ComplexColumnType::Char { .. } => "CHAR",
            ComplexColumnType::Custom { .. } => "CUSTOM",
            ComplexColumnType::Enum { .. } => "ENUM",
        }
    }

    /// Convert to human-readable display string
    pub fn to_display_string(&self) -> String {
        match self {
            ComplexColumnType::Varchar { length } => format!("varchar({length})"),
            ComplexColumnType::Numeric { precision, scale } => {
                format!("numeric({precision},{scale})")
            }
            ComplexColumnType::Char { length } => format!("char({length})"),
            ComplexColumnType::Custom { custom_type } => custom_type.to_lowercase(),
            ComplexColumnType::Enum { name, values } => {
                if values.is_integer() {
                    format!("enum<{name}> (integer)")
                } else {
                    format!("enum<{name}>")
                }
            }
        }
    }

    /// Get the default fill value for this type.
    pub fn default_fill_value(&self) -> &'static str {
        match self {
            ComplexColumnType::Numeric { .. } => "0",
            ComplexColumnType::Varchar { .. }
            | ComplexColumnType::Char { .. }
            | ComplexColumnType::Custom { .. }
            | ComplexColumnType::Enum { .. } => "''",
        }
    }
}
