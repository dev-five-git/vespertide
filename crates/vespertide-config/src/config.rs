use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::file_format::FileFormat;
use crate::name_case::NameCase;

/// Default migration filename pattern: zero-padded version + sanitized comment.
pub fn default_migration_filename_pattern() -> String {
    "%04v_%m".to_string()
}

/// SeaORM-specific export configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeaOrmConfig {
    /// Additional derive macros to add to generated enum types.
    /// Default: `["vespera::Schema"]`
    #[serde(default = "default_extra_enum_derives")]
    pub extra_enum_derives: Vec<String>,
    /// Additional derive macros to add to generated entity model types.
    #[serde(default)]
    pub extra_model_derives: Vec<String>,
    /// Naming case for serde rename_all attribute on generated enums.
    /// Default: `Camel` (generates `#[serde(rename_all = "camelCase")]`)
    #[serde(default = "default_enum_naming_case")]
    pub enum_naming_case: NameCase,
}

fn default_extra_enum_derives() -> Vec<String> {
    vec!["vespera::Schema".to_string()]
}

fn default_enum_naming_case() -> NameCase {
    NameCase::Camel
}

impl Default for SeaOrmConfig {
    fn default() -> Self {
        Self {
            extra_enum_derives: default_extra_enum_derives(),
            extra_model_derives: Vec::new(),
            enum_naming_case: default_enum_naming_case(),
        }
    }
}

impl SeaOrmConfig {
    /// Get the extra derive macros for enum types.
    pub fn extra_enum_derives(&self) -> &[String] {
        &self.extra_enum_derives
    }

    /// Get the extra derive macros for model types.
    pub fn extra_model_derives(&self) -> &[String] {
        &self.extra_model_derives
    }

    /// Get the naming case for serde rename_all attribute on generated enums.
    pub fn enum_naming_case(&self) -> NameCase {
        self.enum_naming_case
    }
}

/// Top-level vespertide configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// Output directory for generated ORM models.
    #[serde(default = "default_model_export_dir")]
    pub model_export_dir: PathBuf,
    /// SeaORM-specific export configuration.
    #[serde(default)]
    pub seaorm: SeaOrmConfig,
    /// Prefix to add to all table names (including migration version table).
    /// Default: "" (no prefix)
    #[serde(default)]
    pub prefix: String,
}

fn default_model_export_dir() -> PathBuf {
    PathBuf::from("src/models")
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
            model_export_dir: default_model_export_dir(),
            seaorm: SeaOrmConfig::default(),
            prefix: String::new(),
        }
    }
}

impl VespertideConfig {
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

    /// Output directory for generated ORM models.
    pub fn model_export_dir(&self) -> &Path {
        &self.model_export_dir
    }

    /// SeaORM-specific export configuration.
    pub fn seaorm(&self) -> &SeaOrmConfig {
        &self.seaorm
    }

    /// Prefix to add to all table names.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Apply prefix to a table name.
    pub fn apply_prefix(&self, table_name: &str) -> String {
        if self.prefix.is_empty() {
            table_name.to_string()
        } else {
            format!("{}{}", self.prefix, table_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vespertide_config_default() {
        let config = VespertideConfig::default();

        assert_eq!(config.models_dir, PathBuf::from("models"));
        assert_eq!(config.migrations_dir, PathBuf::from("migrations"));
        assert_eq!(config.table_naming_case, NameCase::Snake);
        assert_eq!(config.column_naming_case, NameCase::Snake);
        assert_eq!(config.model_format, FileFormat::Json);
        assert_eq!(config.migration_format, FileFormat::Json);
        assert_eq!(config.migration_filename_pattern, "%04v_%m");
        assert_eq!(config.model_export_dir, PathBuf::from("src/models"));
        assert_eq!(
            config.seaorm.extra_enum_derives,
            vec!["vespera::Schema".to_string()]
        );
        assert!(config.seaorm.extra_model_derives.is_empty());
        assert_eq!(config.prefix, "");
    }

    #[test]
    fn test_vespertide_config_prefix() {
        let config = VespertideConfig {
            prefix: "myapp_".to_string(),
            ..Default::default()
        };

        assert_eq!(config.prefix(), "myapp_");
        assert_eq!(config.apply_prefix("users"), "myapp_users");
        assert_eq!(config.apply_prefix("posts"), "myapp_posts");
    }

    #[test]
    fn test_vespertide_config_empty_prefix() {
        let config = VespertideConfig::default();

        assert_eq!(config.prefix(), "");
        assert_eq!(config.apply_prefix("users"), "users");
    }
}
