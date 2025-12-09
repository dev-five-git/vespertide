use serde::{Deserialize, Serialize};

use crate::schema::names::ColumnName;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub data_type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Integer,
    BigInt,
    Text,
    Boolean,
    Timestamp,
    Custom(String),
}

