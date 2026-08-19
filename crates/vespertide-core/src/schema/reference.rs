use serde::{Deserialize, Serialize};

/// The referential action taken on child rows when the referenced parent row changes.
///
/// Used in `ForeignKeyDef::on_delete` and `ForeignKeyDef::on_update` to control cascading
/// behaviour. In JSON model files these are written in `snake_case`
/// (e.g. `"on_delete": "cascade"`).
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReferenceAction {
    /// Automatically delete or update child rows when the parent row is deleted or updated (`CASCADE`).
    Cascade,
    /// Prevent the parent row from being deleted or updated if child rows exist (`RESTRICT`).
    Restrict,
    /// Set the foreign key column(s) in child rows to `NULL` when the parent changes (`SET NULL`).
    /// The column must be nullable.
    SetNull,
    /// Set the foreign key column(s) in child rows to their column default when the parent changes (`SET DEFAULT`).
    SetDefault,
    /// Do nothing to child rows; the database defers enforcement or raises an error (`NO ACTION`).
    NoAction,
}

impl ReferenceAction {
    /// SQL keyword representation as written in `ALTER TABLE ... ADD
    /// CONSTRAINT ... FOREIGN KEY ... ON DELETE <keyword>` etc. Used by
    /// `vespertide-query` when emitting raw SQL (e.g. the F11
    /// `NOT VALID` + `VALIDATE` PG path, which bypasses the sea-query
    /// `ForeignKey` builder).
    #[must_use]
    pub fn to_sql_keyword(&self) -> &'static str {
        match self {
            Self::Cascade => "CASCADE",
            Self::Restrict => "RESTRICT",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
            Self::NoAction => "NO ACTION",
        }
    }
}

/// Closed, exhaustive mirror of [`ReferenceAction`] for downstream crates that need to
/// `match` on it without a wildcard arm.
///
/// [`ReferenceAction`] is `#[non_exhaustive]`, so every match on it made *outside*
/// `vespertide-core` must carry a `_` arm even when every current variant is already
/// handled — that arm is genuinely unreachable and shows up as a permanent 0-hit line
/// under coverage instrumentation. This type is deliberately **not** `#[non_exhaustive]`:
/// the conversion below is written inside the crate that owns `ReferenceAction`, where
/// the non-exhaustiveness restriction doesn't apply, so it can be matched exhaustively
/// with no wildcard, here and in every downstream crate. If `ReferenceAction` ever gains
/// a variant, `From<&ReferenceAction> for ReferenceActionKind` below fails to compile
/// until a matching variant is added here — a compile-time forcing function that
/// replaces the old pattern of a runtime `unreachable!()` guard that only a test could
/// catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceActionKind {
    Cascade,
    Restrict,
    SetNull,
    SetDefault,
    NoAction,
}

impl From<&ReferenceAction> for ReferenceActionKind {
    fn from(action: &ReferenceAction) -> Self {
        match action {
            ReferenceAction::Cascade => Self::Cascade,
            ReferenceAction::Restrict => Self::Restrict,
            ReferenceAction::SetNull => Self::SetNull,
            ReferenceAction::SetDefault => Self::SetDefault,
            ReferenceAction::NoAction => Self::NoAction,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for `ReferenceAction::to_sql_keyword`.
    //! Targets `uncovered-detail.json` lines 40, 41, 42
    //! (`SetNull` / `SetDefault` / `NoAction` match arms).
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, "CASCADE")]
    #[case::restrict(ReferenceAction::Restrict, "RESTRICT")]
    #[case::set_null(ReferenceAction::SetNull, "SET NULL")]
    #[case::set_default(ReferenceAction::SetDefault, "SET DEFAULT")]
    #[case::no_action(ReferenceAction::NoAction, "NO ACTION")]
    fn to_sql_keyword_emits_expected_token(
        #[case] action: ReferenceAction,
        #[case] expected: &'static str,
    ) {
        // Each rstest case visits one match arm of to_sql_keyword. The
        // SetNull/SetDefault/NoAction cases cover the previously-uncovered
        // lines 40, 41, 42.
        assert_eq!(action.to_sql_keyword(), expected);
    }

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, ReferenceActionKind::Cascade)]
    #[case::restrict(ReferenceAction::Restrict, ReferenceActionKind::Restrict)]
    #[case::set_null(ReferenceAction::SetNull, ReferenceActionKind::SetNull)]
    #[case::set_default(ReferenceAction::SetDefault, ReferenceActionKind::SetDefault)]
    #[case::no_action(ReferenceAction::NoAction, ReferenceActionKind::NoAction)]
    fn reference_action_kind_from_matches_variant(
        #[case] action: ReferenceAction,
        #[case] expected: ReferenceActionKind,
    ) {
        assert_eq!(ReferenceActionKind::from(&action), expected);
    }
}
