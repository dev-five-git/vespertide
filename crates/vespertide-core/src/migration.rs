/// Runtime options controlling how Vespertide tracks applied migrations.
///
/// Pass this to the migration runner to configure the version-tracking table name.
/// The default table name used by the `vespertide_migration!` macro is `"vespertide_migrations"`.
///
/// `MigrationOptions` is `#[non_exhaustive]`, so external callers must construct
/// via `MigrationOptions::new()` or `Default::default()` rather than struct literals.
///
/// Construction:
///
/// ```rust
/// use vespertide_core::MigrationOptions;
///
/// // Named constructor — preferred when overriding the version table.
/// let opts = MigrationOptions::new("app_migrations");
/// assert_eq!(opts.version_table, "app_migrations");
///
/// // Default gives the standard tracking table name.
/// let default_opts = MigrationOptions::default();
/// assert_eq!(default_opts.version_table, "vespertide_migrations");
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MigrationOptions {
    /// Name of the table used to record which migration versions have been applied.
    ///
    /// Defaults to `"vespertide_migrations"`. Override this when multiple Vespertide-managed
    /// schemas share the same database and need separate version tables.
    pub version_table: String,
}

impl MigrationOptions {
    /// Create a new `MigrationOptions` with the specified version table name.
    ///
    /// Construction via `new`:
    ///
    /// ```rust
    /// use vespertide_core::MigrationOptions;
    ///
    /// let opts = MigrationOptions::new("tenant_migrations");
    /// assert_eq!(opts.version_table, "tenant_migrations");
    /// ```
    #[must_use]
    pub fn new(version_table: impl Into<String>) -> Self {
        Self {
            version_table: version_table.into(),
        }
    }
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            version_table: "vespertide_migrations".to_string(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    #[error("migration execution is not yet implemented")]
    NotImplemented,
    #[error("database error: {0}")]
    #[deprecated(
        since = "0.1.62",
        note = "Use Database { message, source } for proper error source chains"
    )]
    DatabaseError(String),
    #[error("database error: {message}")]
    Database {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    #[error(
        "migration id mismatch for version {version}: expected '{expected}', found '{found}' in database"
    )]
    IdMismatch {
        version: u32,
        expected: String,
        found: String,
    },
}

impl From<sea_orm::DbErr> for MigrationError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Database {
            message: err.to_string(),
            source: Some(Box::new(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for `MigrationOptions::new` + `Default` + `MigrationError`.
    //! Targets `uncovered-detail.json` lines 44, 45 (new) and 50, 51 (Default).
    use super::*;

    #[test]
    fn new_constructs_with_override_table_name() {
        // Covers lines 44, 45 (MigrationOptions::new body).
        let opts = MigrationOptions::new("tenant_migrations");
        assert_eq!(opts.version_table, "tenant_migrations");
    }

    #[test]
    fn new_accepts_owned_string_via_into() {
        // Re-exercises the `impl Into<String>` path on lines 44, 45.
        let owned: String = String::from("audit_migrations");
        let opts = MigrationOptions::new(owned);
        assert_eq!(opts.version_table, "audit_migrations");
    }

    #[test]
    fn default_uses_canonical_version_table() {
        // Covers lines 50, 51 (Default::default body).
        let opts = MigrationOptions::default();
        assert_eq!(opts.version_table, "vespertide_migrations");
    }

    #[test]
    fn migration_error_not_implemented_display() {
        let err = MigrationError::NotImplemented;
        assert_eq!(
            err.to_string(),
            "migration execution is not yet implemented"
        );
    }

    #[test]
    fn migration_error_database_struct_display() {
        let err = MigrationError::Database {
            message: "connection refused".to_string(),
            source: None,
        };
        assert_eq!(err.to_string(), "database error: connection refused");
    }

    #[test]
    fn migration_error_id_mismatch_display() {
        let err = MigrationError::IdMismatch {
            version: 7,
            expected: "abc".to_string(),
            found: "def".to_string(),
        };
        assert!(err.to_string().contains("version 7"));
        assert!(err.to_string().contains("'abc'"));
        assert!(err.to_string().contains("'def'"));
    }
}
