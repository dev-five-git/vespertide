//! Fault **F4** - `CHECK` constraint added to a table that may already
//! contain rows violating the predicate.
//!
//! Mirrors F2 (UNIQUE) and F3 (FK) detection patterns: an
//! `AddConstraint(Check)` whose expression matches a narrow recognisable
//! shape (the same shape understood by [`check_default`]) is risky
//! against a baseline-existing table - production data may violate the
//! new predicate, and `ALTER TABLE ... ADD CONSTRAINT CHECK (...)` would
//! reject the migration on the first offending row. Vespertide surfaces
//! every such risky addition so the CLI can prompt for a
//! [`CheckViolationStrategy`] choice and stamp it back onto the action's
//! `TableConstraint::Check.strategy`.
//!
//! Specifically suppressed (never reported):
//!
//! - `CreateTable` constraints - table is brand new, no rows exist.
//! - `AddConstraint(Check)` whose target table is not in the baseline -
//!   table is being created in this plan.
//! - `AddConstraint(Check)` whose expression is not in the narrow
//!   recognisable shape ([`parse_simple_check`] returns `None` for
//!   every baseline column). False-positive avoidance: a CHECK that
//!   vespertide cannot statically evaluate is left for the database
//!   to validate at apply time.
//!
//! The detector is **purely static**: no DB access. The actual cleanup
//! SQL is emitted by `vespertide-query::sql::add_constraint::check`
//! based on the user-chosen `CheckViolationStrategy`.
//!
//! [`check_default`]: super::check_default
//! [`parse_simple_check`]: super::check_expr_parser::parse
//! [`CheckViolationStrategy`]: vespertide_core::CheckViolationStrategy

use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};

use super::check_expr_parser::matches_for_column;

/// One risky CHECK addition needing user resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckAdditionWarning {
    /// Index of the `AddConstraint(Check)` action in the plan.
    pub action_index: usize,
    /// Table the CHECK is being added to.
    pub table: String,
    /// CHECK constraint name (always present - `TableConstraint::Check.name` is required).
    pub constraint_name: String,
    /// The CHECK expression text, as authored in the model.
    pub check_expr: String,
    /// Column the narrow-shape parser identified as the CHECK target.
    /// Narrow-shape CHECKs always reduce to a single column.
    pub target_column: String,
    /// `true` when the target column is nullable in the baseline.
    /// Drives the CLI's strategy menu: `NullifyViolatingColumn` is only
    /// offered when this is `true`; otherwise only `DeleteViolatingRows`
    /// is valid.
    pub target_column_nullable: bool,
}

/// Scan the plan for `AddConstraint(Check)` on baseline-existing tables
/// whose expression matches the narrow recognisable shape.
///
/// Returns warnings in plan-order. Empty when the plan adds no
/// statically analysable CHECKs (either because no CHECKs are added,
/// the target table is brand new, or every added CHECK uses a complex
/// expression vespertide cannot parse).
#[must_use]
pub fn find_check_additions(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<CheckAdditionWarning> {
    let mut out = Vec::new();

    for (idx, action) in plan.actions.iter().enumerate() {
        let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::Check { name, expr, .. },
        } = action
        else {
            continue;
        };

        // Skip when the target table is being created in this plan: a
        // brand-new table has no rows and so no violations are
        // possible.
        let Some(table_def) = baseline.iter().find(|t| t.name.as_str() == table.as_str()) else {
            continue;
        };

        // Narrow-shape parseable? Walk every column in the baseline and
        // ask `parse_simple_check` if the CHECK reduces to a
        // single-column form against it. The first matching column is
        // the target; narrow-shape CHECKs cannot match more than one.
        let Some((target_column, target_column_nullable)) =
            find_check_target_column(expr, table_def)
        else {
            // Unparseable expression: the database will catch any
            // violation at apply time. False-positive avoidance: do not
            // warn on complex expressions vespertide cannot evaluate.
            continue;
        };

        out.push(CheckAdditionWarning {
            action_index: idx,
            table: table.to_string(),
            constraint_name: name.clone(),
            check_expr: expr.clone(),
            target_column,
            target_column_nullable,
        });
    }

    out
}

/// Walk every column in `table` and return the first column for which
/// `matches_for_column` succeeds. Returns `(column_name, nullable)`.
fn find_check_target_column(expr: &str, table: &TableDef) -> Option<(String, bool)> {
    for col in &table.columns {
        if matches_for_column(expr, col.name.as_str()) {
            return Some((col.name.to_string(), col.nullable));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_check, col_int, plan};
    use rstest::rstest;
    use vespertide_core::{ColumnDef, TableDef};

    fn table(name: &str, cols: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: cols,
            constraints: vec![],
        }
    }

    #[rstest]
    fn case_01_simple_comparison_nullable_column_flagged() {
        let baseline = vec![table(
            "products",
            vec![col_int("id", false), col_int("price", true)],
        )];
        let p = plan(vec![add_check("products", "chk_positive", "price > 0")]);
        let ws = find_check_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].table, "products");
        assert_eq!(ws[0].constraint_name, "chk_positive");
        assert_eq!(ws[0].target_column, "price");
        assert!(ws[0].target_column_nullable);
    }

    #[rstest]
    fn case_02_simple_comparison_not_null_column_flagged_without_nullify() {
        let baseline = vec![table(
            "products",
            vec![col_int("id", false), col_int("price", false)],
        )];
        let p = plan(vec![add_check("products", "chk_positive", "price > 0")]);
        let ws = find_check_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert!(!ws[0].target_column_nullable);
    }

    #[rstest]
    fn case_03_in_clause_flagged() {
        let baseline = vec![table(
            "orders",
            vec![col_int("id", false), col_int("status", true)],
        )];
        let p = plan(vec![add_check(
            "orders",
            "chk_status",
            "status IN ('pending', 'paid', 'shipped')",
        )]);
        let ws = find_check_additions(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].target_column, "status");
    }

    #[rstest]
    fn case_04_unparseable_expression_skipped() {
        // Function calls / AND-OR / cross-column comparisons are
        // outside the narrow shape - skip silently to avoid false
        // positives.
        let baseline = vec![table(
            "audit",
            vec![col_int("id", false), col_int("a", true), col_int("b", true)],
        )];
        let p = plan(vec![add_check("audit", "chk_complex", "a > b")]);
        let ws = find_check_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_05_function_call_skipped() {
        let baseline = vec![table(
            "users",
            vec![col_int("id", false), col_int("name", true)],
        )];
        let p = plan(vec![add_check("users", "chk_name", "length(name) > 0")]);
        let ws = find_check_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_06_table_not_in_baseline_skipped() {
        // CreateTable in this plan -> no baseline rows -> skip.
        let baseline: Vec<TableDef> = vec![];
        let p = plan(vec![add_check("products", "chk_positive", "price > 0")]);
        let ws = find_check_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_07_target_column_not_in_baseline_skipped() {
        // CHECK references a column the baseline doesn't have - parser
        // returns None for every existing column, so warning is
        // suppressed. (Either F12 will catch the unknown column or the
        // DB will reject the constraint at apply time.)
        let baseline = vec![table("products", vec![col_int("id", false)])];
        let p = plan(vec![add_check("products", "chk_missing", "price > 0")]);
        let ws = find_check_additions(&p, &baseline);
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_08_multiple_checks_each_flagged_separately() {
        let baseline = vec![table(
            "products",
            vec![
                col_int("id", false),
                col_int("price", true),
                col_int("stock", true),
            ],
        )];
        let p = plan(vec![
            add_check("products", "chk_price", "price > 0"),
            add_check("products", "chk_stock", "stock >= 0"),
        ]);
        let ws = find_check_additions(&p, &baseline);
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].target_column, "price");
        assert_eq!(ws[1].target_column, "stock");
    }
}
