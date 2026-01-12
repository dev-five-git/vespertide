use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Supported naming cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NameCase {
    Snake,
    Camel,
    Pascal,
}

impl NameCase {
    /// Returns true when snake case.
    pub fn is_snake(self) -> bool {
        matches!(self, NameCase::Snake)
    }

    /// Returns true when camel case.
    pub fn is_camel(self) -> bool {
        matches!(self, NameCase::Camel)
    }

    /// Returns true when pascal case.
    pub fn is_pascal(self) -> bool {
        matches!(self, NameCase::Pascal)
    }

    /// Returns the serde rename_all attribute value for this case.
    pub fn serde_rename_all(self) -> &'static str {
        match self {
            NameCase::Snake => "snake_case",
            NameCase::Camel => "camelCase",
            NameCase::Pascal => "PascalCase",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_rename_all() {
        assert_eq!(NameCase::Snake.serde_rename_all(), "snake_case");
        assert_eq!(NameCase::Camel.serde_rename_all(), "camelCase");
        assert_eq!(NameCase::Pascal.serde_rename_all(), "PascalCase");
    }
}
