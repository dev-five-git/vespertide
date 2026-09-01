#![expect(
    clippy::doc_markdown,
    reason = "narrative prose: backend names (PostgreSQL, MySQL, SQLite) appear as plain words intentionally"
)]
//! Fault **F29** - CHECK expression strengthening detection.
//!
//! When a migration replaces a CHECK constraint with a *strictly
//! stricter* predicate, every existing row that satisfied the old
//! predicate but fails the new one would be silently rejected by the
//! database at `VALIDATE CONSTRAINT` time (PostgreSQL) or
//! `ADD CONSTRAINT` time (MySQL / SQLite). The migration plan looks
//! benign - a CHECK is just being swapped - but the actual apply may
//! fail half-way through, leaving the schema in an inconsistent state.
//!
//! This validator detects the strengthening *statically* by parsing
//! the old + new CHECK expression with the shared
//! [`super::check_expr_parser`] and comparing them with a
//! deliberately *conservative* strictness rule: a warning fires only
//! when the new predicate is demonstrably a *strict subset* of the
//! values accepted by the old one. Ambiguous, dialect-edged, or
//! semantically incomparable expressions silently pass (same policy
//! as F86 - false positives would block legitimate schema changes,
//! and any actual data-violation is caught by the database itself).
//!
//! # Matching sources
//!
//! F29 considers a CHECK to have been *replaced* when any of the
//! following holds, scoped per-`(table, constraint_name)`:
//!
//! 1. The plan contains a [`MigrationAction::ReplaceConstraint`] whose
//!    `from` and `to` are both `TableConstraint::Check` with the same
//!    name.
//! 2. The plan contains an [`MigrationAction::AddConstraint`] of a
//!    Check whose name matches an earlier `RemoveConstraint` of a
//!    Check on the same table (typical SQLite rebuild path).
//! 3. The plan contains an `AddConstraint(Check)` whose name matches
//!    an *existing* Check in the `baseline` schema (typical PG
//!    drop+add path where the diff produces a single add action).
//!
//! All three sources are checked in the order listed; the first
//! match wins.
//!
//! # Conservative strictness rules
//!
//! The comparator only returns "stricter" for these *demonstrable*
//! transitions. All other shapes return *not stricter* (no warning):
//!
//! - **Same column, same comparison operator, tighter literal**:
//!   `col > 0` -> `col > 10`, `col < 100` -> `col < 50`,
//!   `col >= 1` -> `col >= 5`, `col <= 100` -> `col <= 50`
//! - **Same column, operator boundary tightening with same literal**:
//!   `col >= N` -> `col > N`, `col <= N` -> `col < N`
//! - **IN list strict subset**:
//!   `col IN (a,b,c)` -> `col IN (a,b)` (set shrinks, no new values)
//! - **BETWEEN range narrows** (at least one boundary tightens, neither widens):
//!   `col BETWEEN 0 AND 100` -> `col BETWEEN 10 AND 90`
//! - **Conjunct added** (single -> AND with old as a part, or
//!   AND -> AND with extra conjuncts and all old conjuncts retained):
//!   `col > 0` -> `col > 0 AND col < 100`
//! - **Disjunct removed** (OR shrinks; all new disjuncts already
//!   present in old):
//!   `col = 'a' OR col = 'b' OR col = 'c'` -> `col = 'a' OR col = 'b'`
//!
//! Out of scope (silent pass):
//! - Mixed-type literals (string compared to integer, etc.)
//! - Changes that swap the column being constrained
//! - Operator changes that aren't strict boundary tightening
//!   (e.g. `=` -> `>`)
//! - Anything either parser returns as `Unparseable`

use std::cmp::Ordering;
use std::collections::HashMap;

use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};

use super::check_expr_parser::{CheckExpr, Literal, Op, parse};

/// Classification of *how* the new CHECK is stricter than the old.
/// Multiple kinds may apply in principle; the comparator reports the
/// most specific one detected first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStrengtheningKind {
    /// Same operator, tighter literal (e.g. `> 0` -> `> 10`).
    BoundaryTightened,
    /// Operator boundary tightened with same literal (e.g. `>=` -> `>`).
    OperatorTightened,
    /// `IN (...)` list shrunk (new is strict subset of old).
    InListShrunk,
    /// `BETWEEN ... AND ...` range narrowed.
    BetweenNarrowed,
    /// Extra `AND` conjunct added (old predicate retained, plus more).
    ConjunctAdded,
    /// `OR` disjunct removed (new is strict subset of old branches).
    DisjunctRemoved,
}

/// One CHECK strengthening site needing user confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStrengtheningWarning {
    /// Plan-action index of the triggering `AddConstraint` or
    /// `ReplaceConstraint`.
    pub action_index: usize,
    /// Table the CHECK lives on.
    pub table: String,
    /// Constraint name (the matching key between old and new).
    pub constraint_name: String,
    /// Verbatim old expression as it appeared in the previous schema
    /// (baseline replay or in-plan `RemoveConstraint` / `from`).
    pub old_expr: String,
    /// Verbatim new expression as it appears in the plan action.
    pub new_expr: String,
    /// Which strictness shape fired.
    pub kind: CheckStrengtheningKind,
}

/// Scan `plan` against `baseline` for CHECK strengthenings.
///
/// Returns warnings in plan-order. Empty when the plan introduces no
/// strictly-stricter CHECK replacements.
#[must_use]
pub fn find_check_strengthenings(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<CheckStrengtheningWarning> {
    let baseline_checks = build_baseline_check_map(baseline);
    // Running `(table, constraint_name) -> expr` map of Checks removed by an
    // *earlier* action in the plan (module doc: an AddConstraint only pairs
    // with a preceding RemoveConstraint, so an Add-before-Remove plan whose
    // final state is "constraint removed" never warns).
    let mut removed_in_plan: HashMap<(&str, &str), &str> = HashMap::new();

    let mut out = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        match action {
            MigrationAction::RemoveConstraint {
                table,
                constraint: TableConstraint::Check { name, expr, .. },
            } => {
                removed_in_plan.insert((table.as_str(), name.as_str()), expr.as_str());
            }
            MigrationAction::ReplaceConstraint {
                table,
                from:
                    TableConstraint::Check {
                        name: from_name,
                        expr: from_expr,
                        ..
                    },
                to:
                    TableConstraint::Check {
                        name: to_name,
                        expr: to_expr,
                        ..
                    },
                ..
            } if from_name == to_name => {
                if let Some(kind) = classify_strengthening(from_expr, to_expr) {
                    out.push(CheckStrengtheningWarning {
                        action_index: idx,
                        table: table.to_string(),
                        constraint_name: to_name.clone(),
                        old_expr: from_expr.clone(),
                        new_expr: to_expr.clone(),
                        kind,
                    });
                }
            }
            MigrationAction::AddConstraint {
                table,
                constraint:
                    TableConstraint::Check {
                        name,
                        expr: new_expr,
                        ..
                    },
            } => {
                let key = (table.as_str(), name.as_str());
                // Source 2: same-plan RemoveConstraint wins over baseline
                // because it represents the user's *explicit* intent in
                // this plan, while baseline match is inferred.
                let old_expr_opt = removed_in_plan
                    .get(&key)
                    .or_else(|| baseline_checks.get(&key))
                    .copied();
                let Some(old_expr) = old_expr_opt else {
                    continue;
                };
                if let Some(kind) = classify_strengthening(old_expr, new_expr) {
                    out.push(CheckStrengtheningWarning {
                        action_index: idx,
                        table: table.to_string(),
                        constraint_name: name.clone(),
                        old_expr: old_expr.to_string(),
                        new_expr: new_expr.clone(),
                        kind,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Borrow-keyed `(table, constraint_name) -> expr` map over the baseline.
/// Keys and values borrow the input; owned strings are produced only when
/// a warning actually fires.
fn build_baseline_check_map(baseline: &[TableDef]) -> HashMap<(&str, &str), &str> {
    let mut out = HashMap::new();
    for table in baseline {
        for constraint in &table.constraints {
            if let TableConstraint::Check { name, expr, .. } = constraint {
                out.insert((table.name.as_str(), name.as_str()), expr.as_str());
            }
        }
    }
    out
}

/// Classify whether `new_expr_str` is *demonstrably* stricter than
/// `old_expr_str`. Returns `None` for ambiguous, equal, or
/// non-stricter pairs.
fn classify_strengthening(
    old_expr_str: &str,
    new_expr_str: &str,
) -> Option<CheckStrengtheningKind> {
    if old_expr_str.trim() == new_expr_str.trim() {
        return None;
    }
    // Parse old first and short-circuit: when the old CHECK is outside
    // the recognized grammar the classification is `None` regardless of
    // the new expression, so its lex+parse is skipped entirely.
    let old = parse(old_expr_str);
    if matches!(old, CheckExpr::Unparseable) {
        return None;
    }
    let new = parse(new_expr_str);
    if matches!(new, CheckExpr::Unparseable) {
        return None;
    }
    if old == new {
        return None;
    }
    classify_pair(&old, &new)
}

fn classify_pair(old: &CheckExpr, new: &CheckExpr) -> Option<CheckStrengtheningKind> {
    match (old, new) {
        (
            CheckExpr::Compare {
                column: c1,
                op: op1,
                value: v1,
            },
            CheckExpr::Compare {
                column: c2,
                op: op2,
                value: v2,
            },
        ) if c1 == c2 => compare_strictness(*op1, v1, *op2, v2),
        (
            CheckExpr::In {
                column: c1,
                values: vs1,
                negated: false,
            },
            CheckExpr::In {
                column: c2,
                values: vs2,
                negated: false,
            },
        ) if c1 == c2 && in_is_strict_subset(vs2, vs1) => {
            Some(CheckStrengtheningKind::InListShrunk)
        }
        (
            CheckExpr::Between {
                column: c1,
                low: l1,
                high: h1,
                negated: false,
            },
            CheckExpr::Between {
                column: c2,
                low: l2,
                high: h2,
                negated: false,
            },
        ) if c1 == c2 && between_is_narrower(l1, h1, l2, h2) => {
            Some(CheckStrengtheningKind::BetweenNarrowed)
        }
        // Old single predicate; new is AND containing old as a part.
        (old_atom, CheckExpr::And(new_parts))
            if !matches!(old_atom, CheckExpr::And(_))
                && new_parts.len() >= 2
                && new_parts.iter().any(|p| p == old_atom) =>
        {
            Some(CheckStrengtheningKind::ConjunctAdded)
        }
        // Both AND: every old conjunct present in new + at least one new conjunct.
        (CheckExpr::And(old_parts), CheckExpr::And(new_parts))
            if new_parts.len() > old_parts.len()
                && old_parts
                    .iter()
                    .all(|op| new_parts.iter().any(|np| np == op)) =>
        {
            Some(CheckStrengtheningKind::ConjunctAdded)
        }
        // OR shrinks: every new disjunct already in old, and at least one removed.
        (CheckExpr::Or(old_parts), CheckExpr::Or(new_parts))
            if !new_parts.is_empty()
                && new_parts.len() < old_parts.len()
                && new_parts
                    .iter()
                    .all(|np| old_parts.iter().any(|op| op == np)) =>
        {
            Some(CheckStrengtheningKind::DisjunctRemoved)
        }
        _ => None,
    }
}

/// Strict-than for `Compare` predicates with matching column.
///
/// Returns `Some(BoundaryTightened)` for same-op tighter literal,
/// `Some(OperatorTightened)` for boundary operator strengthening with
/// equal literal, and `None` otherwise (including identical pairs
/// and incomparable types).
fn compare_strictness(
    op1: Op,
    v1: &Literal,
    op2: Op,
    v2: &Literal,
) -> Option<CheckStrengtheningKind> {
    if op1 == op2 && v1.approx_eq(v2) {
        return None; // identical
    }
    // Same operator family, tighter literal:
    if op1 == op2 {
        let cmp = v1.cmp_value(v2)?;
        let tighter = match op1 {
            Op::Gt | Op::Ge => cmp == Ordering::Less, // newer literal is larger
            Op::Lt | Op::Le => cmp == Ordering::Greater, // newer literal is smaller
            // Eq with different literal = different set (not stricter, just other).
            // Ne with different literal = different exclusion (not necessarily stricter).
            _ => false,
        };
        if tighter {
            return Some(CheckStrengtheningKind::BoundaryTightened);
        }
        return None;
    }
    // Boundary operator tightening with same literal:
    if v1.approx_eq(v2) {
        match (op1, op2) {
            (Op::Ge, Op::Gt) | (Op::Le, Op::Lt) => {
                return Some(CheckStrengtheningKind::OperatorTightened);
            }
            _ => {}
        }
    }
    None
}

/// True when `subset` is a strict subset of `superset` by literal
/// equality. Order-insensitive.
fn in_is_strict_subset(subset: &[Literal], superset: &[Literal]) -> bool {
    if subset.len() >= superset.len() {
        return false;
    }
    if subset.is_empty() {
        // `IN ()` is rejected at parse time; can't happen, but guard.
        return false;
    }
    subset
        .iter()
        .all(|s| superset.iter().any(|o| s.approx_eq(o)))
}

/// True when the new BETWEEN range is strictly inside the old range:
/// new_low >= old_low AND new_high <= old_high, with at least one
/// strict inequality.
fn between_is_narrower(
    old_low: &Literal,
    old_high: &Literal,
    new_low: &Literal,
    new_high: &Literal,
) -> bool {
    let Some(lo_cmp) = old_low.cmp_value(new_low) else {
        return false;
    };
    let Some(hi_cmp) = old_high.cmp_value(new_high) else {
        return false;
    };
    // old_low <= new_low (new low is tighter or equal)
    let lo_ok = matches!(lo_cmp, Ordering::Less | Ordering::Equal);
    // old_high >= new_high (new high is tighter or equal)
    let hi_ok = matches!(hi_cmp, Ordering::Greater | Ordering::Equal);
    let any_strict = lo_cmp == Ordering::Less || hi_cmp == Ordering::Greater;
    lo_ok && hi_ok && any_strict
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_check, check, plan};
    use vespertide_core::{MigrationAction, TableDef};

    fn baseline_with_check(table: &str, name: &str, expr: &str) -> Vec<TableDef> {
        vec![TableDef {
            name: table.into(),
            description: None,
            columns: Vec::new(),
            constraints: vec![check(name, expr)],
        }]
    }

    fn remove_check(table: &str, name: &str, expr: &str) -> MigrationAction {
        MigrationAction::RemoveConstraint {
            table: table.into(),
            constraint: check(name, expr),
        }
    }

    fn replace_check(table: &str, name: &str, from_expr: &str, to_expr: &str) -> MigrationAction {
        MigrationAction::ReplaceConstraint {
            table: table.into(),
            from: check(name, from_expr),
            to: check(name, to_expr),
        }
    }

    // -- Boundary tightening (same op, tighter literal) ------------------

    #[test]
    fn gt_boundary_tightened_via_baseline_match() {
        let baseline = baseline_with_check("users", "chk_age", "age > 0");
        let p = plan(vec![add_check("users", "chk_age", "age > 18")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
        assert_eq!(warnings[0].constraint_name, "chk_age");
        assert_eq!(warnings[0].old_expr, "age > 0");
        assert_eq!(warnings[0].new_expr, "age > 18");
    }

    #[test]
    fn lt_boundary_tightened() {
        let baseline = baseline_with_check("orders", "chk_amount", "amount < 1000");
        let p = plan(vec![add_check("orders", "chk_amount", "amount < 500")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    // -- classify_pair AND/OR set logic boundary kills ---------------------

    fn classify(old: &str, new: &str) -> Vec<CheckStrengtheningKind> {
        let baseline = baseline_with_check("t", "chk", old);
        let p = plan(vec![add_check("t", "chk", new)]);
        find_check_strengthenings(&p, &baseline)
            .into_iter()
            .map(|w| w.kind)
            .collect()
    }

    #[test]
    fn and_conjunct_added_is_strengthening() {
        assert_eq!(
            classify("a > 0 AND b > 0", "a > 0 AND b > 0 AND c > 0"),
            vec![CheckStrengtheningKind::ConjunctAdded]
        );
    }

    #[test]
    fn and_reordered_same_conjuncts_is_not_strengthening() {
        // Equal length, same set, reordered -> NOT a strengthening. Pins
        // `new.len() > old.len()` (a `>=` mutant would fire on equal length).
        assert!(classify("a > 0 AND b > 0", "b > 0 AND a > 0").is_empty());
    }

    #[test]
    fn and_added_non_overlapping_conjuncts_is_not_strengthening() {
        // New is longer but does NOT contain every old conjunct -> not a
        // pure addition. Pins the `&&` (a `||` mutant would fire on length
        // alone) and the `np == op` all-present check (a `!=` mutant would
        // make all-present trivially true).
        assert!(classify("a > 0 AND b > 0", "a > 0 AND c > 0 AND d > 0").is_empty());
    }

    #[test]
    fn or_disjunct_removed_is_strengthening() {
        assert_eq!(
            classify("a > 0 OR b > 0 OR c > 0", "a > 0 OR b > 0"),
            vec![CheckStrengtheningKind::DisjunctRemoved]
        );
    }

    #[test]
    fn or_reordered_same_disjuncts_is_not_strengthening() {
        // Equal length, reordered -> not a removal. Pins `new.len() < old.len()`.
        assert!(classify("a > 0 OR b > 0", "b > 0 OR a > 0").is_empty());
    }

    #[test]
    fn or_new_disjunct_not_in_old_is_not_strengthening() {
        // Fewer disjuncts but one is brand new (not in old) -> not a pure
        // removal. Pins the `&&` and the `op == np` subset check.
        assert!(classify("a > 0 OR b > 0 OR c > 0", "a > 0 OR d > 0").is_empty());
    }

    // -- between_is_narrower boundary kills --------------------------------

    #[test]
    fn between_tighten_low_only_is_narrower() {
        // [0,10] -> [5,10]: low tightened, high equal. Pins `lo_cmp ==
        // Ordering::Less` in any_strict (a `!=` mutant drops the only strict
        // inequality, making it report "not narrower").
        assert_eq!(
            classify("x BETWEEN 0 AND 10", "x BETWEEN 5 AND 10"),
            vec![CheckStrengtheningKind::BetweenNarrowed]
        );
    }

    #[test]
    fn between_tighten_high_only_is_narrower() {
        // [0,10] -> [0,8]: high tightened, low equal. Pins `hi_cmp ==
        // Ordering::Greater` in any_strict (a `!=` mutant drops the only
        // strict inequality, making it report "not narrower").
        assert_eq!(
            classify("x BETWEEN 0 AND 10", "x BETWEEN 0 AND 8"),
            vec![CheckStrengtheningKind::BetweenNarrowed]
        );
    }

    #[test]
    fn between_widened_low_tightened_high_is_not_narrower() {
        // [5,10] -> [0,8]: low widened (lo_ok=false), high tightened. Range
        // is NOT a subset, so not narrower. Pins both `&&`s in
        // `lo_ok && hi_ok && any_strict` (either `||` mutant would wrongly
        // report narrowing from the high-only tightening).
        assert!(classify("x BETWEEN 5 AND 10", "x BETWEEN 0 AND 8").is_empty());
    }

    // -- literal_equals EPSILON-boundary kills -----------------------------
    // `1.0000000000000002` is exactly `1.0 + f64::EPSILON`. A literal exactly
    // EPSILON away is DISTINCT under the strict `(a-b).abs() < EPSILON`
    // tolerance, so the bound genuinely tightened (the `<=` mutant would call
    // the two literals equal and suppress the warning).

    #[test]
    fn float_boundary_one_epsilon_tighter_is_tightening() {
        assert_eq!(
            classify("x > 1.0", "x > 1.0000000000000002"),
            vec![CheckStrengtheningKind::BoundaryTightened]
        );
    }

    #[test]
    fn int_vs_float_boundary_one_epsilon_tighter_is_tightening() {
        assert_eq!(
            classify("x > 1", "x > 1.0000000000000002"),
            vec![CheckStrengtheningKind::BoundaryTightened]
        );
    }

    #[test]
    fn float_vs_int_boundary_one_epsilon_apart_is_not_operator_tightening() {
        // `>= 1.0000000000000002` vs `> 1`: literals differ by exactly EPSILON,
        // so this is NOT the (Ge,Gt)-same-literal operator-tightening pattern.
        // Pins the (Float,Integer) EPSILON arm of literal_equals.
        assert!(classify("x >= 1.0000000000000002", "x > 1").is_empty());
    }

    #[test]
    fn ge_boundary_tightened() {
        let baseline = baseline_with_check("t", "c", "x >= 1");
        let p = plan(vec![add_check("t", "c", "x >= 10")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    #[test]
    fn boundary_loosened_emits_no_warning() {
        let baseline = baseline_with_check("users", "chk_age", "age > 18");
        let p = plan(vec![add_check("users", "chk_age", "age > 0")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn equal_predicate_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "x > 0");
        let p = plan(vec![add_check("t", "c", "x > 0")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    // -- Operator tightening (boundary >= -> >, <= -> <) -----------------

    #[test]
    fn ge_to_gt_with_same_literal_is_operator_tightened() {
        let baseline = baseline_with_check("t", "c", "x >= 0");
        let p = plan(vec![add_check("t", "c", "x > 0")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::OperatorTightened);
    }

    #[test]
    fn le_to_lt_with_same_literal_is_operator_tightened() {
        let baseline = baseline_with_check("t", "c", "x <= 100");
        let p = plan(vec![add_check("t", "c", "x < 100")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::OperatorTightened);
    }

    #[test]
    fn gt_to_ge_with_same_literal_is_loosening() {
        let baseline = baseline_with_check("t", "c", "x > 0");
        let p = plan(vec![add_check("t", "c", "x >= 0")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    // -- IN list shrunk --------------------------------------------------

    #[test]
    fn in_list_strict_subset_warns() {
        let baseline = baseline_with_check("t", "c", "s IN ('a', 'b', 'c')");
        let p = plan(vec![add_check("t", "c", "s IN ('a', 'b')")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::InListShrunk);
    }

    #[test]
    fn in_list_unchanged_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "s IN ('a', 'b')");
        let p = plan(vec![add_check("t", "c", "s IN ('a', 'b')")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn in_list_added_value_emits_no_warning() {
        // Adding a value is loosening, not strengthening.
        let baseline = baseline_with_check("t", "c", "s IN ('a', 'b')");
        let p = plan(vec![add_check("t", "c", "s IN ('a', 'b', 'c')")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn in_list_swapped_values_emits_no_warning() {
        // Replacing 'c' with 'd' is *not* a subset relationship.
        let baseline = baseline_with_check("t", "c", "s IN ('a', 'b', 'c')");
        let p = plan(vec![add_check("t", "c", "s IN ('a', 'b', 'd')")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    // -- BETWEEN narrowed ------------------------------------------------

    #[test]
    fn between_narrowed_on_both_sides_warns() {
        let baseline = baseline_with_check("t", "c", "x BETWEEN 0 AND 100");
        let p = plan(vec![add_check("t", "c", "x BETWEEN 10 AND 90")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BetweenNarrowed);
    }

    #[test]
    fn between_narrowed_on_one_side_warns() {
        let baseline = baseline_with_check("t", "c", "x BETWEEN 0 AND 100");
        let p = plan(vec![add_check("t", "c", "x BETWEEN 10 AND 100")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BetweenNarrowed);
    }

    #[test]
    fn between_widened_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "x BETWEEN 10 AND 90");
        let p = plan(vec![add_check("t", "c", "x BETWEEN 0 AND 100")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    // -- Conjunct added (AND composition) --------------------------------

    #[test]
    fn single_predicate_to_and_with_old_as_part_warns() {
        let baseline = baseline_with_check("t", "c", "age > 0");
        let p = plan(vec![add_check("t", "c", "age > 0 AND age < 150")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::ConjunctAdded);
    }

    #[test]
    fn and_to_and_with_extra_conjunct_warns() {
        let baseline = baseline_with_check("t", "c", "age > 0 AND age < 150");
        let p = plan(vec![add_check(
            "t",
            "c",
            "age > 0 AND age < 150 AND age <> 42",
        )]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::ConjunctAdded);
    }

    #[test]
    fn and_to_unrelated_and_emits_no_warning() {
        // Old conjunct `age > 0` not preserved; can't conclude stricter.
        let baseline = baseline_with_check("t", "c", "age > 0 AND age < 150");
        let p = plan(vec![add_check("t", "c", "age > 50 AND age < 150")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        // Even though it's actually stricter, we conservatively skip
        // (we'd need per-conjunct strictness check which is out of
        // scope for the v1 comparator).
        assert!(warnings.is_empty());
    }

    // -- Disjunct removed (OR shrinks) -----------------------------------

    #[test]
    fn or_with_one_branch_removed_warns() {
        let baseline = baseline_with_check("t", "c", "s = 'a' OR s = 'b' OR s = 'c'");
        let p = plan(vec![add_check("t", "c", "s = 'a' OR s = 'b'")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::DisjunctRemoved);
    }

    #[test]
    fn or_with_extra_branch_emits_no_warning() {
        // Adding a disjunct is loosening.
        let baseline = baseline_with_check("t", "c", "s = 'a' OR s = 'b'");
        let p = plan(vec![add_check("t", "c", "s = 'a' OR s = 'b' OR s = 'c'")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    // -- ReplaceConstraint path ------------------------------------------

    #[test]
    fn replace_constraint_with_stricter_to_warns() {
        let p = plan(vec![replace_check("t", "c", "age > 0", "age > 18")]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    #[test]
    fn replace_constraint_with_different_names_skipped() {
        // from/to with different names = treated as drop+add of two
        // different constraints, not a replacement of one.
        let p = plan(vec![MigrationAction::ReplaceConstraint {
            table: "t".into(),
            from: check("chk_old", "age > 0"),
            to: check("chk_new", "age > 18"),
        }]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert!(warnings.is_empty());
    }

    // -- Same-plan Remove + Add path -------------------------------------

    #[test]
    fn remove_then_add_with_stricter_warns() {
        let p = plan(vec![
            remove_check("t", "chk_age", "age > 0"),
            add_check("t", "chk_age", "age > 18"),
        ]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    /// Action order matters: an `AddConstraint` only pairs with an *earlier*
    /// `RemoveConstraint` (module doc, source 2). Add-before-Remove means the
    /// plan's final state is "constraint removed" — no strengthening warning.
    #[test]
    fn add_then_remove_same_name_does_not_warn() {
        let p = plan(vec![
            add_check("t", "chk_age", "age > 18"),
            remove_check("t", "chk_age", "age > 0"),
        ]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert!(warnings.is_empty());
    }

    // -- Conservative behaviour ------------------------------------------

    #[test]
    fn unparseable_old_silently_passes() {
        let baseline = baseline_with_check("t", "c", "LENGTH(name) > 0");
        let p = plan(vec![add_check("t", "c", "name > 'aaa'")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn unparseable_new_silently_passes() {
        let baseline = baseline_with_check("t", "c", "x > 0");
        let p = plan(vec![add_check("t", "c", "LOWER(x) = 'foo'")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn different_columns_skipped() {
        // The columns being constrained differ; can't conclude stricter.
        let baseline = baseline_with_check("t", "c", "age > 0");
        let p = plan(vec![add_check("t", "c", "weight > 100")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn type_mismatch_literal_skipped() {
        // String compared to integer-typed literal — can't order.
        let baseline = baseline_with_check("t", "c", "s = 'a'");
        let p = plan(vec![add_check("t", "c", "s = 1")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        // The pair is Eq with different *types* — neither stricter
        // nor BoundaryTightened applies.
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_baseline_match_and_no_plan_remove_skipped() {
        // AddConstraint with no corresponding baseline or same-plan
        // RemoveConstraint = a fresh CHECK addition (F4 territory),
        // not strengthening.
        let p = plan(vec![add_check("t", "chk_new", "age > 18")]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn float_literal_boundary_tightened() {
        let baseline = baseline_with_check("t", "c", "ratio > 0.1");
        let p = plan(vec![add_check("t", "c", "ratio > 0.5")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    // ── Coverage-closure: BETWEEN / OR / AND deeper paths ──────────────

    /// Both BETWEEN boundaries narrowed via in-plan Remove+Add (covers
    /// `between_is_narrower` true path under remove_then_add source).
    #[test]
    fn between_narrowed_via_remove_then_add_warns() {
        let p = plan(vec![
            remove_check("t", "chk_r", "x BETWEEN 0 AND 100"),
            add_check("t", "chk_r", "x BETWEEN 10 AND 90"),
        ]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BetweenNarrowed);
    }

    /// `between_is_narrower` returns false when only the low boundary is
    /// loosened (new_low < old_low) — silent pass.
    #[test]
    fn between_with_loosened_low_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "x BETWEEN 10 AND 100");
        let p = plan(vec![add_check("t", "c", "x BETWEEN 0 AND 100")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// IN list strict subset via Replace path (covers IN arm under
    /// ReplaceConstraint).
    #[test]
    fn in_list_shrunk_via_replace() {
        let p = plan(vec![replace_check(
            "t",
            "c",
            "s IN ('a', 'b', 'c')",
            "s IN ('a', 'b')",
        )]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::InListShrunk);
    }

    /// `OR` shrinks via Replace — covers DisjunctRemoved arm via
    /// ReplaceConstraint.
    #[test]
    fn or_disjunct_removed_via_replace() {
        let p = plan(vec![replace_check(
            "t",
            "c",
            "s = 'a' OR s = 'b' OR s = 'c'",
            "s = 'a' OR s = 'b'",
        )]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::DisjunctRemoved);
    }

    /// AND-to-AND conjunct addition via Replace — exercises the
    /// `CheckExpr::And(old_parts), CheckExpr::And(new_parts)` arm.
    #[test]
    fn and_to_and_conjunct_added_via_replace() {
        let p = plan(vec![replace_check(
            "t",
            "c",
            "age > 0 AND age < 150",
            "age > 0 AND age < 150 AND age <> 42",
        )]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::ConjunctAdded);
    }

    /// `OperatorTightened` via remove+add path with `<= → <` form.
    #[test]
    fn operator_tightened_le_to_lt_via_remove_add() {
        let p = plan(vec![
            remove_check("t", "c", "x <= 100"),
            add_check("t", "c", "x < 100"),
        ]);
        let warnings = find_check_strengthenings(&p, &[]);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::OperatorTightened);
    }

    /// IN list reduced to empty — guard at `subset.is_empty()` in
    /// `in_is_strict_subset` returns false (silent pass).
    /// SQL doesn't allow empty IN, so we approximate via a single-value
    /// IN that's not a strict subset under our semantics.
    #[test]
    fn in_list_singleton_to_disjoint_singleton_does_not_warn() {
        let baseline = baseline_with_check("t", "c", "s IN ('a')");
        let p = plan(vec![add_check("t", "c", "s IN ('b')")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// Equal expressions after trim — early return at `old.trim() ==
    /// new.trim()` in classify_strengthening.
    #[test]
    fn whitespace_only_difference_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "age > 0");
        let p = plan(vec![add_check("t", "c", "  age > 0  ")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// Old single-atom → new AND that does NOT contain the old atom
    /// (no warning, conservative).
    #[test]
    fn single_predicate_to_unrelated_and_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "age > 0");
        let p = plan(vec![add_check("t", "c", "weight > 100 AND height < 200")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// `OR` shrunk to empty — guard `!new_parts.is_empty()` defends.
    /// Reproduce by going OR-to-single-atom: not detected as DisjunctRemoved
    /// because the new shape is no longer an OR.
    #[test]
    fn or_collapsed_to_single_atom_emits_no_warning() {
        let baseline = baseline_with_check("t", "c", "s = 'a' OR s = 'b'");
        let p = plan(vec![add_check("t", "c", "s = 'a'")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    // ── Coverage-closure W3 (final 15 uncovered lines) ────────────────

    /// L184: `if old == new { return None; }` — exprs differ textually
    /// (so the L175 trim short-circuit doesn't fire) but parse to the
    /// SAME `CheckExpr` tree. `"age > 0"` and `"age>0"` trim to
    /// different strings yet parse identically.
    #[test]
    fn whitespace_internal_difference_parses_equal_no_warning() {
        let baseline = baseline_with_check("t", "c", "age > 0");
        let p = plan(vec![add_check("t", "c", "age>0")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// L212: `if op1 == op2 && literal_equals(v1, v2) { return None; }`
    /// — same op, derived `!=` on Literal but `literal_equals` returns
    /// true via the epsilon/cross-numeric arms. `Integer(0)` vs
    /// `Float(0.0)` satisfies both conditions.
    #[test]
    fn int_vs_float_literal_equal_no_warning() {
        let baseline = baseline_with_check("t", "c", "x > 0");
        let p = plan(vec![add_check("t", "c", "x > 0.0")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// L222: `_ => false,` in compare_strictness same-op match —
    /// `Eq`/`Ne` ops do not satisfy the boundary tightening rule even
    /// when the literal ordering is `Less`/`Greater`.
    #[test]
    fn eq_with_different_literals_does_not_warn() {
        let baseline = baseline_with_check("t", "c", "x = 5");
        let p = plan(vec![add_check("t", "c", "x = 10")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// L259: `let Some(lo_cmp) = literal_compare(...) else { return false; }`
    /// — BETWEEN low boundary literal is incomparable (mixed type).
    #[test]
    fn between_with_incomparable_low_boundary_no_warning() {
        // new low boundary is Bool — literal_compare(Int, Bool) → None.
        let baseline = baseline_with_check("t", "c", "x BETWEEN 0 AND 100");
        let p = plan(vec![add_check("t", "c", "x BETWEEN TRUE AND 90")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// L262: `let Some(hi_cmp) = literal_compare(...) else { return false; }`
    /// — BETWEEN high boundary literal is incomparable.
    #[test]
    fn between_with_incomparable_high_boundary_no_warning() {
        let baseline = baseline_with_check("t", "c", "x BETWEEN 0 AND 100");
        let p = plan(vec![add_check("t", "c", "x BETWEEN 10 AND TRUE")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert!(warnings.is_empty());
    }

    /// L289-291: literal_compare reachable arms — exercised via a
    /// string-literal boundary tightening (String/String, L291) and a
    /// cross-numeric range tightening (Int/Float L289, Float/Int L290).
    #[test]
    fn string_boundary_tightening_warns() {
        let baseline = baseline_with_check("t", "c", "s > 'aaa'");
        let p = plan(vec![add_check("t", "c", "s > 'bbb'")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    #[test]
    fn cross_numeric_int_to_float_boundary_tightening_warns() {
        // old: ratio > 0 (Integer), new: ratio > 0.5 (Float)
        // — literal_compare(Int 0, Float 0.5) at L289 → Less → tighter.
        let baseline = baseline_with_check("t", "c", "ratio > 0");
        let p = plan(vec![add_check("t", "c", "ratio > 0.5")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    #[test]
    fn cross_numeric_float_to_int_boundary_tightening_warns() {
        // old: ratio > 0.5 (Float), new: ratio > 1 (Integer)
        // — literal_compare(Float 0.5, Int 1) at L290 → Less → tighter.
        let baseline = baseline_with_check("t", "c", "ratio > 0.5");
        let p = plan(vec![add_check("t", "c", "ratio > 1")]);
        let warnings = find_check_strengthenings(&p, &baseline);
        assert_eq!(warnings[0].kind, CheckStrengtheningKind::BoundaryTightened);
    }

    /// `Literal::approx_eq` (the shared epsilon literal-equality SoT in
    /// `check_expr_parser`) — direct unit test pinning every reachable
    /// arm including Bool/Bool, Null/Null, and the Int/Float cross-arms.
    #[test]
    fn literal_approx_eq_covers_all_reachable_arms() {
        assert!(Literal::Integer(5).approx_eq(&Literal::Integer(5)));
        assert!(Literal::Float(1.5).approx_eq(&Literal::Float(1.5)));
        // Cross-numeric arms exercise the i64 → f64 epsilon comparison.
        assert!(Literal::Integer(5).approx_eq(&Literal::Float(5.0)));
        assert!(Literal::Float(5.0).approx_eq(&Literal::Integer(5)));
        assert!(Literal::String("a".into()).approx_eq(&Literal::String("a".into())));
        assert!(Literal::Bool(true).approx_eq(&Literal::Bool(true)));
        assert!(Literal::Null.approx_eq(&Literal::Null));
        // Mixed-type fallthrough returns false.
        assert!(!Literal::Integer(1).approx_eq(&Literal::String("1".into())));
        assert!(!Literal::Bool(true).approx_eq(&Literal::Null));
    }

    /// L249: `if subset.is_empty() { return false; }` is a defensive
    /// guard — the CHECK parser already rejects empty `IN ()` lists
    /// (see `check_expr_parser::parse_predicate`'s IN branch which
    /// returns `Unparseable` when `values.is_empty()`). Reaching L249
    /// in production is therefore impossible. Direct unit-test pins
    /// the guard's contract regardless.
    #[test]
    fn in_is_strict_subset_empty_subset_returns_false_defensively() {
        // empty subset vs non-empty superset: false (defensive guard).
        let empty: Vec<Literal> = vec![];
        let superset = vec![Literal::Integer(1), Literal::Integer(2)];
        assert!(!in_is_strict_subset(&empty, &superset));
        // Subset larger than superset: false (size check).
        assert!(!in_is_strict_subset(
            &vec![
                Literal::Integer(1),
                Literal::Integer(2),
                Literal::Integer(3)
            ],
            &vec![Literal::Integer(1)]
        ));
        // Proper strict subset: true.
        assert!(in_is_strict_subset(
            &vec![Literal::Integer(1)],
            &vec![Literal::Integer(1), Literal::Integer(2)]
        ));
    }

    #[test]
    fn classify_strengthening_skips_semantically_equal_parenthesized_expr() {
        assert!(classify_strengthening("age > 0", "(age > 0)").is_none());
    }

    #[test]
    fn between_narrower_rejects_incomparable_bounds() {
        assert!(!between_is_narrower(
            &Literal::String("a".into()),
            &Literal::Integer(10),
            &Literal::Integer(1),
            &Literal::Integer(9)
        ));
        assert!(!between_is_narrower(
            &Literal::Integer(0),
            &Literal::String("z".into()),
            &Literal::Integer(1),
            &Literal::Integer(9)
        ));
    }
}
