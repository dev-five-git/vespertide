use serde::{Deserialize, Serialize};

use crate::schema::names::{ColumnName, IndexName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDef {
    pub name: IndexName,
    pub columns: Vec<ColumnName>,
    pub unique: bool,
}

