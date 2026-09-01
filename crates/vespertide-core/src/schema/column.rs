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
        let base: &'static str = match self {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::SmallInt => "i16",
                SimpleColumnType::Integer => "i32",
                SimpleColumnType::BigInt => "i64",
                SimpleColumnType::Real => "f32",
                SimpleColumnType::DoublePrecision => "f64",
                SimpleColumnType::Text
                | SimpleColumnType::Interval
                | SimpleColumnType::Inet
                | SimpleColumnType::Cidr
                | SimpleColumnType::Macaddr
                | SimpleColumnType::Xml => "String",
                SimpleColumnType::Boolean => "bool",
                SimpleColumnType::Date => "Date",
                SimpleColumnType::Time => "Time",
                SimpleColumnType::Timestamp => "DateTime",
                SimpleColumnType::Timestamptz => "DateTimeWithTimeZone",
                SimpleColumnType::Bytea => "Vec<u8>",
                SimpleColumnType::Uuid => "Uuid",
                SimpleColumnType::Json => "Json",
            },
            ColumnType::Complex(ty) => match ty {
                ComplexColumnType::Numeric { .. } => "Decimal",
                ComplexColumnType::Varchar { .. }
                | ComplexColumnType::Char { .. }
                | ComplexColumnType::Custom { .. }
                | ComplexColumnType::Enum { .. } => "String",
            },
        };

        if nullable {
            format!("Option<{base}>")
        } else {
            base.to_string()
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

    /// Render the type in the **model-file (wire-format) spelling** — the
    /// exact syntax users write in JSON/YAML models: `small_int`,
    /// `varchar(32)`, `numeric(10, 2)`, `enum(status)`, `custom(TSVECTOR)`.
    /// Use this for user-facing diagnostics that echo model syntax.
    /// Distinct from [`Self::to_display_string`], which renders
    /// SQL-flavoured names for CLI prompts (`smallint`, `double precision`,
    /// `enum<status>`), and from `Debug`, which leaks Rust internals
    /// (`Simple(Integer)`, `Varchar { length: 32 }`).
    #[must_use]
    pub fn display_label(&self) -> String {
        match self {
            ColumnType::Simple(simple) => simple.model_name().to_string(),
            ColumnType::Complex(complex) => complex.display_label(),
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
        let ColumnType::Complex(ComplexColumnType::Enum { values, .. }) = self else {
            return None;
        };
        Some(match values {
            EnumValues::String(v) => v.clone(),
            EnumValues::Integer(v) => v.iter().map(|n| n.name.clone()).collect(),
        })
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

    /// Returns the snake_case model-file spelling of this type — the exact
    /// string users write in JSON/YAML models (the serde wire name), e.g.
    /// `SmallInt` → `"small_int"`. Use this for user-facing messages that
    /// reference the model syntax; use [`Self::sql_type`] for SQL rendering.
    #[must_use]
    pub fn model_name(&self) -> &'static str {
        match self {
            SimpleColumnType::SmallInt => "small_int",
            SimpleColumnType::Integer => "integer",
            SimpleColumnType::BigInt => "big_int",
            SimpleColumnType::Real => "real",
            SimpleColumnType::DoublePrecision => "double_precision",
            SimpleColumnType::Text => "text",
            SimpleColumnType::Boolean => "boolean",
            SimpleColumnType::Date => "date",
            SimpleColumnType::Time => "time",
            SimpleColumnType::Timestamp => "timestamp",
            SimpleColumnType::Timestamptz => "timestamptz",
            SimpleColumnType::Interval => "interval",
            SimpleColumnType::Bytea => "bytea",
            SimpleColumnType::Uuid => "uuid",
            SimpleColumnType::Json => "json",
            SimpleColumnType::Inet => "inet",
            SimpleColumnType::Cidr => "cidr",
            SimpleColumnType::Macaddr => "macaddr",
            SimpleColumnType::Xml => "xml",
        }
    }

    /// Returns true if this type supports `auto_increment` (integer types only)
    pub fn supports_auto_increment(&self) -> bool {
        matches!(
            self,
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt
        )
    }

    /// Borrow the human-readable display label as a `&'static str`.
    ///
    /// Prefer this over [`Self::to_display_string`] when the caller only needs
    /// to read the label — it avoids an allocation for a compile-time constant.
    #[must_use]
    pub fn display_str(&self) -> &'static str {
        match self {
            SimpleColumnType::SmallInt => "smallint",
            SimpleColumnType::Integer => "integer",
            SimpleColumnType::BigInt => "bigint",
            SimpleColumnType::Real => "real",
            SimpleColumnType::DoublePrecision => "double precision",
            SimpleColumnType::Text => "text",
            SimpleColumnType::Boolean => "boolean",
            SimpleColumnType::Date => "date",
            SimpleColumnType::Time => "time",
            SimpleColumnType::Timestamp => "timestamp",
            SimpleColumnType::Timestamptz => "timestamptz",
            SimpleColumnType::Interval => "interval",
            SimpleColumnType::Bytea => "bytea",
            SimpleColumnType::Uuid => "uuid",
            SimpleColumnType::Json => "json",
            SimpleColumnType::Inet => "inet",
            SimpleColumnType::Cidr => "cidr",
            SimpleColumnType::Macaddr => "macaddr",
            SimpleColumnType::Xml => "xml",
        }
    }

    /// Convert to human-readable display string
    #[must_use]
    pub fn to_display_string(&self) -> String {
        self.display_str().to_string()
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

    /// Join every variant *name* with `separator`, writing straight into one
    /// buffer — no intermediate `Vec<&str>` allocation.
    ///
    /// Unlike [`Self::sql_values_joined`] (which emits SQL literals: quoted
    /// strings / integer values), this emits the human-readable variant
    /// *names* for diagnostics such as "allowed values are: a, b, c". For an
    /// integer enum the member names are used.
    ///
    /// ```rust
    /// use vespertide_core::{EnumValues, NumValue};
    ///
    /// let s = EnumValues::String(vec!["active".into(), "inactive".into()]);
    /// assert_eq!(s.variant_names_joined(", "), "active, inactive");
    ///
    /// let i = EnumValues::Integer(vec![
    ///     NumValue { name: "low".into(),  value: 0  },
    ///     NumValue { name: "high".into(), value: 10 },
    /// ]);
    /// assert_eq!(i.variant_names_joined(", "), "low, high");
    ///
    /// let empty = EnumValues::String(vec![]);
    /// assert_eq!(empty.variant_names_joined(", "), "");
    /// ```
    #[must_use]
    pub fn variant_names_joined(&self, separator: &str) -> String {
        let mut out = String::new();
        match self {
            EnumValues::String(values) => {
                for (i, s) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(separator);
                    }
                    out.push_str(s);
                }
            }
            EnumValues::Integer(values) => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(separator);
                    }
                    out.push_str(&v.name);
                }
            }
        }
        out
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

    /// Returns `true` when `value` matches any variant of this enum.
    ///
    /// For string enums the comparison is exact-string against each variant.
    /// For integer enums the value is first parsed as `i64` (matching the
    /// `NumValue::value` storage type): successful parses match against
    /// `NumValue::value`, failed parses fall back to matching against
    /// `NumValue::name`. Mirrors the loose JSON-default behaviour expected by
    /// the planner validator so a model author can write either the numeric
    /// literal (`5`) or the variant name (`"Active"`) for an integer enum
    /// default.
    #[must_use]
    pub fn contains_value(&self, value: &str) -> bool {
        match self {
            EnumValues::String(variants) => variants.iter().any(|v| v == value),
            EnumValues::Integer(variants) => value.parse::<i64>().map_or_else(
                |_| variants.iter().any(|v| v.name == value),
                |n| variants.iter().any(|v| v.value == n),
            ),
        }
    }

    /// Format every variant for `CREATE TYPE … AS ENUM(...)` /
    /// `CHECK (col IN (...))` and join with `separator`, writing into one
    /// buffer — no intermediate `Vec<String>` allocation.
    ///
    /// Mirrors `vespertide_query::sql::helpers::quote_idents` and
    /// `vespertide_core::schema::names::join_column_names`.
    ///
    /// ```rust
    /// use vespertide_core::{EnumValues, NumValue};
    ///
    /// let s = EnumValues::String(vec!["active".into(), "O'Brien".into()]);
    /// assert_eq!(s.sql_values_joined(", "), "'active', 'O''Brien'");
    ///
    /// let i = EnumValues::Integer(vec![
    ///     NumValue { name: "low".into(),  value: 0  },
    ///     NumValue { name: "high".into(), value: 10 },
    /// ]);
    /// assert_eq!(i.sql_values_joined(", "), "0, 10");
    ///
    /// let empty = EnumValues::String(vec![]);
    /// assert_eq!(empty.sql_values_joined(", "), "");
    /// ```
    #[must_use]
    pub fn sql_values_joined(&self, separator: &str) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        match self {
            EnumValues::String(values) => {
                for (i, s) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(separator);
                    }
                    out.push('\'');
                    // Centralized '' escape — matches the existing
                    // `format!("'{}'", s.replace('\'', "''"))` byte-for-byte,
                    // and borrows when no quote is present (zero alloc).
                    out.push_str(&crate::escape_sql_string_literal(s));
                    out.push('\'');
                }
            }
            EnumValues::Integer(values) => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(separator);
                    }
                    // i64 Display into String is infallible.
                    write!(out, "{}", v.value).expect("writing an i64 to a String never fails");
                }
            }
        }
        out
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

    /// Wire-format spelling of the complex type for user-facing
    /// diagnostics: `varchar(32)`, `char(2)`, `numeric(10, 2)`,
    /// `custom(TSVECTOR)`, `enum(status)`. See
    /// [`ColumnType::display_label`].
    #[must_use]
    pub fn display_label(&self) -> String {
        match self {
            ComplexColumnType::Varchar { length } => format!("varchar({length})"),
            ComplexColumnType::Char { length } => format!("char({length})"),
            ComplexColumnType::Numeric { precision, scale } => {
                format!("numeric({precision}, {scale})")
            }
            ComplexColumnType::Custom { custom_type } => format!("custom({custom_type})"),
            ComplexColumnType::Enum { name, .. } => format!("enum({name})"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// `display_label` is the wire-format spelling shown in LSP drift messages,
    /// planner type-mismatch diagnostics and schema violations. Every consumer
    /// only forwards the string, so without a direct assertion the whole body
    /// could return a constant and nothing would notice.
    #[rstest]
    #[case::varchar(ComplexColumnType::Varchar { length: 32 }, "varchar(32)")]
    #[case::char(ComplexColumnType::Char { length: 2 }, "char(2)")]
    #[case::numeric(ComplexColumnType::Numeric { precision: 10, scale: 2 }, "numeric(10, 2)")]
    #[case::custom(ComplexColumnType::Custom { custom_type: "TSVECTOR".into() }, "custom(TSVECTOR)")]
    #[case::enum_type(
        ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into()]),
        },
        "enum(status)"
    )]
    fn complex_display_label_renders_wire_format(
        #[case] complex: ComplexColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(complex.display_label(), expected);
        // The `ColumnType` wrapper must forward to the same string rather than
        // rendering its own spelling.
        assert_eq!(ColumnType::Complex(complex).display_label(), expected);
    }

    #[rstest]
    #[case::integer(SimpleColumnType::Integer, "integer")]
    #[case::big_int(SimpleColumnType::BigInt, "big_int")]
    #[case::timestamptz(SimpleColumnType::Timestamptz, "timestamptz")]
    fn simple_display_label_uses_the_model_name(
        #[case] simple: SimpleColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(ColumnType::Simple(simple).display_label(), expected);
    }
}
