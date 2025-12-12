use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{StrOrBool, foreign_key::ForeignKeyDef, names::ColumnName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub r#type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
    pub primary_key: Option<bool>,
    pub unique: Option<StrOrBool>,
    pub index: Option<StrOrBool>,
    pub foreign_key: Option<ForeignKeyDef>,
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
