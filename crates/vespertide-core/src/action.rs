use crate::schema::{
    ColumnDef, ColumnName, ColumnType, IndexDef, IndexName, TableConstraint, TableName,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

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

impl fmt::Display for MigrationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationAction::CreateTable { table, .. } => {
                write!(f, "CreateTable: {}", table)
            }
            MigrationAction::DeleteTable { table } => {
                write!(f, "DeleteTable: {}", table)
            }
            MigrationAction::AddColumn { table, column, .. } => {
                write!(f, "AddColumn: {}.{}", table, column.name)
            }
            MigrationAction::RenameColumn { table, from, to } => {
                write!(f, "RenameColumn: {}.{} -> {}", table, from, to)
            }
            MigrationAction::DeleteColumn { table, column } => {
                write!(f, "DeleteColumn: {}.{}", table, column)
            }
            MigrationAction::ModifyColumnType { table, column, .. } => {
                write!(f, "ModifyColumnType: {}.{}", table, column)
            }
            MigrationAction::AddIndex { table, index } => {
                write!(f, "AddIndex: {}.{}", table, index.name)
            }
            MigrationAction::RemoveIndex { name, .. } => {
                write!(f, "RemoveIndex: {}", name)
            }
            MigrationAction::AddConstraint { table, constraint } => {
                let constraint_name = match constraint {
                    TableConstraint::PrimaryKey { .. } => "PRIMARY KEY",
                    TableConstraint::Unique { name, .. } => {
                        if let Some(n) = name {
                            return write!(f, "AddConstraint: {}.{} (UNIQUE)", table, n);
                        }
                        "UNIQUE"
                    }
                    TableConstraint::ForeignKey { name, .. } => {
                        if let Some(n) = name {
                            return write!(f, "AddConstraint: {}.{} (FOREIGN KEY)", table, n);
                        }
                        "FOREIGN KEY"
                    }
                    TableConstraint::Check { name, .. } => {
                        return write!(f, "AddConstraint: {}.{} (CHECK)", table, name);
                    }
                };
                write!(f, "AddConstraint: {}.{}", table, constraint_name)
            }
            MigrationAction::RemoveConstraint { table, constraint } => {
                let constraint_name = match constraint {
                    TableConstraint::PrimaryKey { .. } => "PRIMARY KEY",
                    TableConstraint::Unique { name, .. } => {
                        if let Some(n) = name {
                            return write!(f, "RemoveConstraint: {}.{} (UNIQUE)", table, n);
                        }
                        "UNIQUE"
                    }
                    TableConstraint::ForeignKey { name, .. } => {
                        if let Some(n) = name {
                            return write!(f, "RemoveConstraint: {}.{} (FOREIGN KEY)", table, n);
                        }
                        "FOREIGN KEY"
                    }
                    TableConstraint::Check { name, .. } => {
                        return write!(f, "RemoveConstraint: {}.{} (CHECK)", table, name);
                    }
                };
                write!(f, "RemoveConstraint: {}.{}", table, constraint_name)
            }
            MigrationAction::RenameTable { from, to } => {
                write!(f, "RenameTable: {} -> {}", from, to)
            }
            MigrationAction::RawSql { sql } => {
                // Truncate SQL if too long for display
                let display_sql = if sql.len() > 50 {
                    format!("{}...", &sql[..47])
                } else {
                    sql.clone()
                };
                write!(f, "RawSql: {}", display_sql)
            }
        }
    }
}
