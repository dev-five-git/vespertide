use serde::{Deserialize, Serialize};

pub type TableName = String;
pub type ColumnName = String;
pub type IndexName = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDef {
    pub name: TableName,
    pub columns: Vec<ColumnDef>,
    pub constraints: Vec<TableConstraint>,
    pub indexes: Vec<IndexDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDef {
    pub name: ColumnName,
    pub data_type: ColumnType,
    pub nullable: bool,
    pub default: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceAction {
    Cascade,
    Restrict,
    SetNull,
    SetDefault,
    NoAction,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDef {
    pub name: IndexName,
    pub columns: Vec<ColumnName>,
    pub unique: bool,
}
