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
}
