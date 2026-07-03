use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::file_format::FileFormat;
use crate::name_case::NameCase;

/// Default migration filename pattern: zero-padded version + sanitized comment.
pub fn default_migration_filename_pattern() -> String {
    "%04v_%m".to_string()
}

/// SeaORM-specific export configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SeaOrmConfig {
    /// Additional derive macros to add to generated enum types.
    /// Default: `["vespera::Schema"]`
    #[serde(default = "default_extra_enum_derives")]
    pub extra_enum_derives: Vec<String>,
    /// Additional derive macros to add to generated entity model types.
    #[serde(default)]
    pub extra_model_derives: Vec<String>,
    /// Naming case for serde `rename_all` attribute on generated enums.
    /// Default: `Camel` (generates `#[serde(rename_all = "camelCase")]`)
    #[serde(default = "default_enum_naming_case")]
    pub enum_naming_case: NameCase,
    /// Generate `vespera::schema_type!` macro invocation for each entity.
    /// Default: `true`
    #[serde(default = "default_vespera_schema_type")]
    pub vespera_schema_type: bool,
}

fn default_extra_enum_derives() -> Vec<String> {
    vec!["vespera::Schema".to_string()]
}

fn default_enum_naming_case() -> NameCase {
    NameCase::Camel
}

fn default_vespera_schema_type() -> bool {
    true
}

impl Default for SeaOrmConfig {
    fn default() -> Self {
        Self {
            extra_enum_derives: default_extra_enum_derives(),
            extra_model_derives: Vec::new(),
            enum_naming_case: default_enum_naming_case(),
            vespera_schema_type: default_vespera_schema_type(),
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

    /// Get the naming case for serde `rename_all` attribute on generated enums.
    pub fn enum_naming_case(&self) -> NameCase {
        self.enum_naming_case
    }

    /// Whether to generate `vespera::schema_type!` macro invocation for each entity.
    pub fn vespera_schema_type(&self) -> bool {
        self.vespera_schema_type
    }
}

/// Top-level vespertide configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
    /// Maximum time (milliseconds) to wait acquiring a lock during a runtime
    /// migration before failing. When set, the `vespertide_migration!` macro
    /// emits a backend-appropriate session/connection timeout at the start of
    /// the migration (`PostgreSQL` `lock_timeout`, `MySQL`
    /// `innodb_lock_wait_timeout`, `SQLite` `PRAGMA busy_timeout`). `None`
    /// (default) leaves backend defaults untouched. Absent from serialized
    /// JSON when `None` (wire-compatible).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_timeout_ms: Option<u64>,
    /// Maximum time (milliseconds) a single migration statement may run before
    /// the backend aborts it. When set, the macro emits `PostgreSQL`
    /// `statement_timeout` / `MySQL` `max_execution_time`. `SQLite` has no
    /// statement timeout, so this is skipped there. `None` (default) leaves
    /// backend defaults untouched. Absent from serialized JSON when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_timeout_ms: Option<u64>,
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
            lock_timeout_ms: None,
            statement_timeout_ms: None,
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

    /// Lock-acquisition timeout (ms) for runtime migrations, if configured.
    pub fn lock_timeout_ms(&self) -> Option<u64> {
        self.lock_timeout_ms
    }

    /// Per-statement timeout (ms) for runtime migrations, if configured.
    pub fn statement_timeout_ms(&self) -> Option<u64> {
        self.statement_timeout_ms
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
        assert!(config.seaorm.vespera_schema_type);
        assert_eq!(config.prefix, "");
        assert_eq!(config.lock_timeout_ms, None);
        assert_eq!(config.statement_timeout_ms, None);
    }

    #[test]
    fn timeout_fields_absent_from_json_when_none() {
        let config = VespertideConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("lockTimeoutMs"),
            "None lock_timeout_ms must not serialize (wire-compat): {json}"
        );
        assert!(
            !json.contains("statementTimeoutMs"),
            "None statement_timeout_ms must not serialize (wire-compat): {json}"
        );
    }

    #[test]
    fn timeout_fields_roundtrip_when_set() {
        let config = VespertideConfig {
            lock_timeout_ms: Some(5000),
            statement_timeout_ms: Some(30000),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"lockTimeoutMs\":5000"));
        assert!(json.contains("\"statementTimeoutMs\":30000"));
        let back: VespertideConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.lock_timeout_ms(), Some(5000));
        assert_eq!(back.statement_timeout_ms(), Some(30000));
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
