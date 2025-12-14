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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MigrationAction {
    CreateTable {
        table: TableName,
        columns: Vec<ColumnDef>,
        constraints: Vec<TableConstraint>,
    },
    DeleteTable {
        table: TableName,
    },
    AddColumn {
        table: TableName,
        column: ColumnDef,
        /// Optional fill value to backfill existing rows when adding NOT NULL without default.
        fill_with: Option<String>,
    },
    RenameColumn {
        table: TableName,
        from: ColumnName,
        to: ColumnName,
    },
    DeleteColumn {
        table: TableName,
        column: ColumnName,
    },
    ModifyColumnType {
        table: TableName,
        column: ColumnName,
        new_type: ColumnType,
    },
    AddIndex {
        table: TableName,
        index: IndexDef,
    },
    RemoveIndex {
        table: TableName,
        name: IndexName,
    },
    AddConstraint {
        table: TableName,
        constraint: TableConstraint,
    },
    RemoveConstraint {
        table: TableName,
        constraint: TableConstraint,
    },
    RenameTable {
        from: TableName,
        to: TableName,
    },
    RawSql {
        sql: String,
    },
}
