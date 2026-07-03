//! Fault **F3** - `FOREIGN KEY` added to a column that may already
//! contain rows referencing non-existent parent values ("orphan rows").
//!
//! Mirrors the F2 (UNIQUE addition) detection pattern: an
//! `AddConstraint(ForeignKey)` whose every `columns` entry already
//! exists in the baseline schema is risky - production data may contain
//! references to parent rows that have since been deleted, and
//! `ALTER TABLE ... ADD CONSTRAINT FOREIGN KEY` would reject the
//! migration when even one orphan exists. Vespertide surfaces every
//! such addition so the CLI can prompt for an
//! [`ForeignKeyOrphanStrategy`] choice and stamp it back onto the
//! action's `TableConstraint::ForeignKey.orphan_strategy`.
//!
//! Specifically suppressed (never reported):
//!
//! - `CreateTable` constraints - table is brand new, no rows exist.
//! - `AddColumn` with inline `foreign_key` - column is brand new.
//! - `AddConstraint(ForeignKey)` whose **any** column is not yet present
//!   in the baseline. The composite-FK pathway that mixes new and
//!   existing columns is also skipped: F3 Edge #1
//!   (`fk_addcolumn_nullable`) covers the new-column variant.
//!
//! The detector is **purely static**: no DB access. The actual cleanup
//! SQL is emitted by `vespertide-query::sql::add_constraint::foreign_key`
//! based on the user-chosen `ForeignKeyOrphanStrategy`.
//!
//! [`ForeignKeyOrphanStrategy`]: vespertide_core::ForeignKeyOrphanStrategy

use std::collections::HashSet;

use vespertide_core::{ColumnName, MigrationAction, MigrationPlan, TableConstraint, TableDef};

/// One risky FK addition needing user resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FkOrphanAdditionWarning {
    /// Index of the `AddConstraint(ForeignKey)` action in the plan.
    pub action_index: usize,
    /// Table the FK is being added to (the child table).
    pub table: String,
    /// FK constraint name (when declared in the model).
    pub constraint_name: Option<String>,
    /// Child columns covered by the new FK, in declared order.
    pub columns: Vec<String>,
    /// Parent (referenced) table name.
    pub ref_table: String,
    /// Parent (referenced) columns, parallel to `columns`.
    pub ref_columns: Vec<String>,
    /// `true` when **every** child column in `columns` is nullable in
    /// the baseline. Drives the CLI's strategy menu: `NullifyOrphans`
    /// is only offered when this is `true`; otherwise only
    /// `DeleteOrphans` is valid.
    pub all_columns_nullable: bool,
}

/// Scan the plan for `AddConstraint(ForeignKey)` on baseline-existing
/// columns.
///
/// Returns warnings in plan-order. Empty when the plan adds no risky
/// FKs (or when every added FK only covers brand-new columns).
#[must_use]
pub fn find_fk_orphan_additions(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<FkOrphanAdditionWarning> {
    let mut out = Vec::new();

    for (idx, action) in plan.actions.iter().enumerate() {
        let MigrationAction::AddConstraint {
            table,
            constraint:
                TableConstraint::ForeignKey {
                    name,
                    columns,
                    ref_table,
                    ref_columns,
                    ..
                },
        } = action
        else {
            continue;
        };

        // Only flag when the child table itself exists in the baseline.
        // If the table is being created in this plan it has no rows yet
        // and no orphans can exist.
        let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str()) else {
            continue;
        };

        let baseline_columns: HashSet<&str> =
            table_def.columns.iter().map(|c| c.name.as_str()).collect();

        // Skip if any child column is brand new: orphan rows can only
        // exist when every column has been populated by prior INSERTs.
        // The mixed-new-and-existing case is uncommon and (when the
        // new column carries fill_with) belongs to F3 Edge #1.
        let all_existing = columns
            .iter()
            .all(|c| baseline_columns.contains(c.as_str()));
        if !all_existing {
            continue;
        }

        // Determine whether every child column is nullable in the
        // baseline. Used by the CLI to decide if `NullifyOrphans` is a
        // valid strategy choice for this warning.
        let all_columns_nullable = columns.iter().all(|c| {
            table_def
                .columns
                .iter()
                .find(|cd| cd.name.as_str() == c.as_str())
                .is_some_and(|cd| cd.nullable)
        });

        out.push(FkOrphanAdditionWarning {
            action_index: idx,
            table: table.to_string(),
            constraint_name: name.clone(),
            columns: columns_to_strings(columns),
            ref_table: ref_table.to_string(),
            ref_columns: columns_to_strings(ref_columns),
            all_columns_nullable,
        });
    }

    out
}

fn columns_to_strings(cols: &[ColumnName]) -> Vec<String> {
    cols.iter().map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableConstraint,
        TableDef, TableName,
    };

    fn col(name: &str, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(name: &str, cols: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: cols,
            constraints: vec![],
        }
    }

    fn add_fk(
        table: &str,
        name: Option<&str>,
        columns: &[&str],
        ref_table: &str,
        ref_cols: &[&str],
    ) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: TableName::from(table),
            constraint: TableConstraint::ForeignKey {
                name: name.map(ToString::to_string),
                columns: columns.iter().map(|c| (*c).into()).collect(),
                ref_table: TableName::from(ref_table),
                ref_columns: ref_cols.iter().map(|c| (*c).into()).collect(),
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }
    }

    fn plan(actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: "test".into(),
            version: 1,
            comment: None,
            created_at: None,
            actions,
        }
    }

    #[rstest]
    fn case_01_existing_nullable_column_flagged_with_nullify_available() {
        // Child column exists in baseline and is nullable -> warning + NullifyOrphans valid.
        let baseline = vec![table("posts", vec![col("id", false), col("user_id", true)])];
        let p = plan(vec![add_fk(
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
        )]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].table, "posts");
        assert_eq!(ws[0].columns, vec!["user_id".to_string()]);
        assert_eq!(ws[0].ref_table, "users");
        assert!(ws[0].all_columns_nullable);
    }

    #[rstest]
    fn case_02_existing_not_null_column_flagged_without_nullify() {
        // Child column exists in baseline but is NOT NULL -> warning + only DeleteOrphans valid.
        let baseline = vec![table(
            "posts",
            vec![col("id", false), col("user_id", false)],
        )];
        let p = plan(vec![add_fk("posts", None, &["user_id"], "users", &["id"])]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert!(!ws[0].all_columns_nullable);
    }

    #[rstest]
    fn case_03_new_column_skipped() {
        // FK references a column that doesn't yet exist in baseline -> no warning.
        let baseline = vec![table("posts", vec![col("id", false)])];
        let p = plan(vec![add_fk("posts", None, &["user_id"], "users", &["id"])]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_04_composite_fk_all_existing_flagged() {
        // Composite FK over two existing columns -> single warning, columns preserved in order.
        let baseline = vec![table(
            "audit",
            vec![
                col("id", false),
                col("team_id", true),
                col("member_id", true),
            ],
        )];
        let p = plan(vec![add_fk(
            "audit",
            Some("fk_team_member"),
            &["team_id", "member_id"],
            "teams",
            &["id", "member_id"],
        )]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(
            ws[0].columns,
            vec!["team_id".to_string(), "member_id".to_string()]
        );
        assert!(ws[0].all_columns_nullable);
    }

    #[rstest]
    fn case_05_composite_fk_mixed_existing_and_new_skipped() {
        // Composite FK with one new column -> Edge #1's responsibility, skipped here.
        let baseline = vec![table("audit", vec![col("id", false), col("team_id", true)])];
        let p = plan(vec![add_fk(
            "audit",
            None,
            &["team_id", "member_id"],
            "teams",
            &["id", "member_id"],
        )]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_06_composite_fk_mixed_nullability_records_false() {
        // One column nullable, the other NOT NULL -> all_columns_nullable = false.
        let baseline = vec![table(
            "audit",
            vec![
                col("id", false),
                col("team_id", true),
                col("member_id", false),
            ],
        )];
        let p = plan(vec![add_fk(
            "audit",
            None,
            &["team_id", "member_id"],
            "teams",
            &["id", "member_id"],
        )]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert!(!ws[0].all_columns_nullable);
    }

    #[rstest]
    fn case_07_self_referential_fk_flagged() {
        // FK referencing the same table - still a warning (parent_id may dangle).
        let baseline = vec![table(
            "category",
            vec![col("id", false), col("parent_id", true)],
        )];
        let p = plan(vec![add_fk(
            "category",
            Some("fk_parent"),
            &["parent_id"],
            "category",
            &["id"],
        )]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].table, ws[0].ref_table);
    }

    #[rstest]
    fn case_08_table_not_in_baseline_skipped() {
        // FK on a table that is being created in this plan -> no rows yet -> skip.
        let baseline: Vec<TableDef> = vec![];
        let p = plan(vec![add_fk("posts", None, &["user_id"], "users", &["id"])]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    /// Coverage-closure: a plan that contains non-FK `AddConstraint` /
    /// `DeleteTable` actions mixed with one real FK addition exercises the
    /// `let-else continue` skip arm (lines 78-81) for every non-matching
    /// action variant in addition to the warned one.
    #[rstest]
    fn case_09_mixed_plan_only_emits_fk_warning_and_skips_other_actions() {
        let baseline = vec![table("posts", vec![col("id", false), col("user_id", true)])];
        let p = plan(vec![
            // 0: AddConstraint Unique - not a FK, hits let-else continue
            MigrationAction::AddConstraint {
                table: "posts".into(),
                constraint: TableConstraint::Unique {
                    name: Some("uq".into()),
                    columns: vec!["id".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::default(),
                },
            },
            // 1: DeleteTable - not AddConstraint at all
            MigrationAction::DeleteTable {
                table: "old".into(),
            },
            // 2: AddConstraint FK on baseline-existing column - the real warning
            add_fk("posts", Some("fk"), &["user_id"], "users", &["id"]),
        ]);
        let ws = find_fk_orphan_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].action_index, 2);
    }

    /// Coverage-closure: ensure `columns_to_strings` produces the expected
    /// owned String list, including the empty-input edge case (defensive
    /// helper contract).
    #[rstest]
    fn case_10_columns_to_strings_helper_contract() {
        let empty: Vec<ColumnName> = vec![];
        assert!(columns_to_strings(&empty).is_empty());
        let some: Vec<ColumnName> = vec!["a".into(), "b".into()];
        assert_eq!(
            columns_to_strings(&some),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
