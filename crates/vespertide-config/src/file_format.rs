use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Supported file formats for generated artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    Json,
    Yaml,
    Yml,
}

impl Default for FileFormat {
    fn default() -> Self {
        FileFormat::Json
    }
}

