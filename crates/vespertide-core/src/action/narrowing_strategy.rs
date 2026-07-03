//! Strategy applied to existing rows whose value would violate a narrowed
//! column type, so the migration succeeds on every supported backend.
//!
//! Attached to `MigrationAction::ModifyColumnType` as
//! `narrowing_strategy: Option<NarrowingStrategy>`. When `None`, the SQL
//! generator emits a plain `ALTER COLUMN TYPE` — which is safe *only if*
//! the user has independently verified no row violates the new type
//! (typically caught by the `vespertide diff` / `vespertide revision`
//! warnings).
//!
//! When `Some`, the SQL generator emits a pre-processing statement
//! (`UPDATE` / `DELETE`) immediately before the `ALTER`, transforming
//! violating rows so the type change cannot fail.

use serde::{Deserialize, Serialize};

/// How an existing row that violates the new (narrowed) column type should
/// be transformed *before* the `ALTER COLUMN TYPE` statement runs.
///
/// The wire format uses an internally tagged JSON representation
/// (`{"kind": "..."}`) so future variants stay backwards compatible. The
/// enum is `#[non_exhaustive]` — downstream `match` expressions must
/// include a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "kind")]
#[non_exhaustive]
pub enum NarrowingStrategy {
    /// Trim the violating value to fit the new type. The row is preserved;
    /// only the column value loses its overflowing tail.
    ///
    /// Applicable to *string-like* narrowings (`varchar(N)`, `char(N)`,
    /// `text -> varchar(N)`) via `LEFT(col, N)` / `substr(col, 1, N)`, and
    /// to `NUMERIC` scale narrowing via `ROUND(col, new_scale)`.
    ///
    /// **Not** applicable to integer / float / timezone narrowings —
    /// truncation has no natural definition there. The CLI revision UI
    /// hides this option for those cases.
    Truncate,

    /// Delete the entire row containing a violating value. Other columns of
    /// that row are lost along with it.
    ///
    /// Universally applicable across every narrowing kind. Watch for FK
    /// cascade behaviour — deleting a parent row with `ON DELETE CASCADE`
    /// can propagate into child tables.
    Delete,

    /// Replace the violating value with a fixed sentinel that fits the new
    /// type. The row is preserved; only the violating column is rewritten.
    ///
    /// The `value` field is emitted verbatim into the generated SQL
    /// (`UPDATE ... SET col = <value>`), so callers must quote string
    /// literals themselves (e.g. `"'TRUNCATED'"`, not `"TRUNCATED"`) and
    /// must ensure the value itself fits the new type — otherwise the
    /// migration will fail in a different way.
    ///
    /// Universally applicable across every narrowing kind, including
    /// integer overflow (e.g. `value: "0"`) and timezone loss
    /// (e.g. `value: "(now() AT TIME ZONE 'UTC')"`).
    SetToValue {
        /// SQL fragment substituted for violating values. Strings must be
        /// pre-quoted by the caller. The CLI revision prompt wraps user
        /// input with single quotes automatically when the new type is a
        /// string-like type.
        value: String,
    },
}

impl NarrowingStrategy {
    /// Tag name used in JSON wire format and CLI output (`truncate`,
    /// `delete`, `set_to_value`). `#[non_exhaustive]` is enforced at
    /// downstream-crate boundaries; this in-crate match is exhaustive.
    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        match self {
            NarrowingStrategy::Truncate => "truncate",
            NarrowingStrategy::Delete => "delete",
            NarrowingStrategy::SetToValue { .. } => "set_to_value",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_roundtrips_through_json_with_kind_tag() {
        let s = NarrowingStrategy::Truncate;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"truncate"}"#);
        let back: NarrowingStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn delete_roundtrips_through_json_with_kind_tag() {
        let s = NarrowingStrategy::Delete;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"delete"}"#);
        let back: NarrowingStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn set_to_value_roundtrips_with_value_field() {
        let s = NarrowingStrategy::SetToValue {
            value: "'TRUNCATED'".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"set_to_value","value":"'TRUNCATED'"}"#);
        let back: NarrowingStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn kind_label_returns_snake_case_tag() {
        assert_eq!(NarrowingStrategy::Truncate.kind_label(), "truncate");
        assert_eq!(NarrowingStrategy::Delete.kind_label(), "delete");
        assert_eq!(
            NarrowingStrategy::SetToValue { value: "0".into() }.kind_label(),
            "set_to_value"
        );
    }
}
