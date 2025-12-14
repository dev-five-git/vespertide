use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::names::ColumnName;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PrimaryKeyDef {
    #[serde(default)]
    pub auto_increment: bool,
    pub columns: Vec<ColumnName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", untagged)]
pub enum PrimaryKeySyntax {
    Bool(bool),
    Object(PrimaryKeyDef),
}
