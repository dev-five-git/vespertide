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
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
    ForeignKey {
        #[serde(skip_serializing_if = "Option::is_none")]
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
    Index {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
}

impl TableConstraint {
    /// Returns the columns referenced by this constraint.
    /// For Check constraints, returns an empty slice (expression-based, not column-based).
    pub fn columns(&self) -> &[ColumnName] {
        match self {
            TableConstraint::PrimaryKey { columns, .. } => columns,
            TableConstraint::Unique { columns, .. } => columns,
            TableConstraint::ForeignKey { columns, .. } => columns,
            TableConstraint::Index { columns, .. } => columns,
            TableConstraint::Check { .. } => &[],
        }
    }
}
