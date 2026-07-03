use serde::{Deserialize, Serialize};

use crate::schema::{
    fk_orphan_strategy::ForeignKeyOrphanStrategy, names::ColumnName, names::TableName,
    reference::ReferenceAction,
};

/// `serde(skip_serializing_if)` helper - true when `orphan_strategy` is
/// the canonical default. Mirror of the matching helper in
/// `schema::constraint`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde `skip_serializing_if` callbacks must have signature `fn(&T) -> bool`"
)]
fn is_default_fk_orphan_strategy(s: &ForeignKeyOrphanStrategy) -> bool {
    matches!(s, ForeignKeyOrphanStrategy::NullifyOrphans)
}

/// Full foreign key definition used in the normalized table representation.
///
/// Specifies the referenced table and columns along with optional referential actions for
/// `ON DELETE` and `ON UPDATE`. This is the canonical form produced after parsing any of the
/// three [`ForeignKeySyntax`] variants.
///
/// Always add `"index": true` on the column carrying the foreign key for query performance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ForeignKeyDef {
    /// The table being referenced (the "parent" side of the relationship).
    pub ref_table: TableName,
    /// The column(s) in the referenced table that this foreign key points to.
    pub ref_columns: Vec<ColumnName>,
    /// Action to take on child rows when the parent row is deleted.
    pub on_delete: Option<ReferenceAction>,
    /// Action to take on child rows when the referenced column(s) in the parent row are updated.
    pub on_update: Option<ReferenceAction>,
    /// Pre-cleanup strategy for orphan child rows when the FK is added to
    /// a populated table. See [`ForeignKeyOrphanStrategy`] for semantics;
    /// the canonical default ([`ForeignKeyOrphanStrategy::NullifyOrphans`])
    /// is omitted from the JSON wire format.
    ///
    /// **Stripped from `model.schema.json`** by the schema generator but
    /// **preserved in `migration.schema.json`**.
    #[serde(default, skip_serializing_if = "is_default_fk_orphan_strategy")]
    pub orphan_strategy: ForeignKeyOrphanStrategy,
}

/// Compact foreign key syntax using a `"references"` string in `"table.column"` format.
///
/// Useful when you only need to specify the target and optionally an `on_delete` action without
/// listing columns explicitly. The planner resolves the column from the `"table.column"` string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ReferenceSyntaxDef {
    /// Reference target in `"table.column"` format, e.g. `"user.id"`.
    pub references: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<ReferenceAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_update: Option<ReferenceAction>,
}

/// Inline foreign key declaration on a [`ColumnDef`], supporting three levels of verbosity.
///
/// In JSON model files you can write:
/// - `"foreign_key": "user.id"` (string shorthand)
/// - `"foreign_key": {"references": "user.id", "on_delete": "cascade"}` (reference object)
/// - `"foreign_key": {"ref_table": "user", "ref_columns": ["id"], "on_delete": "cascade"}` (full object)
///
/// All three forms are normalized into a [`ForeignKeyDef`] by the planner.
///
/// [`ColumnDef`]: crate::schema::ColumnDef
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", untagged)]
pub enum ForeignKeySyntax {
    /// Shorthand string in `"table.column"` format, e.g. `"user.id"`.
    String(String),
    /// Object with a `"references"` key in `"table.column"` format plus optional actions.
    Reference(ReferenceSyntaxDef),
    /// Full object with explicit `ref_table`, `ref_columns`, and optional actions.
    Object(ForeignKeyDef),
}
