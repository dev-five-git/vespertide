use serde::{Deserialize, Serialize};

/// Supported file formats for generated artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum FileFormat {
    #[default]
    Json,
    Yaml,
    Yml,
}

impl FileFormat {
    /// File extension (without the leading dot) for artifacts in this format.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Yml => "yml",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FileFormat;

    #[test]
    fn default_is_json() {
        assert_eq!(FileFormat::default(), FileFormat::Json);
    }

    #[test]
    fn extension_matches_serde_wire_name() {
        assert_eq!(FileFormat::Json.extension(), "json");
        assert_eq!(FileFormat::Yaml.extension(), "yaml");
        assert_eq!(FileFormat::Yml.extension(), "yml");
    }
}
