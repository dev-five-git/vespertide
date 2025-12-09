use crate::schema::{
    ColumnDef, ColumnName, ColumnType, IndexDef, IndexName, TableConstraint, TableName,
};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub comment: Option<String>,
    pub version: u32,
    pub actions: Vec<MigrationAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum MigrationAction {
    #[serde(rename_all = "camelCase")]
    CreateTable {
        table: TableName,
        columns: Vec<ColumnDef>,
        constraints: Vec<TableConstraint>,
    },
    #[serde(rename_all = "camelCase")]
    DeleteTable { table: TableName },
    #[serde(rename_all = "camelCase")]
    AddColumn {
        table: TableName,
        column: ColumnDef,
        /// Optional fill value to backfill existing rows when adding NOT NULL without default.
        fill_with: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    RenameColumn {
        table: TableName,
        from: ColumnName,
        to: ColumnName,
    },
    #[serde(rename_all = "camelCase")]
    DeleteColumn {
        table: TableName,
        column: ColumnName,
    },
    #[serde(rename_all = "camelCase")]
    ModifyColumnType {
        table: TableName,
        column: ColumnName,
        #[serde(rename = "newType")]
        new_type: ColumnType,
    },
    #[serde(rename_all = "camelCase")]
    AddIndex { table: TableName, index: IndexDef },
    #[serde(rename_all = "camelCase")]
    RemoveIndex { table: TableName, name: IndexName },
    #[serde(rename_all = "camelCase")]
    RenameTable { from: TableName, to: TableName },
}
