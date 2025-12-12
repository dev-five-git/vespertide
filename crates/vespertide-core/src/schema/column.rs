use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{foreign_key::ForeignKeyDef, names::ColumnName, str_or_bool::StrOrBoolOrArray};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub r#type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
    pub primary_key: Option<bool>,
    pub unique: Option<StrOrBoolOrArray>,
    pub index: Option<StrOrBoolOrArray>,
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
