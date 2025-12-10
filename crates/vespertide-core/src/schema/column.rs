use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::names::ColumnName;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub r#type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ColumnType {
    Integer,
    BigInt,
    Text,
    Boolean,
    Timestamp,
    Custom(String),
}
