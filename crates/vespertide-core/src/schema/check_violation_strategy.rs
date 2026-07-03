//! Strategy for pre-existing rows that violate a `CHECK` constraint being
//! added to an existing table.
//!
//! `AddConstraint(Check)` against a populated column would fail at apply
//! time when at least one row violates the predicate. Vespertide treats
//! this as a fault the user must resolve explicitly: every revision that
//! adds a `CHECK` whose expression matches the narrow shape
//! `<col> <op> <literal>` or `<col> IN (...)` (see
//! [`crate::schema::TableConstraint::Check`] doc) emits a pre-cleanup SQL
//! statement ahead of the `ADD CONSTRAINT`, either NULL-ing the offending
//! column ([`NullifyViolatingColumn`]) or deleting the offending row
//! ([`DeleteViolatingRows`]).
//!
//! There is no "skip cleanup" / "fail" option ? letting the database
//! reject the migration is incompatible with vespertide's safety
//! promise. Users who *know* their data is clean still get the same
//! SQL: the pre-cleanup statement is a no-op on a clean table.
//!
//! `#[non_exhaustive]` so additional strategies can be added in a
//! future minor release without breaking downstream `match`es.
//!
//! [`NullifyViolatingColumn`]: CheckViolationStrategy::NullifyViolatingColumn
//! [`DeleteViolatingRows`]: CheckViolationStrategy::DeleteViolatingRows

use serde::{Deserialize, Serialize};

use crate::schema::names::ColumnName;

/// How `AddConstraint(Check)` should handle pre-existing rows that
/// violate the CHECK predicate.
///
/// "Violation" is computed statically by the narrow-shape parser in
/// `vespertide-planner::validate::check_default` and the cleanup SQL is
/// emitted by `vespertide-query::sql::add_constraint::check`. CHECK
/// expressions that exceed the narrow shape (function calls, AND/OR
/// composition, subqueries) are *not* covered by either side ? the
/// detector skips them and no pre-cleanup is emitted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckViolationStrategy {
    /// Set the named column of every violating row to `NULL` ahead of
    /// the `ADD CONSTRAINT`. The row is preserved; only the failing
    /// value is cleared.
    ///
    /// Only applicable when `column` is nullable in the baseline; the
    /// SQL generator emits a runtime SQL error otherwise (UPDATE ...
    /// SET col = NULL violates the existing NOT NULL). The revision
    /// CLI's narrow-shape parser identifies the target column
    /// automatically; the user rarely sets this directly in the model
    /// JSON.
    NullifyViolatingColumn {
        /// Which column receives `SET = NULL`. Narrow-shape CHECKs
        /// always reduce to a single column, so this is the only
        /// column the cleanup affects.
        column: ColumnName,
    },
    /// Delete the violating rows outright ahead of the
    /// `ADD CONSTRAINT`. Use when the violating rows are themselves
    /// invalid (logically dangling records).
    ///
    /// This is also the canonical default for the wire format: v0.1.x
    /// models carry no `strategy` field and so deserialize to
    /// `DeleteViolatingRows`. The revision CLI re-prompts for an
    /// explicit choice, and the prompt offers
    /// `NullifyViolatingColumn { column }` when the narrow-shape
    /// parser can identify a nullable target column.
    DeleteViolatingRows,
}

impl Default for CheckViolationStrategy {
    /// Default strategy is `DeleteViolatingRows`. Unlike F3
    /// (`NullifyOrphans`), F4 cannot default to the less destructive
    /// `NullifyViolatingColumn` because that variant requires a
    /// `column` argument the wire-format default cannot supply
    /// (CHECK expressions are free-form text and the column name has
    /// to be parsed out by `vespertide-planner`).
    ///
    /// **Wire-format note.** v0.1.x emitted no pre-cleanup at all and
    /// let the database reject the migration. v0.2 emits
    /// `DELETE FROM table WHERE NOT (<expr>)` automatically. Existing
    /// migrations that *already applied* under v0.1.x are unaffected
    /// (apply happens once); v0.1.x migrations *re-run* against a
    /// fresh DB will now drop violating rows instead of failing. The
    /// revision CLI prompts for an explicit choice on every new
    /// `AddConstraint(Check)`, so production usage rarely hits the
    /// default - it exists for v0.1.x compatibility only.
    fn default() -> Self {
        Self::DeleteViolatingRows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_delete_violating_rows() {
        assert_eq!(
            CheckViolationStrategy::default(),
            CheckViolationStrategy::DeleteViolatingRows
        );
    }

    #[test]
    fn serde_roundtrip_nullify() {
        let s = CheckViolationStrategy::NullifyViolatingColumn {
            column: "price".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"nullify_violating_column","column":"price"}"#);
        let back: CheckViolationStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn serde_roundtrip_delete() {
        let s = CheckViolationStrategy::DeleteViolatingRows;
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"delete_violating_rows"}"#);
        let back: CheckViolationStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}
