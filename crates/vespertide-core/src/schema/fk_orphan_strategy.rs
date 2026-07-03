//! Strategy for pre-existing orphan rows when adding a `FOREIGN KEY` to an
//! existing column.
//!
//! `AddConstraint(ForeignKey)` against a populated column would fail at
//! apply time when child rows reference a value that does not exist in
//! the parent table. Vespertide treats this as a fault the user must
//! resolve explicitly: every revision that adds a `FOREIGN KEY` to an
//! existing column emits a pre-cleanup SQL statement ahead of the
//! `ADD CONSTRAINT`, either NULL-ing out the offending references
//! (`NullifyOrphans`) or deleting the offending rows (`DeleteOrphans`).
//!
//! There is no "skip cleanup" / "fail" option ? letting the database
//! reject the migration is incompatible with vespertide's safety
//! promise. Users who *know* their data is clean still get the same
//! SQL: the pre-cleanup statement is a no-op on a table with no
//! orphans.
//!
//! `#[non_exhaustive]` so additional strategies can be added in a
//! future minor release without breaking downstream `match`es.

use serde::{Deserialize, Serialize};

/// How `AddConstraint(ForeignKey)` should handle pre-existing orphan
/// rows ? child rows whose FK column(s) reference a value that does
/// not exist in the parent table.
///
/// "Orphan" is computed via the standard SQL `NOT EXISTS` correlated
/// subquery against the parent table; see
/// `vespertide-query::sql::add_constraint::foreign_key` for the emitted
/// SQL pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ForeignKeyOrphanStrategy {
    /// Set the FK column(s) of orphan child rows to `NULL` ahead of the
    /// `ADD CONSTRAINT`. The row is preserved; only the dangling
    /// reference is cleared.
    ///
    /// Only applicable when **every** column of the FK is nullable; the
    /// SQL generator falls back to a runtime error if the user picks
    /// this strategy on a NOT NULL column set.
    NullifyOrphans,
    /// Delete the orphan child rows outright ahead of the
    /// `ADD CONSTRAINT`. Use when the orphan rows are themselves
    /// invalid (logically dangling records).
    DeleteOrphans,
}

impl Default for ForeignKeyOrphanStrategy {
    /// Default strategy is `NullifyOrphans`: the less destructive
    /// choice when the SQL generator can satisfy it (i.e. when every FK
    /// column is nullable). The revision CLI re-prompts the user for an
    /// explicit choice regardless of this default; the default exists
    /// only so the JSON wire format can omit the `orphan_strategy`
    /// field for backwards compatibility with v0.1.x model files.
    ///
    /// **Wire-format note.** v0.1.x emitted no pre-cleanup at all and
    /// let the database reject the migration. v0.2 emits a pre-cleanup
    /// statement automatically. Existing migrations that *already
    /// applied* under v0.1.x are unaffected (apply happens once);
    /// v0.1.x migrations *re-run* against a fresh DB will now NULL out
    /// orphan references instead of failing.
    fn default() -> Self {
        Self::NullifyOrphans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_nullify_orphans() {
        assert_eq!(
            crate::ForeignKeyOrphanStrategy::default(),
            ForeignKeyOrphanStrategy::NullifyOrphans
        );
    }

    #[test]
    fn serde_roundtrip_nullify() {
        let s = ForeignKeyOrphanStrategy::NullifyOrphans;
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"nullify_orphans"}"#);
        let back: ForeignKeyOrphanStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn serde_roundtrip_delete() {
        let s = ForeignKeyOrphanStrategy::DeleteOrphans;
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"delete_orphans"}"#);
        let back: ForeignKeyOrphanStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}
