use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use clap::ValueEnum;

/// Supported naming cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Default migration filename pattern: zero-padded version + sanitized comment.
pub fn default_migration_filename_pattern() -> String {
    "%04v_%m".to_string()
}

/// Top-level vespertide configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VespertideConfig {
    pub models_dir: PathBuf,
    pub migrations_dir: PathBuf,
    pub table_naming_case: NameCase,
    pub column_naming_case: NameCase,
    #[serde(default)]
    pub model_format: FileFormat,
    #[serde(default)]
    pub migration_format: FileFormat,
    #[serde(default = "default_migration_filename_pattern")]
    pub migration_filename_pattern: String,
}

impl Default for VespertideConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("models"),
            migrations_dir: PathBuf::from("migrations"),
            table_naming_case: NameCase::Snake,
            column_naming_case: NameCase::Snake,
            model_format: FileFormat::Json,
            migration_format: FileFormat::Json,
            migration_filename_pattern: default_migration_filename_pattern(),
        }
    }
}

impl VespertideConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Path where model definitions are stored.
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Path where migrations are stored.
    pub fn migrations_dir(&self) -> &Path {
        &self.migrations_dir
    }

    /// Naming case for table names (flattened).
    pub fn table_case(&self) -> NameCase {
        self.table_naming_case
    }

    /// Naming case for column names (flattened).
    pub fn column_case(&self) -> NameCase {
        self.column_naming_case
    }

    /// Preferred file format for models.
    pub fn model_format(&self) -> FileFormat {
        self.model_format
    }

    /// Preferred file format for migrations.
    pub fn migration_format(&self) -> FileFormat {
        self.migration_format
    }

    /// Pattern for migration filenames (supports %v and %m placeholders).
    pub fn migration_filename_pattern(&self) -> &str {
        &self.migration_filename_pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_snake_and_standard_paths() {
        let cfg = VespertideConfig::default();
        assert_eq!(cfg.models_dir, PathBuf::from("models"));
        assert_eq!(cfg.migrations_dir, PathBuf::from("migrations"));
        assert!(cfg.table_case().is_snake());
        assert!(cfg.column_case().is_snake());
    }

    #[test]
    fn overrides_work_via_struct_update() {
        let cfg = VespertideConfig {
            models_dir: PathBuf::from("custom_models"),
            migrations_dir: PathBuf::from("custom_migrations"),
            table_naming_case: NameCase::Camel,
            column_naming_case: NameCase::Pascal,
            ..Default::default()
        };

        assert_eq!(cfg.models_dir(), Path::new("custom_models"));
        assert_eq!(cfg.migrations_dir(), Path::new("custom_migrations"));
        assert!(cfg.table_case().is_camel());
        assert!(cfg.column_case().is_pascal());
    }
}
