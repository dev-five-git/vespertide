use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{
    ReferenceAction,
    names::{ColumnName, TableName},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TableConstraint {
    PrimaryKey {
        #[serde(default)]
        auto_increment: bool,
        columns: Vec<ColumnName>,
    },
    Unique {
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
    ForeignKey {
        name: Option<String>,
        columns: Vec<ColumnName>,
        ref_table: TableName,
        ref_columns: Vec<ColumnName>,
        on_delete: Option<ReferenceAction>,
        on_update: Option<ReferenceAction>,
    },
    Check {
        name: String,
        expr: String,
    },
}
