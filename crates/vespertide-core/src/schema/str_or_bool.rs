use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum StrOrBoolOrArray {
    Str(String),
    Array(Vec<String>),
    Bool(bool),
}

/// A value that can be a string, boolean, or number.
/// This is used for default values where columns can use literal values directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DefaultValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

impl Eq for DefaultValue {}

impl DefaultValue {
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
        DefaultValue::Integer(n as i64)
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
        let val = DefaultValue::String("".into());
        assert_eq!(val.to_sql(), "''");
    }

    #[test]
    fn test_default_value_is_empty_string() {
        assert!(DefaultValue::String("".into()).is_empty_string());
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
}
