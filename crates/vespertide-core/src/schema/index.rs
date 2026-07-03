use serde::{Deserialize, Serialize};

use crate::schema::names::{ColumnName, IndexName};

/// A named index on one or more columns of a table.
///
/// Index names follow the convention `ix_{table}__{columns}` (double underscore separator).
/// When `unique` is `true` the index also enforces a uniqueness constraint, equivalent to a
/// `UNIQUE` constraint but expressed as an index.
///
/// `IndexDef` is the normalized form produced from inline `"index"` declarations on columns.
/// You rarely construct this directly; use the `"index"` field on a [`ColumnDef`] in your model
/// JSON and let the planner normalize it.
///
/// [`ColumnDef`]: crate::schema::ColumnDef
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct IndexDef {
    /// The index name, conventionally `ix_{table}__{columns}`.
    pub name: IndexName,
    /// The ordered list of columns included in the index.
    pub columns: Vec<ColumnName>,
    /// When `true`, the index enforces uniqueness across the listed columns.
    pub unique: bool,
}
