//! Strategy for pre-existing duplicates when adding a PRIMARY KEY to
//! an existing column set.
//!
//! `AddConstraint(PrimaryKey)` against a populated table fails at apply
//! time when the chosen column set already contains duplicate values
//! (PK ⇒ UNIQUE) or NULLs (PK ⇒ NOT NULL). Vespertide handles the two
//! failures separately:
//!
//! - **Duplicate violation** → this strategy (`DeleteDuplicates`)
//!   emits a `DELETE` ahead of `ADD CONSTRAINT PRIMARY KEY`, keeping
//!   one row per group based on `keep`.
//! - **NULL violation** → handled by the existing F1 `fill_with`
//!   mechanism (`MigrationAction::AddColumn.fill_with`,
//!   `ModifyColumnDefault.backfill`). The revision CLI prompts the
//!   user for fill values on every nullable PK column.
//!
//! This is the F5 mirror of F2 (`UniqueConstraintStrategy`) - same
//! shape, different constraint family. The variants are kept separate
//! types to make the constraint family explicit at every call site.
//!
//! There is no "skip cleanup" / "fail" option - matches the F2/F3/F4
//! policy of refusing to let the database reject the migration when
//! the cleanup is trivially derivable.
//!
//! `#[non_exhaustive]` so additional strategies can be added in a
//! future minor release without breaking downstream `match`es.

use serde::{Deserialize, Serialize};

use crate::schema::unique_strategy::KeepPolicy;

/// How `AddConstraint(PrimaryKey)` should handle pre-existing
/// duplicate rows in the chosen column set.
///
/// `KeepPolicy` is reused from `UniqueConstraintStrategy` (F2) -
/// "First" / "Last" are defined by the table's surviving primary-key
/// or row identity ordering. The cleanup SQL is emitted by
/// `vespertide-query::sql::add_constraint::primary_key`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PrimaryKeyAdditionStrategy {
    /// Emit a `DELETE` statement ahead of `ADD CONSTRAINT PRIMARY KEY`
    /// so only one row per group of new-PK duplicates survives.
    DeleteDuplicates {
        /// Which row of each duplicate group is preserved.
        keep: KeepPolicy,
    },
}

impl Default for PrimaryKeyAdditionStrategy {
    /// Default strategy is `DeleteDuplicates { keep: First }`. Wire-
    /// format omitted-field decodes to this value.
    ///
    /// **Wire-format breaking change vs v0.1.x.** v0.1.x emitted no
    /// pre-cleanup and let the database reject the migration. v0.2
    /// emits a `DELETE` automatically. Already-applied migrations are
    /// unaffected (apply happens once); v0.1.x migration files re-run
    /// against a fresh DB now drop duplicate rows instead of failing.
    fn default() -> Self {
        Self::DeleteDuplicates {
            keep: KeepPolicy::First,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_delete_duplicates_first() {
        assert_eq!(
            PrimaryKeyAdditionStrategy::default(),
            PrimaryKeyAdditionStrategy::DeleteDuplicates {
                keep: KeepPolicy::First
            }
        );
    }

    #[test]
    fn serde_roundtrip_first() {
        let s = PrimaryKeyAdditionStrategy::DeleteDuplicates {
            keep: KeepPolicy::First,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"delete_duplicates","keep":"first"}"#);
        let back: PrimaryKeyAdditionStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn serde_roundtrip_last() {
        let s = PrimaryKeyAdditionStrategy::DeleteDuplicates {
            keep: KeepPolicy::Last,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"delete_duplicates","keep":"last"}"#);
        let back: PrimaryKeyAdditionStrategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}
