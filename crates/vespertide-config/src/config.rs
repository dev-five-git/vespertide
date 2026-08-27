use std::borrow::Cow;
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

/// Django-specific export configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DjangoConfig {
    /// Explicit `app_label` written into every generated model's `Meta`
    /// class. Needed when generated models don't live inside a standard
    /// Django app package layout, where Django would otherwise infer the
    /// label from the containing package. `None` (default) omits
    /// `app_label` and leaves Django's normal inference in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_label: Option<String>,
}

impl DjangoConfig {
    /// Explicit `app_label` to emit in every model's `Meta` class, if set.
    pub fn app_label(&self) -> Option<&str> {
        self.app_label.as_deref()
    }
}

/// GORM-specific export configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct GormConfig {
    /// Go package name emitted at the top of every generated file
    /// (`package <name>`). `None` (default) infers the name from the
    /// export directory's final path segment (sanitized to a valid Go
    /// identifier), falling back to `"models"` when that segment isn't
    /// usable. See [`VespertideConfig::gorm_package_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
}

impl GormConfig {
    /// Explicit Go package name from config, if set. Prefer
    /// [`VespertideConfig::gorm_package_name`] to resolve the effective
    /// name (this accessor doesn't apply the folder-based inference).
    pub fn package_name(&self) -> Option<&str> {
        self.package_name.as_deref()
    }
}

/// Fallback Go package name used when neither an explicit `gorm.package_name`
/// nor a usable export directory name is available.
pub const DEFAULT_GORM_PACKAGE_NAME: &str = "models";

/// Go reserved words, which can't be used as a package name.
const GO_RESERVED_WORDS: &[&str] = &[
    "break",
    "default",
    "func",
    "interface",
    "select",
    "case",
    "defer",
    "go",
    "map",
    "struct",
    "chan",
    "else",
    "goto",
    "package",
    "switch",
    "const",
    "fallthrough",
    "if",
    "range",
    "type",
    "continue",
    "for",
    "import",
    "return",
    "var",
];

/// Sanitize a candidate string into a valid, idiomatic Go package identifier:
/// lowercase ASCII letters/digits only, must not start with a digit, must
/// not collide with a Go reserved word. Returns `None` when nothing usable
/// remains (e.g. an all-Unicode or empty candidate).
fn sanitize_go_package_name(candidate: &str) -> Option<String> {
    let cleaned: String = candidate
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();

    if cleaned.is_empty() || cleaned.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if GO_RESERVED_WORDS.contains(&cleaned.as_str()) {
        return None;
    }
    Some(cleaned)
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
    /// Django-specific export configuration.
    #[serde(default)]
    pub django: DjangoConfig,
    /// GORM-specific export configuration.
    #[serde(default)]
    pub gorm: GormConfig,
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
            django: DjangoConfig::default(),
            gorm: GormConfig::default(),
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

    /// Django-specific export configuration.
    pub fn django(&self) -> &DjangoConfig {
        &self.django
    }

    /// GORM-specific export configuration.
    pub fn gorm(&self) -> &GormConfig {
        &self.gorm
    }

    /// Effective Go package name for GORM export: an explicit
    /// `gorm.package_name` always wins; otherwise it's inferred from
    /// `export_dir`'s final path segment (sanitized to a valid Go
    /// identifier), falling back to [`DEFAULT_GORM_PACKAGE_NAME`] when that
    /// segment isn't usable (e.g. empty, digit-led, or non-ASCII).
    ///
    /// `export_dir` is the *actual* directory the `.go` files will be
    /// written to — normally `model_export_dir`, but callers must pass
    /// whatever directory wins after resolving CLI overrides (e.g.
    /// `vespertide export --export-dir <dir>`), since Go requires the
    /// `package` declaration to match the directory the files live in.
    pub fn gorm_package_name(&self, export_dir: &Path) -> Cow<'_, str> {
        if let Some(name) = &self.gorm.package_name {
            return Cow::Borrowed(name);
        }
        let inferred = export_dir
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(sanitize_go_package_name);
        Cow::Owned(inferred.unwrap_or_else(|| DEFAULT_GORM_PACKAGE_NAME.to_string()))
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
