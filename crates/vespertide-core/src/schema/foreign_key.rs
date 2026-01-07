use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{names::ColumnName, names::TableName, reference::ReferenceAction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ForeignKeyDef {
    pub ref_table: TableName,
    pub ref_columns: Vec<ColumnName>,
    pub on_delete: Option<ReferenceAction>,
    pub on_update: Option<ReferenceAction>,
}

/// Shorthand syntax for foreign key: { "references": "table.column", "on_delete": "cascade" }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReferenceSyntaxDef {
    /// Reference in "table.column" format
    pub references: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<ReferenceAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_update: Option<ReferenceAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum ForeignKeySyntax {
    /// table.column
    String(String),
    /// { "references": "table.column", "on_delete": "cascade" }
    Reference(ReferenceSyntaxDef),
    /// { "ref_table": "table", "ref_columns": ["column"], ... }
    Object(ForeignKeyDef),
}
