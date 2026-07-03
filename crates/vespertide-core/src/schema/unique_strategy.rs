//! Strategy for pre-existing duplicates when adding a UNIQUE constraint to
//! an existing column.
//!
//! `AddConstraint(Unique)` against a populated column would fail at apply
//! time when duplicate values exist. Vespertide treats this as a fault
//! the user must resolve explicitly: every revision that adds UNIQUE on
//! an existing column emits a `DELETE` ahead of the `ADD CONSTRAINT`,
//! keeping one row per group based on the strategy below.
//!
//! There is no "skip cleanup" option — relying on the database to reject
//! the migration is incompatible with vespertide's safety promise. Users
//! who *know* their data is clean still get the same SQL: `DELETE` on
//! a clean table is a no-op.
//!
//! `#[non_exhaustive]` so additional strategies (e.g. `KeepWithExpression`
//! when single-column-PK is insufficient) can be added in a future minor
//! release without breaking downstream `match`es.

use serde::{Deserialize, Serialize};

/// How `AddConstraint(Unique)` should handle pre-existing duplicate rows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum UniqueConstraintStrategy {
    /// Emit a `DELETE` statement just before the `ADD CONSTRAINT` so only
    /// one row per group of duplicates survives. `keep` picks which one.
    /// This is the only strategy in v0.2 — see module docs.
    DeleteDuplicates {
        /// Which row of each duplicate group is preserved. See
        /// [`KeepPolicy`].
        keep: KeepPolicy,
    },
}

/// Which row of a duplicate group is kept when `DeleteDuplicates` runs.
///
/// "First" / "Last" are defined by the table's PRIMARY KEY ordering:
/// `First = MIN(pk)`, `Last = MAX(pk)`. Requires a single-column PK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KeepPolicy {
    /// Keep the row with the smallest primary-key value.
    First,
    /// Keep the row with the largest primary-key value.
    Last,
}

impl Default for UniqueConstraintStrategy {
    /// Default strategy is `DeleteDuplicates { keep: First }`: keep the
    /// row with the smallest primary-key value of each duplicate group.
    /// This is what wire-format-omitted `strategy` (v0.1.x migrations or
    /// any JSON without the field) deserializes to.
    ///
    /// **Wire-format breaking change vs v0.1.x.** v0.1.x emitted no
    /// pre-cleanup and let the database reject the migration. v0.2 emits
    /// a `DELETE` automatically. Existing migrations that *already
    /// applied* under v0.1.x are unaffected (apply happens once); v0.1.x
    /// migrations *re-run* against a fresh DB will now drop duplicate
    /// rows instead of failing.
    fn default() -> Self {
        Self::DeleteDuplicates {
            keep: KeepPolicy::First,
        }
    }
}
