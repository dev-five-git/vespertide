//! Fault **F3 Edge #1** - `AddColumn` action that participates in a
//! `FOREIGN KEY` must declare `nullable: true` whenever it also carries
//! a `fill_with` or `default` value.
//!
//! ## Why
//!
//! The F3 (FK with orphan rows) emit pipeline issues three statements when
//! a new column is born with both a default/fill value *and* a FK:
//!
//! 1. `ALTER TABLE t ADD COLUMN c TYPE DEFAULT <fill>;` - every existing
//!    row receives the fill value.
//! 2. `UPDATE t SET c = NULL WHERE c IS NOT NULL AND NOT EXISTS (SELECT 1
//!    FROM parent WHERE parent.pk = t.c);` - rows whose fill value is not
//!    present in the parent table get their reference NULL-ed out.
//! 3. `ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (c) REFERENCES
//!    parent(pk);` - the FK is finally added.
//!
//! Step (2) requires the column to be nullable. If the user declared
//! `nullable: false`, step (2) would fail on every row whose fill value
//! mismatches the parent, leaving the migration unrunnable.
//!
//! Vespertide does **not** silently lift `nullable` to `true` - schema
//! state is the user's promise to downstream consumers and cannot be
//! mutated implicitly. Instead, this check surfaces the conflict as a
//! [`PlannerError`] so the user fixes the model explicitly.
//!
//! ## Detection scope
//!
//! "Participates in a FK" means either:
//!
//! - The column carries an **inline** `foreign_key` declaration
//!   (`ColumnDef::foreign_key.is_some()`), or
//! - The same migration plan includes an `AddConstraint(ForeignKey)`
//!   whose `columns` contains this column.
//!
//! Both pathways produce the same runtime behaviour and so both are
//! rejected here.

use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint};

use crate::error::PlannerError;

/// Scan the plan for `AddColumn` actions that violate the F3 Edge #1
/// invariant.
///
/// Returns one [`PlannerError::AddColumnWithFkRequiresNullable`] per
/// offending column in plan-order. Empty when the plan is well-formed.
#[must_use]
pub fn find_addcolumn_fk_nullable_violations(plan: &MigrationPlan) -> Vec<PlannerError> {
    // Pre-compute the set of (table, column) pairs covered by an
    // `AddConstraint(ForeignKey)` in this plan. Used to detect the
    // out-of-line FK path.
    let mut fk_pairs: Vec<(String, String)> = Vec::new();
    for action in &plan.actions {
        if let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::ForeignKey { columns, .. },
        } = action
        {
            for col in columns {
                fk_pairs.push((table.to_string(), col.to_string()));
            }
        }
    }

    let mut errors = Vec::new();
    for action in &plan.actions {
        let MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } = action
        else {
            continue;
        };

        let has_value = fill_with.is_some() || column.default.is_some();
        if !has_value || column.nullable {
            continue;
        }

        let in_inline_fk = column.foreign_key.is_some();
        let in_paired_fk = fk_pairs
            .iter()
            .any(|(t, c)| t == table.as_str() && c == column.name.as_str());

        if in_inline_fk || in_paired_fk {
            errors.push(PlannerError::AddColumnWithFkRequiresNullable {
                table: table.to_string(),
                column: column.name.to_string(),
            });
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{col_int, plan};
    use rstest::rstest;
    use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
    use vespertide_core::{
        ColumnDef, ColumnName, MigrationAction, StringOrBool, TableConstraint, TableName,
    };

    fn add_column(table: &str, column: ColumnDef, fill: Option<&str>) -> MigrationAction {
        MigrationAction::AddColumn {
            table: TableName::from(table),
            column: Box::new(column),
            fill_with: fill.map(ToString::to_string),
        }
    }

    fn add_constraint_fk(
        table: &str,
        columns: &[&str],
        ref_table: &str,
        ref_cols: &[&str],
    ) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: TableName::from(table),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: columns.iter().map(|c| ColumnName::from(*c)).collect(),
                ref_table: TableName::from(ref_table),
                ref_columns: ref_cols.iter().map(|c| ColumnName::from(*c)).collect(),
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }
    }

    fn fk_def(ref_table: &str, ref_cols: &[&str]) -> ForeignKeySyntax {
        ForeignKeySyntax::Object(ForeignKeyDef {
            ref_table: ref_table.into(),
            ref_columns: ref_cols.iter().map(|c| ColumnName::from(*c)).collect(),
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        })
    }

    #[rstest]
    fn case_01_nullable_true_fill_inline_fk_ok() {
        // nullable: true + fill_with + inline FK -> no violation
        let mut c = col_int("user_id", true);
        c.foreign_key = Some(fk_def("users", &["id"]));
        let p = plan(vec![add_column("posts", c, Some("1"))]);
        assert!(find_addcolumn_fk_nullable_violations(&p).is_empty());
    }

    #[rstest]
    fn case_02_nullable_false_fill_inline_fk_violation() {
        // nullable: false + fill_with + inline FK -> violation
        let mut c = col_int("user_id", false);
        c.foreign_key = Some(fk_def("users", &["id"]));
        let p = plan(vec![add_column("posts", c, Some("1"))]);
        let errs = find_addcolumn_fk_nullable_violations(&p);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            PlannerError::AddColumnWithFkRequiresNullable { table, column }
                if table == "posts" && column == "user_id"
        ));
    }

    // An out-of-line FK on a DIFFERENT column of the same table must not flag
    // an unrelated AddColumn. The paired-FK match requires BOTH the table AND
    // the column to match; a `&& -> ||` mutant would match on the table alone
    // and wrongly raise a violation.
    #[rstest]
    fn paired_fk_on_other_column_does_not_flag_unrelated_addcolumn() {
        // AddColumn `a` (non-nullable, has fill, NO inline FK) on `posts`,
        // plus an out-of-line FK that covers a DIFFERENT column `b`.
        let c = col_int("a", false);
        let p = plan(vec![
            add_column("posts", c, Some("1")),
            add_constraint_fk("posts", &["b"], "users", &["id"]),
        ]);
        assert!(
            find_addcolumn_fk_nullable_violations(&p).is_empty(),
            "FK on column `b` must not implicate AddColumn `a`"
        );
    }

    #[rstest]
    fn case_03_nullable_false_default_inline_fk_violation() {
        // nullable: false + default + inline FK -> violation
        let mut c = col_int("user_id", false);
        c.default = Some(StringOrBool::String("1".into()));
        c.foreign_key = Some(fk_def("users", &["id"]));
        let p = plan(vec![add_column("posts", c, None)]);
        let errs = find_addcolumn_fk_nullable_violations(&p);
        assert_eq!(errs.len(), 1);
    }

    #[rstest]
    fn case_04_nullable_false_fill_no_fk_ok() {
        // nullable: false + fill_with + no FK -> not a F3 case, no violation
        let c = col_int("name", false);
        let p = plan(vec![add_column("posts", c, Some("'unknown'"))]);
        assert!(find_addcolumn_fk_nullable_violations(&p).is_empty());
    }

    #[rstest]
    fn case_05_paired_addconstraint_fk_violation() {
        // nullable: false + fill_with + separate AddConstraint(FK) -> violation
        let c = col_int("user_id", false);
        let p = plan(vec![
            add_column("posts", c, Some("1")),
            add_constraint_fk("posts", &["user_id"], "users", &["id"]),
        ]);
        let errs = find_addcolumn_fk_nullable_violations(&p);
        assert_eq!(errs.len(), 1);
    }

    #[rstest]
    fn case_06_paired_addconstraint_fk_nullable_ok() {
        // nullable: true + paired FK -> ok
        let c = col_int("user_id", true);
        let p = plan(vec![
            add_column("posts", c, Some("1")),
            add_constraint_fk("posts", &["user_id"], "users", &["id"]),
        ]);
        assert!(find_addcolumn_fk_nullable_violations(&p).is_empty());
    }

    #[rstest]
    fn case_07_nullable_false_no_fill_no_default_ok() {
        // nullable: false but no fill_with/default -> separate fault (F1
        // via find_missing_fill_with). Not F3 Edge #1.
        let mut c = col_int("user_id", false);
        c.foreign_key = Some(fk_def("users", &["id"]));
        let p = plan(vec![add_column("posts", c, None)]);
        assert!(find_addcolumn_fk_nullable_violations(&p).is_empty());
    }

    #[rstest]
    fn case_08_composite_fk_one_violating() {
        // composite FK over two new columns, one violating nullable invariant
        let bad = {
            let mut c = col_int("col_a", false);
            c.default = Some(StringOrBool::String("1".into()));
            c
        };
        let good = col_int("col_b", true);
        let p = plan(vec![
            add_column("posts", bad, None),
            add_column("posts", good, None),
            add_constraint_fk("posts", &["col_a", "col_b"], "users", &["a", "b"]),
        ]);
        let errs = find_addcolumn_fk_nullable_violations(&p);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            PlannerError::AddColumnWithFkRequiresNullable { column, .. } if column == "col_a"
        ));
    }
}
