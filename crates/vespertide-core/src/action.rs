use crate::schema::{ColumnDef, ColumnName, ColumnType, TableConstraint, TableName};
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
        column: Box<ColumnDef>,
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
    ModifyColumnNullable {
        table: TableName,
        column: ColumnName,
        nullable: bool,
        /// Required when changing from nullable to non-nullable to backfill existing NULL values.
        fill_with: Option<String>,
    },
    ModifyColumnDefault {
        table: TableName,
        column: ColumnName,
        /// The new default value, or None to remove the default.
        new_default: Option<String>,
    },
    ModifyColumnComment {
        table: TableName,
        column: ColumnName,
        /// The new comment, or None to remove the comment.
        new_comment: Option<String>,
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

impl MigrationPlan {
    /// Apply a prefix to all table names in the migration plan.
    /// This modifies all table references in all actions.
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        Self {
            actions: self
                .actions
                .into_iter()
                .map(|action| action.with_prefix(prefix))
                .collect(),
            ..self
        }
    }
}

impl MigrationAction {
    /// Apply a prefix to all table names in this action.
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        match self {
            MigrationAction::CreateTable {
                table,
                columns,
                constraints,
            } => MigrationAction::CreateTable {
                table: format!("{}{}", prefix, table),
                columns,
                constraints: constraints
                    .into_iter()
                    .map(|c| c.with_prefix(prefix))
                    .collect(),
            },
            MigrationAction::DeleteTable { table } => MigrationAction::DeleteTable {
                table: format!("{}{}", prefix, table),
            },
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => MigrationAction::AddColumn {
                table: format!("{}{}", prefix, table),
                column,
                fill_with,
            },
            MigrationAction::RenameColumn { table, from, to } => MigrationAction::RenameColumn {
                table: format!("{}{}", prefix, table),
                from,
                to,
            },
            MigrationAction::DeleteColumn { table, column } => MigrationAction::DeleteColumn {
                table: format!("{}{}", prefix, table),
                column,
            },
            MigrationAction::ModifyColumnType {
                table,
                column,
                new_type,
            } => MigrationAction::ModifyColumnType {
                table: format!("{}{}", prefix, table),
                column,
                new_type,
            },
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                fill_with,
            } => MigrationAction::ModifyColumnNullable {
                table: format!("{}{}", prefix, table),
                column,
                nullable,
                fill_with,
            },
            MigrationAction::ModifyColumnDefault {
                table,
                column,
                new_default,
            } => MigrationAction::ModifyColumnDefault {
                table: format!("{}{}", prefix, table),
                column,
                new_default,
            },
            MigrationAction::ModifyColumnComment {
                table,
                column,
                new_comment,
            } => MigrationAction::ModifyColumnComment {
                table: format!("{}{}", prefix, table),
                column,
                new_comment,
            },
            MigrationAction::AddConstraint { table, constraint } => {
                MigrationAction::AddConstraint {
                    table: format!("{}{}", prefix, table),
                    constraint: constraint.with_prefix(prefix),
                }
            }
            MigrationAction::RemoveConstraint { table, constraint } => {
                MigrationAction::RemoveConstraint {
                    table: format!("{}{}", prefix, table),
                    constraint: constraint.with_prefix(prefix),
                }
            }
            MigrationAction::RenameTable { from, to } => MigrationAction::RenameTable {
                from: format!("{}{}", prefix, from),
                to: format!("{}{}", prefix, to),
            },
            MigrationAction::RawSql { sql } => MigrationAction::RawSql { sql },
        }
    }
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
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                ..
            } => {
                let nullability = if *nullable { "NULL" } else { "NOT NULL" };
                write!(
                    f,
                    "ModifyColumnNullable: {}.{} -> {}",
                    table, column, nullability
                )
            }
            MigrationAction::ModifyColumnDefault {
                table,
                column,
                new_default,
            } => {
                if let Some(default) = new_default {
                    write!(
                        f,
                        "ModifyColumnDefault: {}.{} -> {}",
                        table, column, default
                    )
                } else {
                    write!(f, "ModifyColumnDefault: {}.{} -> (none)", table, column)
                }
            }
            MigrationAction::ModifyColumnComment {
                table,
                column,
                new_comment,
            } => {
                if let Some(comment) = new_comment {
                    let display = if comment.len() > 30 {
                        format!("{}...", &comment[..27])
                    } else {
                        comment.clone()
                    };
                    write!(
                        f,
                        "ModifyColumnComment: {}.{} -> '{}'",
                        table, column, display
                    )
                } else {
                    write!(f, "ModifyColumnComment: {}.{} -> (none)", table, column)
                }
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
                    TableConstraint::Index { name, .. } => {
                        if let Some(n) = name {
                            return write!(f, "AddConstraint: {}.{} (INDEX)", table, n);
                        }
                        "INDEX"
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
                    TableConstraint::Index { name, .. } => {
                        if let Some(n) = name {
                            return write!(f, "RemoveConstraint: {}.{} (INDEX)", table, n);
                        }
                        "INDEX"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ReferenceAction, SimpleColumnType};
    use rstest::rstest;

    fn default_column() -> ColumnDef {
        ColumnDef {
            name: "email".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[rstest]
    #[case::create_table(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![],
            constraints: vec![],
        },
        "CreateTable: users"
    )]
    #[case::delete_table(
        MigrationAction::DeleteTable {
            table: "users".into(),
        },
        "DeleteTable: users"
    )]
    #[case::add_column(
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(default_column()),
            fill_with: None,
        },
        "AddColumn: users.email"
    )]
    #[case::rename_column(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old_name".into(),
            to: "new_name".into(),
        },
        "RenameColumn: users.old_name -> new_name"
    )]
    #[case::delete_column(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        "DeleteColumn: users.email"
    )]
    #[case::modify_column_type(
        MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "age".into(),
            new_type: ColumnType::Simple(SimpleColumnType::Integer),
        },
        "ModifyColumnType: users.age"
    )]
    #[case::add_constraint_index_with_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("ix_users__email".into()),
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.ix_users__email (INDEX)"
    )]
    #[case::add_constraint_index_without_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.INDEX"
    )]
    #[case::remove_constraint_index_with_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: Some("ix_users__email".into()),
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.ix_users__email (INDEX)"
    )]
    #[case::remove_constraint_index_without_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.INDEX"
    )]
    #[case::rename_table(
        MigrationAction::RenameTable {
            from: "old_table".into(),
            to: "new_table".into(),
        },
        "RenameTable: old_table -> new_table"
    )]
    fn test_display_basic_actions(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(action.to_string(), expected);
    }

    #[rstest]
    #[case::add_constraint_primary_key(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        },
        "AddConstraint: users.PRIMARY KEY"
    )]
    #[case::add_constraint_unique_with_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.uq_email (UNIQUE)"
    )]
    #[case::add_constraint_unique_without_name(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "AddConstraint: users.UNIQUE"
    )]
    #[case::add_constraint_foreign_key_with_name(
        MigrationAction::AddConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
            },
        },
        "AddConstraint: posts.fk_user (FOREIGN KEY)"
    )]
    #[case::add_constraint_foreign_key_without_name(
        MigrationAction::AddConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "AddConstraint: posts.FOREIGN KEY"
    )]
    #[case::add_constraint_check(
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            },
        },
        "AddConstraint: users.chk_age (CHECK)"
    )]
    fn test_display_add_constraint(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(action.to_string(), expected);
    }

    #[rstest]
    #[case::remove_constraint_primary_key(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        },
        "RemoveConstraint: users.PRIMARY KEY"
    )]
    #[case::remove_constraint_unique_with_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.uq_email (UNIQUE)"
    )]
    #[case::remove_constraint_unique_without_name(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            },
        },
        "RemoveConstraint: users.UNIQUE"
    )]
    #[case::remove_constraint_foreign_key_with_name(
        MigrationAction::RemoveConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "RemoveConstraint: posts.fk_user (FOREIGN KEY)"
    )]
    #[case::remove_constraint_foreign_key_without_name(
        MigrationAction::RemoveConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        },
        "RemoveConstraint: posts.FOREIGN KEY"
    )]
    #[case::remove_constraint_check(
        MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            },
        },
        "RemoveConstraint: users.chk_age (CHECK)"
    )]
    fn test_display_remove_constraint(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(action.to_string(), expected);
    }

    #[rstest]
    #[case::raw_sql_short(
        MigrationAction::RawSql {
            sql: "SELECT 1".into(),
        },
        "RawSql: SELECT 1"
    )]
    fn test_display_raw_sql_short(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(action.to_string(), expected);
    }

    #[test]
    fn test_display_raw_sql_long() {
        let action = MigrationAction::RawSql {
            sql:
                "SELECT * FROM users WHERE id = 1 AND name = 'test' AND email = 'test@example.com'"
                    .into(),
        };
        let result = action.to_string();
        assert!(result.starts_with("RawSql: "));
        assert!(result.ends_with("..."));
        assert!(result.len() > 10);
    }

    #[rstest]
    #[case::modify_column_nullable_to_not_null(
        MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
        },
        "ModifyColumnNullable: users.email -> NOT NULL"
    )]
    #[case::modify_column_nullable_to_null(
        MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: true,
            fill_with: None,
        },
        "ModifyColumnNullable: users.email -> NULL"
    )]
    fn test_display_modify_column_nullable(
        #[case] action: MigrationAction,
        #[case] expected: &str,
    ) {
        assert_eq!(action.to_string(), expected);
    }

    #[rstest]
    #[case::modify_column_default_set(
        MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
        },
        "ModifyColumnDefault: users.status -> 'active'"
    )]
    #[case::modify_column_default_drop(
        MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: None,
        },
        "ModifyColumnDefault: users.status -> (none)"
    )]
    fn test_display_modify_column_default(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(action.to_string(), expected);
    }

    #[rstest]
    #[case::modify_column_comment_set(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email address".into()),
        },
        "ModifyColumnComment: users.email -> 'User email address'"
    )]
    #[case::modify_column_comment_drop(
        MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: None,
        },
        "ModifyColumnComment: users.email -> (none)"
    )]
    fn test_display_modify_column_comment(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(action.to_string(), expected);
    }

    #[test]
    fn test_display_modify_column_comment_long() {
        // Test truncation for long comments (> 30 chars)
        let action = MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some(
                "This is a very long comment that should be truncated in display".into(),
            ),
        };
        let result = action.to_string();
        assert!(result.contains("..."));
        assert!(result.contains("This is a very long comment"));
        // Should be truncated at 27 chars + "..."
        assert!(!result.contains("truncated in display"));
    }

    // Tests for with_prefix
    #[test]
    fn test_action_with_prefix_create_table() {
        let action = MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![default_column()],
            constraints: vec![TableConstraint::ForeignKey {
                name: Some("fk_org".into()),
                columns: vec!["org_id".into()],
                ref_table: "organizations".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        };
        let prefixed = action.with_prefix("myapp_");
        if let MigrationAction::CreateTable {
            table, constraints, ..
        } = prefixed
        {
            assert_eq!(table.as_str(), "myapp_users");
            if let TableConstraint::ForeignKey { ref_table, .. } = &constraints[0] {
                assert_eq!(ref_table.as_str(), "myapp_organizations");
            }
        } else {
            panic!("Expected CreateTable");
        }
    }

    #[test]
    fn test_action_with_prefix_delete_table() {
        let action = MigrationAction::DeleteTable {
            table: "users".into(),
        };
        let prefixed = action.with_prefix("myapp_");
        if let MigrationAction::DeleteTable { table } = prefixed {
            assert_eq!(table.as_str(), "myapp_users");
        } else {
            panic!("Expected DeleteTable");
        }
    }

    #[test]
    fn test_action_with_prefix_add_column() {
        let action = MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(default_column()),
            fill_with: None,
        };
        let prefixed = action.with_prefix("myapp_");
        if let MigrationAction::AddColumn { table, .. } = prefixed {
            assert_eq!(table.as_str(), "myapp_users");
        } else {
            panic!("Expected AddColumn");
        }
    }

    #[test]
    fn test_action_with_prefix_rename_table() {
        let action = MigrationAction::RenameTable {
            from: "old_table".into(),
            to: "new_table".into(),
        };
        let prefixed = action.with_prefix("myapp_");
        if let MigrationAction::RenameTable { from, to } = prefixed {
            assert_eq!(from.as_str(), "myapp_old_table");
            assert_eq!(to.as_str(), "myapp_new_table");
        } else {
            panic!("Expected RenameTable");
        }
    }

    #[test]
    fn test_action_with_prefix_raw_sql_unchanged() {
        let action = MigrationAction::RawSql {
            sql: "SELECT * FROM users".into(),
        };
        let prefixed = action.with_prefix("myapp_");
        if let MigrationAction::RawSql { sql } = prefixed {
            // RawSql is not modified - user is responsible for table names
            assert_eq!(sql, "SELECT * FROM users");
        } else {
            panic!("Expected RawSql");
        }
    }

    #[test]
    fn test_action_with_prefix_empty_prefix() {
        let action = MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![],
            constraints: vec![],
        };
        let prefixed = action.clone().with_prefix("");
        if let MigrationAction::CreateTable { table, .. } = prefixed {
            assert_eq!(table.as_str(), "users");
        }
    }

    #[test]
    fn test_migration_plan_with_prefix() {
        let plan = MigrationPlan {
            comment: Some("test".into()),
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![],
                    constraints: vec![],
                },
                MigrationAction::CreateTable {
                    table: "posts".into(),
                    columns: vec![],
                    constraints: vec![TableConstraint::ForeignKey {
                        name: Some("fk_user".into()),
                        columns: vec!["user_id".into()],
                        ref_table: "users".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                    }],
                },
            ],
        };
        let prefixed = plan.with_prefix("myapp_");
        assert_eq!(prefixed.actions.len(), 2);

        if let MigrationAction::CreateTable { table, .. } = &prefixed.actions[0] {
            assert_eq!(table.as_str(), "myapp_users");
        }
        if let MigrationAction::CreateTable {
            table, constraints, ..
        } = &prefixed.actions[1]
        {
            assert_eq!(table.as_str(), "myapp_posts");
            if let TableConstraint::ForeignKey { ref_table, .. } = &constraints[0] {
                assert_eq!(ref_table.as_str(), "myapp_users");
            }
        }
    }
}
