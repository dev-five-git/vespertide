use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::file_format::FileFormat;
use crate::name_case::NameCase;

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

