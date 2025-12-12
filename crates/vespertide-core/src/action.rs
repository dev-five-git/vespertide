use crate::schema::{
    ColumnDef, ColumnName, ColumnType, IndexDef, IndexName, TableConstraint, TableName,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MigrationPlan {
    pub comment: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    pub version: u32,
    pub actions: Vec<MigrationAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum MigrationAction {
    #[serde(rename_all = "snake_case")]
    CreateTable {
        table: TableName,
        columns: Vec<ColumnDef>,
        constraints: Vec<TableConstraint>,
    },
    #[serde(rename_all = "snake_case")]
    DeleteTable { table: TableName },
    #[serde(rename_all = "snake_case")]
    AddColumn {
        table: TableName,
        column: ColumnDef,
        /// Optional fill value to backfill existing rows when adding NOT NULL without default.
        fill_with: Option<String>,
    },
    #[serde(rename_all = "snake_case")]
    RenameColumn {
        table: TableName,
        from: ColumnName,
        to: ColumnName,
    },
    #[serde(rename_all = "snake_case")]
    DeleteColumn {
        table: TableName,
        column: ColumnName,
    },
    #[serde(rename_all = "snake_case")]
    ModifyColumnType {
        table: TableName,
        column: ColumnName,
        new_type: ColumnType,
    },
    #[serde(rename_all = "snake_case")]
    AddIndex { table: TableName, index: IndexDef },
    #[serde(rename_all = "snake_case")]
    RemoveIndex { table: TableName, name: IndexName },
    #[serde(rename_all = "snake_case")]
    AddConstraint {
        table: TableName,
        constraint: TableConstraint,
    },
    #[serde(rename_all = "snake_case")]
    RemoveConstraint {
        table: TableName,
        constraint: TableConstraint,
    },
    #[serde(rename_all = "snake_case")]
    RenameTable { from: TableName, to: TableName },
    #[serde(rename_all = "snake_case")]
    RawSql { sql: String },
}
