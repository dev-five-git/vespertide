use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum StrOrBoolOrArray {
    Str(String),
    Array(Vec<String>),
    Bool(bool),
}

/// A value that can be either a string or a boolean.
/// This is used for default values where boolean columns can use `true`/`false` directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StringOrBool {
    Bool(bool),
    String(String),
}

impl StringOrBool {
    /// Convert to SQL string representation
    pub fn to_sql(&self) -> String {
        match self {
            StringOrBool::Bool(b) => b.to_string(),
            StringOrBool::String(s) => s.clone(),
        }
    }
}

impl From<bool> for StringOrBool {
    fn from(b: bool) -> Self {
        StringOrBool::Bool(b)
    }
}

impl From<String> for StringOrBool {
    fn from(s: String) -> Self {
        StringOrBool::String(s)
    }
}

impl From<&str> for StringOrBool {
    fn from(s: &str) -> Self {
        StringOrBool::String(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_or_bool_to_sql_bool() {
        let val = StringOrBool::Bool(true);
        assert_eq!(val.to_sql(), "true");

        let val = StringOrBool::Bool(false);
        assert_eq!(val.to_sql(), "false");
    }

    #[test]
    fn test_string_or_bool_to_sql_string() {
        let val = StringOrBool::String("hello".into());
        assert_eq!(val.to_sql(), "hello");
    }

    #[test]
    fn test_string_or_bool_from_bool() {
        let val: StringOrBool = true.into();
        assert_eq!(val, StringOrBool::Bool(true));

        let val: StringOrBool = false.into();
        assert_eq!(val, StringOrBool::Bool(false));
    }

    #[test]
    fn test_string_or_bool_from_string() {
        let val: StringOrBool = String::from("test").into();
        assert_eq!(val, StringOrBool::String("test".into()));
    }

    #[test]
    fn test_string_or_bool_from_str() {
        let val: StringOrBool = "test".into();
        assert_eq!(val, StringOrBool::String("test".into()));
    }
}
