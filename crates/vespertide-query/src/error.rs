use thiserror::Error;

use crate::sql::types::DatabaseBackend;

/// Errors returned by SQL query generation.
///
/// `QueryError` is `#[non_exhaustive]`, so match arms must include a wildcard.
///
/// Matching variants:
///
/// ```rust
/// use vespertide_query::{QueryError, DatabaseBackend};
///
/// fn report(err: &QueryError) -> String {
///     match err {
///         QueryError::SchemaError(msg) => format!("schema: {msg}"),
///         QueryError::InvalidColumnType { backend, message } => {
///             format!("type error for {backend:?}: {message}")
///         }
///         _ => format!("other: {err}"),
///     }
/// }
///
/// let schema_err = QueryError::SchemaError("missing table user".into());
/// assert_eq!(report(&schema_err), "schema: missing table user");
///
/// let type_err = QueryError::InvalidColumnType {
///     backend: DatabaseBackend::Postgres,
///     message: "inet not supported".into(),
/// };
/// assert!(report(&type_err).contains("Postgres"));
/// ```
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum QueryError {
    /// The SQL backend doesn't support the requested table constraint variant.
    #[error("unsupported table constraint")]
    UnsupportedConstraint,

    /// A column's type cannot be mapped to the target SQL backend.
    ///
    /// Constructor with named fields:
    ///
    /// ```rust
    /// use vespertide_query::{QueryError, DatabaseBackend};
    ///
    /// let err = QueryError::InvalidColumnType {
    ///     backend: DatabaseBackend::MySql,
    ///     message: "inet is not supported by MySQL".into(),
    /// };
    /// assert!(err.to_string().contains("MySql"));
    /// assert!(err.to_string().contains("inet is not supported by MySQL"));
    /// ```
    #[error("invalid column type for {backend:?}: {message}")]
    InvalidColumnType {
        backend: DatabaseBackend,
        message: String,
    },

    /// A schema validation issue surfaced during query generation
    /// (e.g., missing referenced table, malformed constraint).
    ///
    /// Constructor and `Display`:
    ///
    /// ```rust
    /// use vespertide_query::QueryError;
    ///
    /// let err = QueryError::SchemaError("referenced table 'user' not found".into());
    /// assert_eq!(err.to_string(), "schema error: referenced table 'user' not found");
    /// ```
    #[error("schema error: {0}")]
    SchemaError(String),

    /// Backend-specific SQL generation failed.
    #[error("backend error ({backend:?}): {message}")]
    BackendError {
        backend: DatabaseBackend,
        message: String,
    },

    /// The requested `MigrationAction` variant isn't yet supported by query generation.
    #[error("unsupported action: {0}")]
    UnsupportedAction(String),

    /// Fallback variant for errors that don't fit a specific category yet.
    /// Will be removed in a future major version — prefer specific variants when possible.
    #[deprecated(
        since = "0.2.0",
        note = "Match a specific variant (InvalidColumnType, SchemaError, BackendError, UnsupportedAction)"
    )]
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_column_type_message_includes_backend() {
        let err = QueryError::InvalidColumnType {
            backend: DatabaseBackend::Postgres,
            message: "unknown type: ZZZ".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Postgres"));
        assert!(msg.contains("ZZZ"));
    }

    #[test]
    fn schema_error_displays_message() {
        let err = QueryError::SchemaError("missing table foo".into());
        assert_eq!(err.to_string(), "schema error: missing table foo");
    }

    #[test]
    fn errors_can_be_cloned() {
        let original = QueryError::UnsupportedConstraint;
        let _clone = original.clone();
    }

    #[test]
    #[expect(
        deprecated,
        reason = "0.2.0 G.3 explicit backward-compat test for deprecated QueryError::Other variant"
    )]
    fn other_variant_still_constructable_for_backward_compat() {
        let err = QueryError::Other("legacy".into());
        assert_eq!(err.to_string(), "legacy");
    }
}
