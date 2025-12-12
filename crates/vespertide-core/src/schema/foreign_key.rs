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
