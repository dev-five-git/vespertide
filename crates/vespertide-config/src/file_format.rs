use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Supported file formats for generated artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum FileFormat {
    #[default]
    Json,
    Yaml,
    Yml,
}

#[cfg(test)]
mod tests {
    use super::FileFormat;

    #[test]
    fn default_is_json() {
        assert_eq!(FileFormat::default(), FileFormat::Json);
    }
}
