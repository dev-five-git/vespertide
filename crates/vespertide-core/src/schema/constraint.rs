use serde::{Deserialize, Serialize};

use crate::schema::{
    ReferenceAction,
    check_violation_strategy::CheckViolationStrategy,
    fk_orphan_strategy::ForeignKeyOrphanStrategy,
    names::{ColumnName, TableName},
    pk_addition_strategy::PrimaryKeyAdditionStrategy,
    unique_strategy::{KeepPolicy, UniqueConstraintStrategy},
};

/// `serde(skip_serializing_if)` helper ? true when `strategy` is the
/// canonical default (`DeleteDuplicates { keep: First }`). Lets the
/// common case omit the field from the JSON wire format.
fn is_default_unique_strategy(s: &UniqueConstraintStrategy) -> bool {
    matches!(
        s,
        UniqueConstraintStrategy::DeleteDuplicates {
            keep: KeepPolicy::First
        }
    )
}

/// `serde(skip_serializing_if)` helper ? true when `orphan_strategy` is
/// the canonical default (`NullifyOrphans`). Lets the common case omit
/// the field from the JSON wire format.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde `skip_serializing_if` callbacks must have signature `fn(&T) -> bool`"
)]
fn is_default_fk_orphan_strategy(s: &ForeignKeyOrphanStrategy) -> bool {
    matches!(s, ForeignKeyOrphanStrategy::NullifyOrphans)
}

/// `serde(skip_serializing_if)` helper ? true when CHECK `strategy` is the
/// canonical default (`DeleteViolatingRows`). Lets the common case
/// omit the field from the JSON wire format.
fn is_default_check_violation_strategy(s: &CheckViolationStrategy) -> bool {
    matches!(s, CheckViolationStrategy::DeleteViolatingRows)
}

/// `serde(skip_serializing_if)` helper ? true when PK `strategy` is
/// the canonical default (`DeleteDuplicates { keep: First }`). Lets
/// the common case omit the field from the JSON wire format.
fn is_default_pk_addition_strategy(s: &PrimaryKeyAdditionStrategy) -> bool {
    matches!(
        s,
        PrimaryKeyAdditionStrategy::DeleteDuplicates {
            keep: KeepPolicy::First
        }
    )
}

/// A table-level constraint produced by [`TableDef::normalize`].
///
/// Inline column constraints (`primary_key`, `unique`, `index`, `foreign_key`) declared in model
/// JSON files are converted into `TableConstraint` variants during normalization. You rarely
/// construct these directly; the planner and SQL generator consume them.
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
///
/// [`TableDef::normalize`]: crate::schema::TableDef::normalize
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum TableConstraint {
    /// Primary key constraint, optionally with auto-increment (serial / identity) semantics.
    ///
    /// `strategy` controls how pre-existing duplicate rows in the
    /// chosen column set are handled when this constraint is added to
    /// an already-populated table. The canonical default is
    /// [`PrimaryKeyAdditionStrategy::DeleteDuplicates { keep: KeepPolicy::First }`]
    /// (omitted from the JSON wire format). NULL violations on PK
    /// columns are handled separately by the F1 `fill_with` mechanism;
    /// the revision CLI prompts for fill values on every nullable PK
    /// column.
    ///
    /// `strategy` is **stripped from `model.schema.json`** but
    /// **preserved in `migration.schema.json`**.
    PrimaryKey {
        #[serde(default)]
        auto_increment: bool,
        columns: Vec<ColumnName>,
        #[serde(default, skip_serializing_if = "is_default_pk_addition_strategy")]
        strategy: PrimaryKeyAdditionStrategy,
    },
    /// Unique constraint ensuring no two rows share the same value(s) in the listed columns.
    ///
    /// `strategy` controls how pre-existing duplicate rows are handled when
    /// this constraint is added to an already-populated table. The
    /// canonical default is [`UniqueConstraintStrategy::DeleteDuplicates { keep: KeepPolicy::First }`],
    /// which matches v0.1.x behaviour and is omitted from the JSON wire
    /// format. Other strategies (e.g. `DeleteDuplicates`) emit a pre-cleanup
    /// step ahead of `ADD CONSTRAINT` so the migration succeeds even when
    /// production data has duplicates.
    ///
    /// `strategy` is **stripped from `model.schema.json`** by the
    /// schema generator (`vespertide-schema-gen`), but **preserved in
    /// `migration.schema.json`** since migration files carry the
    /// revision CLI's stamped choice.
    Unique {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
        #[serde(default, skip_serializing_if = "is_default_unique_strategy")]
        strategy: UniqueConstraintStrategy,
    },
    /// Foreign key constraint linking columns in this table to columns in another table.
    ///
    /// `orphan_strategy` controls how pre-existing orphan rows are
    /// handled when this constraint is added to an already-populated
    /// table. The canonical default is
    /// [`ForeignKeyOrphanStrategy::NullifyOrphans`] (omitted from the
    /// JSON wire format). The revision CLI re-prompts the user for an
    /// explicit choice; the default exists only so v0.1.x model files
    /// continue to deserialize.
    ///
    /// `orphan_strategy` is **stripped from `model.schema.json`** by
    /// the schema generator but **preserved in `migration.schema.json`**.
    ForeignKey {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
        ref_table: TableName,
        ref_columns: Vec<ColumnName>,
        on_delete: Option<ReferenceAction>,
        on_update: Option<ReferenceAction>,
        #[serde(default, skip_serializing_if = "is_default_fk_orphan_strategy")]
        orphan_strategy: ForeignKeyOrphanStrategy,
    },
    /// Arbitrary SQL CHECK expression enforced by the database on every write.
    ///
    /// `strategy` controls how pre-existing violating rows are handled when
    /// this constraint is added to an already-populated table. The
    /// canonical default is [`CheckViolationStrategy::NullifyViolatingColumn`]
    /// (omitted from the JSON wire format). The revision CLI re-prompts
    /// the user for an explicit choice; the default exists only so v0.1.x
    /// model files continue to deserialize.
    ///
    /// Cleanup SQL is emitted only when the expression matches a narrow
    /// recognisable shape (`<col> <op> <literal>` or `<col> IN (...)`);
    /// more complex expressions skip pre-cleanup and rely on the database
    /// to validate at apply time.
    ///
    /// `strategy` is **stripped from `model.schema.json`** but
    /// **preserved in `migration.schema.json`**.
    Check {
        name: String,
        expr: String,
        #[serde(default, skip_serializing_if = "is_default_check_violation_strategy")]
        strategy: CheckViolationStrategy,
    },
    /// Non-unique index to speed up queries on the listed columns.
    Index {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
}

/// Lightweight tag identifying the kind of a [`TableConstraint`] without carrying its data.
///
/// Returned by [`TableConstraint::kind`] and useful for filtering or grouping constraints without
/// pattern-matching on the full enum.
///
/// This enum is `#[non_exhaustive]`: new variants may be added in future releases.
/// Downstream `match` expressions should include a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConstraintKind {
    /// Identifies a [`TableConstraint::PrimaryKey`] constraint.
    PrimaryKey,
    /// Identifies a [`TableConstraint::ForeignKey`] constraint.
    ForeignKey,
    /// Identifies a [`TableConstraint::Unique`] constraint.
    Unique,
    /// Identifies a [`TableConstraint::Check`] constraint.
    Check,
    /// Identifies a [`TableConstraint::Index`] constraint.
    Index,
}

impl TableConstraint {
    /// Returns the high-level kind of this constraint.
    #[must_use]
    pub fn kind(&self) -> ConstraintKind {
        match self {
            TableConstraint::PrimaryKey { .. } => ConstraintKind::PrimaryKey,
            TableConstraint::ForeignKey { .. } => ConstraintKind::ForeignKey,
            TableConstraint::Unique { .. } => ConstraintKind::Unique,
            TableConstraint::Check { .. } => ConstraintKind::Check,
            TableConstraint::Index { .. } => ConstraintKind::Index,
        }
    }

    /// Returns the columns referenced by this constraint.
    /// For Check constraints, returns an empty slice (expression-based, not column-based).
    pub fn columns(&self) -> &[ColumnName] {
        match self {
            TableConstraint::PrimaryKey { columns, .. }
            | TableConstraint::Unique { columns, .. }
            | TableConstraint::ForeignKey { columns, .. }
            | TableConstraint::Index { columns, .. } => columns,
            TableConstraint::Check { .. } => &[],
        }
    }

    /// Apply a prefix to referenced table names in this constraint.
    /// Only affects `ForeignKey` constraints (which reference other tables).
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        match self {
            TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                orphan_strategy,
            } => TableConstraint::ForeignKey {
                name,
                columns,
                ref_table: ref_table.with_prefix(prefix),
                ref_columns,
                on_delete,
                on_update,
                orphan_strategy,
            },
            // Other constraints don't reference external tables
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_columns_primary_key() {
        let pk = TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into(), "tenant_id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        };
        assert_eq!(pk.columns().len(), 2);
        assert_eq!(pk.columns()[0], "id");
        assert_eq!(pk.columns()[1], "tenant_id");
    }

    #[test]
    fn test_columns_unique() {
        let unique = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                keep: crate::schema::KeepPolicy::First,
            },
        };
        assert_eq!(unique.columns().len(), 1);
        assert_eq!(unique.columns()[0], "email");
    }

    #[test]
    fn test_columns_foreign_key() {
        let fk = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
        };
        assert_eq!(fk.columns().len(), 1);
        assert_eq!(fk.columns()[0], "user_id");
    }

    #[test]
    fn test_columns_index() {
        let idx = TableConstraint::Index {
            name: Some("ix_created_at".into()),
            columns: vec!["created_at".into()],
        };
        assert_eq!(idx.columns().len(), 1);
        assert_eq!(idx.columns()[0], "created_at");
    }

    #[test]
    fn test_columns_check_returns_empty() {
        let check = TableConstraint::Check {
            name: "check_positive".into(),
            expr: "amount > 0".into(),
            strategy: crate::CheckViolationStrategy::default(),
        };
        assert!(check.columns().is_empty());
    }

    #[test]
    fn test_kind() {
        let constraints = [
            (
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: PrimaryKeyAdditionStrategy::default(),
                },
                ConstraintKind::PrimaryKey,
            ),
            (
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
                },
                ConstraintKind::ForeignKey,
            ),
            (
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                    strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                        keep: crate::schema::KeepPolicy::First,
                    },
                },
                ConstraintKind::Unique,
            ),
            (
                TableConstraint::Check {
                    name: "check_positive".into(),
                    expr: "amount > 0".into(),
                    strategy: crate::CheckViolationStrategy::default(),
                },
                ConstraintKind::Check,
            ),
            (
                TableConstraint::Index {
                    name: None,
                    columns: vec!["email".into()],
                },
                ConstraintKind::Index,
            ),
        ];

        for (constraint, expected) in constraints {
            assert_eq!(constraint.kind(), expected);
        }
    }

    #[test]
    fn test_with_prefix_foreign_key() {
        let fk = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
        };
        let prefixed = fk.with_prefix("myapp_");
        if let TableConstraint::ForeignKey { ref_table, .. } = prefixed {
            assert_eq!(ref_table.as_str(), "myapp_users");
        } else {
            panic!("Expected ForeignKey");
        }
    }

    #[test]
    fn test_with_prefix_non_fk_unchanged() {
        let pk = TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        };
        let prefixed = pk.clone().with_prefix("myapp_");
        assert_eq!(pk, prefixed);

        let unique = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                keep: crate::schema::KeepPolicy::First,
            },
        };
        let prefixed = unique.clone().with_prefix("myapp_");
        assert_eq!(unique, prefixed);

        let idx = TableConstraint::Index {
            name: Some("ix_created_at".into()),
            columns: vec!["created_at".into()],
        };
        let prefixed = idx.clone().with_prefix("myapp_");
        assert_eq!(idx, prefixed);

        let check = TableConstraint::Check {
            name: "check_positive".into(),
            expr: "amount > 0".into(),
            strategy: crate::CheckViolationStrategy::default(),
        };
        let prefixed = check.clone().with_prefix("myapp_");
        assert_eq!(check, prefixed);
    }

    #[test]
    fn test_with_prefix_empty_prefix() {
        let fk = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
        };
        let prefixed = fk.clone().with_prefix("");
        assert_eq!(fk, prefixed);
    }
}
