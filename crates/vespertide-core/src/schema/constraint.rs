use serde::{Deserialize, Serialize};

use crate::schema::{names::ColumnName, names::TableName, ReferenceAction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableConstraint {
    PrimaryKey(Vec<ColumnName>),
    Unique {
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
    ForeignKey {
        name: Option<String>,
        columns: Vec<ColumnName>,
        #[serde(rename = "refTable")]
        ref_table: TableName,
        #[serde(rename = "refColumns")]
        ref_columns: Vec<ColumnName>,
        #[serde(rename = "onDelete")]
        on_delete: Option<ReferenceAction>,
        #[serde(rename = "onUpdate")]
        on_update: Option<ReferenceAction>,
    },
    Check {
        name: Option<String>,
        expr: String,
    },
}

