use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

use crate::schema::{
    column::ColumnDef, constraint::TableConstraint, index::IndexDef, names::TableName,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TableDef {
    pub name: TableName,
    pub columns: Vec<ColumnDef>,
    pub constraints: Vec<TableConstraint>,
    pub indexes: Vec<IndexDef>,
}
