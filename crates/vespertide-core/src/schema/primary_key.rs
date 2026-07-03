use serde::{Deserialize, Serialize};

/// Configuration for a primary key declared with the object syntax.
///
/// When `auto_increment` is `true` the column is generated as a serial / identity column
/// (`PostgreSQL` `SERIAL`, `MySQL` `AUTO_INCREMENT`, `SQLite` `AUTOINCREMENT`).
/// Use [`PrimaryKeySyntax::Object`] to pass this to a [`ColumnDef`].
///
/// [`ColumnDef`]: crate::schema::ColumnDef
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct PrimaryKeyDef {
    /// When `true`, the column value is generated automatically by the database on insert.
    #[serde(default)]
    pub auto_increment: bool,
}

/// Inline primary key declaration on a [`ColumnDef`], supporting both shorthand and full syntax.
///
/// In JSON model files you can write either:
/// - `"primary_key": true` (shorthand, no auto-increment)
/// - `"primary_key": {"auto_increment": true}` (full object syntax)
///
/// [`ColumnDef`]: crate::schema::ColumnDef
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", untagged)]
pub enum PrimaryKeySyntax {
    /// Shorthand: `true` marks the column as a primary key without auto-increment.
    Bool(bool),
    /// Full object syntax allowing `auto_increment` to be configured.
    Object(PrimaryKeyDef),
}
