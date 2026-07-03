use serde::{Deserialize, Serialize};

/// A JSON value that can be a string, an array of strings, or a boolean.
///
/// Used for inline `"unique"` and `"index"` declarations on a [`ColumnDef`]:
/// - `true` / `false` — enable or disable the constraint with an auto-generated name.
/// - A single string — the constraint name (used to group columns into a composite constraint).
/// - An array of strings — a list of constraint names for multi-group membership.
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
///
/// [`ColumnDef`]: crate::schema::ColumnDef
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", untagged)]
#[non_exhaustive]
pub enum StrOrBoolOrArray {
    /// A named constraint or group identifier.
    Str(String),
    /// Multiple constraint group names for multi-group membership.
    Array(Vec<String>),
    /// `true` to enable with an auto-generated name; `false` to disable.
    Bool(bool),
}

impl StrOrBoolOrArray {
    /// Returns the string value when this is `Str`.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            StrOrBoolOrArray::Str(s) => Some(s.as_str()),
            StrOrBoolOrArray::Array(_) | StrOrBoolOrArray::Bool(_) => None,
        }
    }
}

/// A column default value that can be a boolean, integer, float, or SQL expression string.
///
/// In JSON model files the `"default"` field accepts any of these forms:
/// - `true` / `false` — boolean literal.
/// - `0`, `42` — integer literal.
/// - `0.0`, `1.5` — floating-point literal.
/// - `"'pending'"` — SQL string literal (note the inner single quotes).
/// - `"NOW()"` — SQL function call (no surrounding quotes).
///
/// Use [`DefaultValue::to_sql`] to convert to the SQL representation for DDL generation.
///
/// `StringOrBool` is a backwards-compatibility alias for this type.
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
#[non_exhaustive]
pub enum DefaultValue {
    /// A boolean default (`true` or `false`).
    Bool(bool),
    /// An integer default value.
    Integer(i64),
    /// A floating-point default value.
    Float(f64),
    /// A SQL expression or string literal default (e.g. `"'pending'"` or `"NOW()"`).
    String(String),
}

impl Eq for DefaultValue {}

impl DefaultValue {
    /// Returns the boolean value when this is `Bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            DefaultValue::Bool(b) => Some(*b),
            DefaultValue::Integer(_) | DefaultValue::Float(_) | DefaultValue::String(_) => None,
        }
    }

    /// Convert to SQL string representation
    /// Empty strings are converted to '' (SQL empty string literal)
    pub fn to_sql(&self) -> String {
        match self {
            DefaultValue::Bool(b) => b.to_string(),
            DefaultValue::Integer(n) => n.to_string(),
            DefaultValue::Float(f) => f.to_string(),
            DefaultValue::String(s) => {
                if s.is_empty() {
                    "''".to_string()
                } else {
                    s.clone()
                }
            }
        }
    }

    /// Check if this is a string type (needs quoting for certain column types)
    pub fn is_string(&self) -> bool {
        matches!(self, DefaultValue::String(_))
    }

    /// Check if this is an empty string
    pub fn is_empty_string(&self) -> bool {
        matches!(self, DefaultValue::String(s) if s.is_empty())
    }
}

impl From<bool> for DefaultValue {
    fn from(b: bool) -> Self {
        DefaultValue::Bool(b)
    }
}

impl From<i64> for DefaultValue {
    fn from(n: i64) -> Self {
        DefaultValue::Integer(n)
    }
}

impl From<i32> for DefaultValue {
    fn from(n: i32) -> Self {
        DefaultValue::Integer(i64::from(n))
    }
}

impl From<f64> for DefaultValue {
    fn from(f: f64) -> Self {
        DefaultValue::Float(f)
    }
}

impl From<String> for DefaultValue {
    fn from(s: String) -> Self {
        DefaultValue::String(s)
    }
}

impl From<&str> for DefaultValue {
    fn from(s: &str) -> Self {
        DefaultValue::String(s.to_string())
    }
}

/// Backwards compatibility alias
pub type StringOrBool = DefaultValue;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_value_to_sql_bool() {
        let val = DefaultValue::Bool(true);
        assert_eq!(val.to_sql(), "true");

        let val = DefaultValue::Bool(false);
        assert_eq!(val.to_sql(), "false");
    }

    #[test]
    fn test_default_value_to_sql_integer() {
        let val = DefaultValue::Integer(42);
        assert_eq!(val.to_sql(), "42");

        let val = DefaultValue::Integer(-100);
        assert_eq!(val.to_sql(), "-100");
    }

    #[test]
    fn test_default_value_to_sql_float() {
        let val = DefaultValue::Float(1.5);
        assert_eq!(val.to_sql(), "1.5");
    }

    #[test]
    fn test_default_value_to_sql_string() {
        let val = DefaultValue::String("hello".into());
        assert_eq!(val.to_sql(), "hello");
    }

    #[test]
    fn test_default_value_to_sql_empty_string() {
        let val = DefaultValue::String(String::new());
        assert_eq!(val.to_sql(), "''");
    }

    #[test]
    fn test_default_value_is_empty_string() {
        assert!(DefaultValue::String(String::new()).is_empty_string());
        assert!(!DefaultValue::String("hello".into()).is_empty_string());
        assert!(!DefaultValue::Bool(true).is_empty_string());
        assert!(!DefaultValue::Integer(0).is_empty_string());
    }

    #[test]
    fn test_default_value_from_bool() {
        let val: DefaultValue = true.into();
        assert_eq!(val, DefaultValue::Bool(true));

        let val: DefaultValue = false.into();
        assert_eq!(val, DefaultValue::Bool(false));
    }

    #[test]
    fn test_default_value_from_integer() {
        let val: DefaultValue = 42i64.into();
        assert_eq!(val, DefaultValue::Integer(42));

        let val: DefaultValue = 100i32.into();
        assert_eq!(val, DefaultValue::Integer(100));
    }

    #[test]
    fn test_default_value_from_float() {
        let val: DefaultValue = 1.5f64.into();
        assert_eq!(val, DefaultValue::Float(1.5));
    }

    #[test]
    fn test_default_value_from_string() {
        let val: DefaultValue = String::from("test").into();
        assert_eq!(val, DefaultValue::String("test".into()));
    }

    #[test]
    fn test_default_value_from_str() {
        let val: DefaultValue = "test".into();
        assert_eq!(val, DefaultValue::String("test".into()));
    }

    #[test]
    fn test_default_value_is_string() {
        assert!(DefaultValue::String("test".into()).is_string());
        assert!(!DefaultValue::Bool(true).is_string());
        assert!(!DefaultValue::Integer(42).is_string());
        assert!(!DefaultValue::Float(1.5).is_string());
    }

    #[test]
    fn test_string_or_bool_as_bool() {
        assert_eq!(StringOrBool::Bool(true).as_bool(), Some(true));
        assert_eq!(StringOrBool::Bool(false).as_bool(), Some(false));
        assert_eq!(StringOrBool::String("value".into()).as_bool(), None);
    }

    #[test]
    fn test_str_or_bool_or_array_as_str() {
        assert_eq!(StrOrBoolOrArray::Str("name".into()).as_str(), Some("name"));
        assert_eq!(StrOrBoolOrArray::Bool(true).as_str(), None);
        assert_eq!(StrOrBoolOrArray::Array(vec!["name".into()]).as_str(), None);
    }
}
