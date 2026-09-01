use serde::{Deserialize, Serialize};

/// Supported naming cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NameCase {
    Snake,
    Camel,
    Pascal,
}

impl NameCase {
    /// Returns the serde `rename_all` attribute value for this case.
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
