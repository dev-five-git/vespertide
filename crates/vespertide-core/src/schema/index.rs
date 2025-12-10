use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

use crate::schema::names::{ColumnName, IndexName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct IndexDef {
    pub name: IndexName,
    pub columns: Vec<ColumnName>,
    pub unique: bool,
}

