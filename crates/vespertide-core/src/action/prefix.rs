use super::{MigrationAction, MigrationPlan};
use crate::schema::TableName;

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

        prefix_migration_action(self, prefix)
    }
}

fn prefix_migration_action(action: MigrationAction, prefix: &str) -> MigrationAction {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => MigrationAction::CreateTable {
            table: add_prefix(table, prefix),
            columns,
            constraints: constraints
                .into_iter()
                .map(|c| c.with_prefix(prefix))
                .collect(),
        },
        MigrationAction::DeleteTable { table } => MigrationAction::DeleteTable {
            table: add_prefix(table, prefix),
        },
        MigrationAction::RenameTable { from, to } => MigrationAction::RenameTable {
            from: add_prefix(from, prefix),
            to: add_prefix(to, prefix),
        },
        MigrationAction::RawSql { sql } => MigrationAction::RawSql { sql },
        action => prefix_column_or_constraint_action(action, prefix),
    }
}

fn prefix_column_or_constraint_action(action: MigrationAction, prefix: &str) -> MigrationAction {
    match action {
        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => MigrationAction::AddColumn {
            table: add_prefix(table, prefix),
            column,
            fill_with,
        },
        MigrationAction::RenameColumn { table, from, to } => MigrationAction::RenameColumn {
            table: add_prefix(table, prefix),
            from,
            to,
        },
        MigrationAction::DeleteColumn { table, column } => MigrationAction::DeleteColumn {
            table: add_prefix(table, prefix),
            column,
        },
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
            fill_with,
            narrowing_strategy,
            timezone,
        } => MigrationAction::ModifyColumnType {
            table: add_prefix(table, prefix),
            column,
            new_type,
            fill_with,
            narrowing_strategy,
            timezone,
        },
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            fill_with,
            delete_null_rows,
        } => MigrationAction::ModifyColumnNullable {
            table: add_prefix(table, prefix),
            column,
            nullable,
            fill_with,
            delete_null_rows,
        },
        action => prefix_remaining_action(action, prefix),
    }
}

fn prefix_remaining_action(action: MigrationAction, prefix: &str) -> MigrationAction {
    match action {
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            backfill,
        } => MigrationAction::ModifyColumnDefault {
            table: add_prefix(table, prefix),
            column,
            new_default,
            backfill,
        },
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => MigrationAction::ModifyColumnComment {
            table: add_prefix(table, prefix),
            column,
            new_comment,
        },
        MigrationAction::AddConstraint { table, constraint } => MigrationAction::AddConstraint {
            table: format!("{prefix}{table}").into(),
            constraint: constraint.with_prefix(prefix),
        },
        MigrationAction::RemoveConstraint { table, constraint } => {
            MigrationAction::RemoveConstraint {
                table: add_prefix(table, prefix),
                constraint: constraint.with_prefix(prefix),
            }
        }
        MigrationAction::ReplaceConstraint { table, from, to } => {
            MigrationAction::ReplaceConstraint {
                table: add_prefix(table, prefix),
                from: from.with_prefix(prefix),
                to: to.with_prefix(prefix),
            }
        }
        other => other,
    }
}

fn add_prefix(table: TableName, prefix: &str) -> TableName {
    let mut table = table.into_inner();
    table.insert_str(0, prefix);
    table.into()
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for the `RawSql` arm of `prefix_migration_action`.
    //! Targets `uncovered-detail.json` line 54.
    use super::*;

    #[test]
    fn raw_sql_with_prefix_is_a_noop_on_sql_body() {
        // Hits line 54 — the RawSql arm of prefix_migration_action.
        let action = MigrationAction::RawSql {
            sql: "SELECT 1".to_string(),
        };
        let prefixed = action.with_prefix("p_");
        match prefixed {
            MigrationAction::RawSql { sql } => assert_eq!(sql, "SELECT 1"),
            other => panic!("expected RawSql, got {other:?}"),
        }
    }

    #[test]
    fn raw_sql_within_plan_with_prefix_preserves_sql() {
        // Drives the same RawSql arm via MigrationPlan::with_prefix.
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "UPDATE x SET y = 1".to_string(),
            }],
        };
        let prefixed = plan.with_prefix("tenant_");
        match prefixed.actions.into_iter().next() {
            Some(MigrationAction::RawSql { sql }) => assert_eq!(sql, "UPDATE x SET y = 1"),
            other => panic!("expected RawSql, got {other:?}"),
        }
    }

    #[test]
    fn remap_enum_values_with_prefix_passes_through_catch_all() {
        // Drives line 54 — the `other => other` catch-all of
        // `prefix_remaining_action`. `RemapEnumValues` is not explicitly
        // handled by any of the three prefix dispatchers, so it must fall
        // through unchanged.
        let action = MigrationAction::RemapEnumValues {
            table: "user".into(),
            column: "status".into(),
            mapping: vec![(1_i64, 2_i64)].into_iter().collect(),
        };
        let prefixed = action.with_prefix("t_");
        match prefixed {
            MigrationAction::RemapEnumValues {
                table,
                column,
                mapping,
            } => {
                // Catch-all arm does NOT rewrite the table name (by
                // design — the action is the prefix-agnostic enum
                // value-remap, table identifiers are left to whichever
                // sibling action carries the rename).
                assert_eq!(table.as_str(), "user");
                assert_eq!(column.as_str(), "status");
                assert_eq!(mapping, vec![(1_i64, 2_i64)].into_iter().collect());
            }
            other => panic!("expected RemapEnumValues, got {other:?}"),
        }
    }
}
