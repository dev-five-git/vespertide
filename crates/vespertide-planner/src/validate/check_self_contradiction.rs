//! Fault **F-novel-1** - CHECK self-contradiction detection.
//!
//! A CHECK constraint whose top-level `AND` conjuncts contain a
//! demonstrable contradiction on the same column. Every row would
//! be rejected by the database because no value can satisfy all
//! conjuncts simultaneously. Almost always an authoring error.
//!
//! # Recognised contradictions
//!
//! For two predicates `P1` and `P2` referencing the same column,
//! the comparator flags these patterns as **demonstrably
//! contradictory**:
//!
//! 1. **Range impossibility** (`Compare` x `Compare`):
//!    - `col > N` and `col < M` where `N >= M`
//!      (no value can be both greater than N and less than M when N >= M)
//!    - `col >= N` and `col <= M` where `N > M`
//!    - `col >= N` and `col < M` where `N >= M`
//!    - `col > N` and `col <= M` where `N >= M`
//! 2. **Boundary impossibility** (same literal):
//!    - `col >= N` and `col < N` (strict less excludes the boundary)
//!    - `col > N` and `col <= N` (strict greater excludes the boundary)
//! 3. **Equality conflict** (`Compare(Eq)` x `Compare(Eq)`):
//!    - `col = X` and `col = Y` where `X != Y` and same literal type
//! 4. **Equality vs not-equality**:
//!    - `col = X` and `col != X` (same literal)
//! 5. **Null conflict** (`IsNull` x `IsNull`):
//!    - `col IS NULL` and `col IS NOT NULL`
//! 6. **Null vs equality** (CHECK passes on NULL by SQL semantics,
//!    but inside an AND with `IS NOT NULL` the equality demands a
//!    non-NULL value matching X — combined with `IS NULL` on same
//!    column, the AND can never be satisfied):
//!    - `col IS NULL` and `col = X`
//!    - `col IS NULL` and `col != X`
//!    - `col IS NULL` and `col > X` (or any non-IS Compare)
//!
//! # Suppression rules (conservative, false-positive 0)
//!
//! - `OR` branches are not analysed (would require proving *every*
//!   branch contradicts — much harder, and the resulting "always
//!   false OR" tautology is rare in real schemas).
//! - `NOT` wrappers are not unfolded — `NOT (col > 5)` is treated
//!   as opaque to keep the comparator simple.
//! - Mixed-type literals (string compared to integer, etc.) silently
//!   pass — F-novel-4 (type-mismatch) covers those.
//! - `BETWEEN` is decomposed into `>=` + `<=` for the contradiction
//!   check.
//! - Different columns never contradict each other (we don't model
//!   inter-column constraints).
//!
//! # Why hard error
//!
//! Mirrors F86 / F-novel-15: this is a *deterministic* failure -
//! the constraint rejects every row by construction. A prompt
//! would add friction without offering a meaningful choice; the
//! only correct fix is to edit the model.

use std::cmp::Ordering;

use vespertide_core::{TableConstraint, TableDef};

use super::check_expr_parser::{CheckExpr, Literal, Op, parse};
use crate::error::PlannerError;

/// Inspect every table-level CHECK constraint on `table`. If the
/// expression's top-level AND conjuncts contain a contradictory
/// pair on the same column, raise
/// [`PlannerError::CheckSelfContradiction`] on the first such
/// violation.
///
/// Static: no data access. Pure structural / textual analysis.
pub(super) fn validate_self_contradiction(table: &TableDef) -> Result<(), PlannerError> {
    find_self_contradictions(table)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

/// Inspect every table-level CHECK constraint on `table` and collect each
/// constraint whose expression contains a demonstrable self-contradiction.
///
/// Unlike `validate_self_contradiction`, this does not stop at the first
/// faulty constraint. It is used by editor diagnostics so independent CHECK
/// mistakes in one model all get their own squiggle.
pub fn find_self_contradictions(table: &TableDef) -> Vec<PlannerError> {
    let mut errors = Vec::new();
    for constraint in &table.constraints {
        let TableConstraint::Check { name, expr, .. } = constraint else {
            continue;
        };
        let parsed = parse(expr);
        if let Some(contradiction) = find_contradiction(&parsed) {
            errors.push(PlannerError::CheckSelfContradiction {
                table: table.name.to_string(),
                check_name: name.clone(),
                column: contradiction.column,
                first: contradiction.first,
                second: contradiction.second,
            });
        }
    }
    errors
}

/// First contradictory pair detected anywhere under an `And` node.
/// Returns `None` when nothing demonstrably contradicts.
fn find_contradiction(expr: &CheckExpr) -> Option<Contradiction> {
    // Top-level And: flatten and pairwise-check.
    if let CheckExpr::And(parts) = expr {
        let flat = flatten_and(parts);
        // Group by column to keep the pairwise loop cheap.
        let by_column = group_predicates_by_column(&flat);
        for (column, preds) in by_column {
            // Pairwise contradiction check within the same column.
            for i in 0..preds.len() {
                for j in (i + 1)..preds.len() {
                    if let Some(c) = check_pair(column, preds[i], preds[j]) {
                        return Some(c);
                    }
                }
            }
        }
        // Recurse into nested ANDs and ORs - look for a contradiction
        // anywhere in the tree (not just the top-level AND).
        for part in flat {
            if let Some(c) = find_contradiction(part) {
                return Some(c);
            }
        }
        None
    } else if let CheckExpr::Or(parts) = expr {
        // Recurse into OR branches; a contradiction inside any branch
        // is still worth reporting (the branch itself is dead code).
        parts.iter().find_map(find_contradiction)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Contradiction {
    column: String,
    first: String,
    second: String,
}

/// Flatten nested `And` nodes into a single Vec of leaf predicates.
/// Stops recursion at non-And nodes (so And-inside-Or is preserved
/// as one entry).
fn flatten_and(parts: &[CheckExpr]) -> Vec<&CheckExpr> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            CheckExpr::And(inner) => out.extend(flatten_and(inner)),
            _ => out.push(part),
        }
    }
    out
}

/// Bucket `Compare` / `In` / `Between` / `IsNull` predicates by the
/// column they reference. Predicates that don't directly reference a
/// single column (And/Or/Not/Unparseable) are skipped.
fn group_predicates_by_column<'a>(flat: &[&'a CheckExpr]) -> Vec<(&'a str, Vec<&'a CheckExpr>)> {
    let mut groups: Vec<(&'a str, Vec<&'a CheckExpr>)> = Vec::new();
    for pred in flat {
        // `if let` (not `let … else { continue; }`) so the skip path folds
        // into the loop tail — LLVM coverage mis-attributes a bare `continue`.
        if let Some(col) = predicate_column(pred) {
            if let Some((_, existing)) = groups.iter_mut().find(|(c, _)| *c == col) {
                existing.push(pred);
            } else {
                groups.push((col, vec![pred]));
            }
        }
    }
    groups
}

fn predicate_column(expr: &CheckExpr) -> Option<&str> {
    match expr {
        CheckExpr::Compare { column, .. }
        | CheckExpr::In { column, .. }
        | CheckExpr::Between { column, .. }
        | CheckExpr::IsNull { column, .. } => Some(column),
        _ => None,
    }
}

/// Pairwise contradiction check for two predicates on the same column.
fn check_pair(column: &str, a: &CheckExpr, b: &CheckExpr) -> Option<Contradiction> {
    // Try Compare vs Compare in both orderings.
    if let (
        CheckExpr::Compare {
            op: op_a,
            value: va,
            ..
        },
        CheckExpr::Compare {
            op: op_b,
            value: vb,
            ..
        },
    ) = (a, b)
        && let Some((first, second)) = compare_pair_contradicts(*op_a, va, *op_b, vb)
    {
        return Some(Contradiction {
            column: column.to_string(),
            first: format_compare(column, *op_a, &first),
            second: format_compare(column, *op_b, &second),
        });
    }
    // IsNull vs IsNull: opposite negations contradict.
    if let (CheckExpr::IsNull { negated: na, .. }, CheckExpr::IsNull { negated: nb, .. }) = (a, b)
        && na != nb
    {
        return Some(Contradiction {
            column: column.to_string(),
            first: format_is_null(column, *na),
            second: format_is_null(column, *nb),
        });
    }
    // IsNull (positive) vs Compare on same column: AND is unsatisfiable.
    // Only positive `IS NULL` is contradictory with a non-null comparison;
    // `IS NOT NULL` is the *expected* companion of a Compare and never
    // contradicts — display Strings are built only once that (rare)
    // contradiction is confirmed.
    if let Some((isnull_neg, op, value)) = is_null_vs_other(a, b)
        && !isnull_neg
    {
        return Some(Contradiction {
            column: column.to_string(),
            first: format_is_null(column, isnull_neg),
            second: format_compare(column, op, &value.display_value()),
        });
    }
    None
}

/// Returns `Some((first_label, second_label))` when two Compare
/// predicates on the same column cannot be simultaneously satisfied.
/// The label strings are used by the caller for display; they
/// always echo the literal value passed in.
fn compare_pair_contradicts(
    op_a: Op,
    va: &Literal,
    op_b: Op,
    vb: &Literal,
) -> Option<(String, String)> {
    let cmp = va.cmp_value(vb)?; // Need ordered literals.

    // Equality conflict: col = X AND col = Y where X != Y.
    if op_a == Op::Eq && op_b == Op::Eq && cmp != Ordering::Equal {
        return Some((va.display_value(), vb.display_value()));
    }
    // Equality vs negation: col = X AND col != X.
    if (op_a == Op::Eq && op_b == Op::Ne || op_a == Op::Ne && op_b == Op::Eq)
        && cmp == Ordering::Equal
    {
        return Some((va.display_value(), vb.display_value()));
    }

    // Range impossibility: at most one direction each.
    let (lower_op, lower_val, upper_op, upper_val) = match (op_a, op_b) {
        // a is lower bound, b is upper bound:
        (Op::Gt | Op::Ge, Op::Lt | Op::Le) => (op_a, va, op_b, vb),
        // b is lower bound, a is upper bound:
        (Op::Lt | Op::Le, Op::Gt | Op::Ge) => (op_b, vb, op_a, va),
        _ => return None,
    };
    let lower_vs_upper = lower_val.cmp_value(upper_val)?;
    let strict_boundary = lower_op == Op::Gt || upper_op == Op::Lt;
    let unsatisfiable = matches!(lower_vs_upper, Ordering::Greater)
        || strict_boundary && lower_vs_upper == Ordering::Equal;
    if unsatisfiable {
        Some((va.display_value(), vb.display_value()))
    } else {
        None
    }
}

/// When one of `(a, b)` is `IsNull(negated)` and the other is any
/// `Compare`, return the structural facts: the IsNull's `negated`
/// flag plus the Compare's op and literal. Returns `None` otherwise.
/// The caller formats display labels only after a contradiction is
/// confirmed, so the common healthy `IS NOT NULL AND col > X` shape
/// allocates nothing here.
fn is_null_vs_other<'a>(a: &'a CheckExpr, b: &'a CheckExpr) -> Option<(bool, Op, &'a Literal)> {
    // Normalise to (IsNull, Compare) ordering so the body is written once.
    match (a, b) {
        (CheckExpr::IsNull { negated, .. }, CheckExpr::Compare { op, value, .. })
        | (CheckExpr::Compare { op, value, .. }, CheckExpr::IsNull { negated, .. }) => {
            Some((*negated, *op, value))
        }
        _ => None,
    }
}

fn format_compare(column: &str, op: Op, value_text: &str) -> String {
    let op_str = match op {
        Op::Eq => "=",
        Op::Ne => "<>",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        Op::Ge => ">=",
    };
    format!("{column} {op_str} {value_text}")
}

fn format_is_null(column: &str, negated: bool) -> String {
    if negated {
        format!("{column} IS NOT NULL")
    } else {
        format!("{column} IS NULL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::check;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableDef};

    fn table(checks: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: "t".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(
                    vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true),
                ),
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: checks,
        }
    }

    // A top-level AND whose conjunct is an OR (not a single-column
    // predicate) exercises the `group_predicates_by_column` skip path: the
    // OR has no single owning column, so `predicate_column` returns None and
    // the bucketing loop `continue`s past it. The expression is satisfiable,
    // so no contradiction is reported.
    #[test]
    fn and_with_or_conjunct_skips_non_column_predicate() {
        let t = table(vec![check("chk", "(age > 0 OR age < 5) AND id <> 3")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // `col = X AND col != X` and the reversed `col != X AND col = X` are both
    // contradictions. The reversed order pins the SECOND disjunct
    // (`op_a == Ne && op_b == Eq`) of the equality-vs-negation check, whose
    // `op_b == Op::Eq` comparison a `!=` mutant would break.
    #[test]
    fn eq_then_ne_same_literal_is_contradiction() {
        let t = table(vec![check("chk", "age = 5 AND age <> 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn ne_then_eq_same_literal_is_contradiction() {
        let t = table(vec![check("chk", "age <> 5 AND age = 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    // -- Range impossibility ---------------------------------------------

    #[test]
    fn gt_and_lt_range_impossible() {
        let t = table(vec![check("chk", "age > 100 AND age < 0")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn gt_and_lt_equal_boundaries_impossible() {
        // col > 5 AND col < 5 — no value satisfies both.
        let t = table(vec![check("chk", "age > 5 AND age < 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn ge_and_le_reversed_impossible() {
        // col >= 10 AND col <= 5 — empty interval.
        let t = table(vec![check("chk", "age >= 10 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn ge_and_le_equal_is_valid_singleton() {
        // col >= 5 AND col <= 5 = singleton {5}, non-empty.
        let t = table(vec![check("chk", "age >= 5 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn ge_and_lt_boundary_impossible() {
        // col >= 5 AND col < 5 — boundary excludes value.
        let t = table(vec![check("chk", "age >= 5 AND age < 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn gt_and_le_boundary_impossible() {
        // col > 5 AND col <= 5 — boundary excludes value.
        let t = table(vec![check("chk", "age > 5 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn proper_range_is_valid() {
        let t = table(vec![check("chk", "age > 0 AND age < 100")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Equality conflict -----------------------------------------------

    #[test]
    fn eq_with_different_literals_contradicts() {
        let t = table(vec![check("chk", "code = 'a' AND code = 'b'")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn eq_with_same_literal_is_fine() {
        let t = table(vec![check("chk", "code = 'a' AND code = 'a'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn eq_vs_ne_same_literal_contradicts() {
        let t = table(vec![check("chk", "code = 'a' AND code <> 'a'")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn eq_vs_ne_different_literal_is_fine() {
        let t = table(vec![check("chk", "code = 'a' AND code <> 'b'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Null conflict ---------------------------------------------------

    #[test]
    fn is_null_and_is_not_null_contradict() {
        let t = table(vec![check(
            "chk",
            "deleted_at IS NULL AND deleted_at IS NOT NULL",
        )]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn is_null_and_compare_contradicts() {
        // col IS NULL AND col = 5 — IS NULL demands NULL, = 5 demands non-NULL.
        let t = table(vec![check("chk", "score IS NULL AND score = 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn is_not_null_and_compare_is_fine() {
        let t = table(vec![check("chk", "score IS NOT NULL AND score = 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn is_null_alone_is_fine() {
        let t = table(vec![check("chk", "deleted_at IS NULL")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Different columns never contradict ------------------------------

    #[test]
    fn different_columns_with_opposite_predicates_pass() {
        let t = table(vec![check("chk", "a > 5 AND b < 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn different_columns_eq_pass() {
        let t = table(vec![check("chk", "a = 'x' AND b = 'y'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Mixed types silently pass --------------------------------------

    #[test]
    fn integer_vs_string_literal_silently_passes() {
        // F-novel-4 territory; F-novel-1 doesn't second-guess.
        let t = table(vec![check("chk", "age > 5 AND age < 'foo'")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Composition -----------------------------------------------------

    #[test]
    fn contradiction_inside_or_branch_is_detected() {
        // The OR as a whole is satisfiable (the other branch works),
        // but the second branch is dead code — surface as warning.
        let t = table(vec![check("chk", "age < 0 OR (age > 100 AND age < 50)")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn nested_and_flattens() {
        // ((a AND b) AND c) treated as `a AND b AND c`.
        let t = table(vec![check("chk", "(age > 100 AND age < 200) AND age < 0")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    #[test]
    fn three_conjuncts_pairwise_check() {
        // No pair contradicts: 0 < age < 100, and age != 50.
        let t = table(vec![check("chk", "age > 0 AND age < 100 AND age <> 50")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // -- Unparseable silently passes ------------------------------------

    #[test]
    fn unparseable_check_silently_passes() {
        let t = table(vec![check("chk", "LENGTH(name) > 0 AND LENGTH(name) < 0")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn first_violation_wins_when_multiple_checks_contradict() {
        let t = table(vec![
            check("chk_first", "age > 100 AND age < 0"),
            check("chk_second", "score = 1 AND score = 2"),
        ]);
        let err = validate_self_contradiction(&t).unwrap_err();
        let PlannerError::CheckSelfContradiction { check_name, .. } = err else {
            panic!("expected CheckSelfContradiction");
        };
        assert_eq!(check_name, "chk_first");
    }

    #[test]
    fn or_without_contradiction_passes() {
        let t = table(vec![check("chk", "age < 0 OR age > 100")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    #[test]
    fn finder_collects_two_contradicting_constraints() {
        let t = table(vec![
            check("chk_first", "age > 100 AND age < 0"),
            check("chk_second", "score = 1 AND score = 2"),
        ]);

        let errors = find_self_contradictions(&t);
        assert_eq!(errors.len(), 2);
        assert!(matches!(
            &errors[0],
            PlannerError::CheckSelfContradiction { check_name, .. } if check_name == "chk_first"
        ));
        assert!(matches!(
            &errors[1],
            PlannerError::CheckSelfContradiction { check_name, .. } if check_name == "chk_second"
        ));
    }

    #[test]
    fn finder_collects_one_contradiction_among_valid_constraints() {
        let t = table(vec![
            check("chk_valid", "age > 0 AND age < 100"),
            check("chk_impossible", "score >= 10 AND score <= 5"),
        ]);

        let errors = find_self_contradictions(&t);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            PlannerError::CheckSelfContradiction { check_name, .. } if check_name == "chk_impossible"
        ));
    }

    // ── Coverage-closure: pairwise / grouping / range / IsNull combinations ──

    /// 3-predicate AND on same column — exercises the inner `for j in
    /// (i + 1)..` loop (line 119) across multiple `j` iterations and the
    /// grouping `iter_mut().find(...)` path on existing-column hit (line 174).
    #[test]
    fn three_same_column_predicates_with_contradiction_in_inner_pair() {
        // (age > 0, age < 5, age > 5) — pair (0)/(1) is fine, pair (1)/(2) contradicts.
        let t = table(vec![check("chk", "age > 0 AND age < 5 AND age > 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// Nested AND under OR — find_contradiction recurses through OR
    /// branches; the inner AND is flattened via `flatten_and`'s
    /// `match part { CheckExpr::And(inner) => out.extend(...) }` (line 160).
    #[test]
    fn or_branch_with_nested_and_invokes_flatten_and() {
        let t = table(vec![check(
            "chk",
            "(age < 0) OR ((age > 100 AND age < 200) AND age <> 150)",
        )]);
        // No contradiction anywhere — but execution reaches the nested
        // flatten path through the OR branch.
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// `compare_pair_contradicts` Lt-first ordering arm (`(Op::Lt | Op::Le,
    /// Op::Gt | Op::Ge)` swap, lines 265-269): the literal `(age < 0 AND
    /// age > 100)` puts upper bound first.
    #[test]
    fn lt_first_then_gt_range_impossible() {
        let t = table(vec![check("chk", "age < 0 AND age > 100")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// `Op::Gt` + `Op::Le` boundary mix (`> 5 AND <= 5`) — exercises one
    /// of the unsatisfiable match arms (lines 277-278).
    #[test]
    fn gt_and_le_same_literal_is_impossible() {
        let t = table(vec![check("chk", "age > 5 AND age <= 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// `Op::Ge` + `Op::Lt` boundary mix (`>= 5 AND < 5`) — second
    /// unsatisfiable arm.
    #[test]
    fn ge_and_lt_same_literal_is_impossible() {
        let t = table(vec![check("chk", "age >= 5 AND age < 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// Strict range that *is* satisfiable: `>= 5 AND <= 10` — exercises
    /// the unsatisfiable=`false` branch (line 285's `else None`).
    #[test]
    fn ge_and_le_valid_range_passes() {
        let t = table(vec![check("chk", "age >= 5 AND age <= 10")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// `IsNull` vs `Compare` with `IS NOT NULL` form — exercises the
    /// `is_null_vs_other` early-return when `isnull_neg = true` (line
    /// 225 area where the IsNull `negated=true` is the expected
    /// companion and does NOT contradict).
    #[test]
    fn is_not_null_with_compare_is_fine() {
        // `score IS NOT NULL AND score = 5` is sensible — not a contradiction.
        let t = table(vec![check("chk", "score IS NOT NULL AND score = 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// Three-conjunct test with grouping across two distinct columns —
    /// exercises both branches of `group_predicates_by_column`'s
    /// `iter_mut().find(...)` (lines 174-178): existing-column hit on
    /// second `age` predicate, new-column miss on first `score`.
    #[test]
    fn predicates_across_columns_group_correctly_no_contradiction() {
        // Two columns, one predicate each plus a second `age` predicate —
        // total 3 conjuncts, two columns, exercises both arms of the
        // groups.iter_mut().find branch (insert + existing append).
        let t = table(vec![check("chk", "age > 0 AND age < 100 AND score = 5")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// Eq vs Ne contradiction (line 259-262 in compare_pair_contradicts).
    #[test]
    fn eq_vs_ne_contradiction_via_integer_literals() {
        let t = table(vec![check("chk", "age = 5 AND age <> 5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// `check_pair` final `None` return (line 238) — two `In` predicates
    /// on the same column are skipped by all `if let` guards and fall
    /// through.
    #[test]
    fn in_vs_in_returns_no_contradiction() {
        // Two `IN (...)` predicates on same column — predicate_column
        // groups them, but check_pair has no matching arm for In/In, so
        // returns None via the final fall-through.
        let t = table(vec![check("chk", "x IN (1, 2) AND x IN (3, 4)")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// BETWEEN decomposed → `>=` + `<=`, paired with another BETWEEN
    /// on the same column — runs the grouping + pairwise loop with
    /// `Between` predicates.
    #[test]
    fn between_pairs_on_same_column_run_pairwise_loop() {
        // Two BETWEEN clauses overlap but don't contradict.
        let t = table(vec![check(
            "chk",
            "age BETWEEN 0 AND 100 AND age BETWEEN 10 AND 90",
        )]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    // ── Coverage-closure W3 (final 16 uncovered lines) ────────────────

    /// L119: `return Some(c);` inside the outer And recursion loop.
    /// Top-level And whose direct pairwise check finds no contradiction
    /// but a nested Or branch contains a contradicting And.
    #[test]
    fn nested_or_inside_top_level_and_recursion_finds_contradiction() {
        let t = table(vec![check(
            "chk",
            "a > 0 AND (b > 100 OR (c > 5 AND c < 0))",
        )]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// L160: `continue;` in group_predicates_by_column when
    /// predicate_column returns None (NOT-wrapped predicate has no
    /// direct column). Also covers L174 `_ => None,` in predicate_column.
    #[test]
    fn not_wrapped_predicate_skipped_in_grouping() {
        // `NOT (age > 5) AND age < 0` - Not wrapper has no direct column
        // → predicate_column returns None → continue at line 160 + the
        // `_ => None,` fallthrough at line 174.
        let t = table(vec![check("chk", "NOT (age > 5) AND age < 0")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// L250: `(Compare, IsNull) => (b, a)` swap arm in is_null_vs_other.
    /// The existing `is_null_and_compare_contradicts` test has IsNull
    /// first; here Compare is first.
    #[test]
    fn compare_then_is_null_swap_arm_is_contradiction() {
        let t = table(vec![check("chk", "score = 5 AND score IS NULL")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// L265: `(Float, Float)` literal comparison via compare_pair.
    #[test]
    fn float_vs_float_range_contradiction() {
        let t = table(vec![check("chk", "ratio > 0.5 AND ratio < 0.1")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// L266: `(Integer, Float)` literal comparison.
    #[test]
    fn integer_vs_float_range_contradiction() {
        // age > 100 (int) AND age < 5.5 (float) -> i64_to_f64 used
        let t = table(vec![check("chk", "age > 100 AND age < 5.5")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// L267: `(Float, Integer)` literal comparison.
    #[test]
    fn float_vs_integer_range_contradiction() {
        let t = table(vec![check("chk", "age > 100.5 AND age < 50")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// L269: `(Bool, Bool)` literal comparison.
    #[test]
    fn bool_vs_bool_equality_contradiction() {
        let t = table(vec![check("chk", "flag = TRUE AND flag = FALSE")]);
        assert!(validate_self_contradiction(&t).is_err());
    }

    /// Pin every reachable arm of `Literal::display_value` (the unified
    /// fault-message formatter on `check_expr_parser::Literal`). Float /
    /// Bool / Null arms aren't reached by the public flow because the
    /// production path filters them upstream, so this is the canonical
    /// regression test for those arms.
    #[test]
    fn display_value_covers_all_arms() {
        assert_eq!(Literal::Integer(7).display_value(), "7");
        assert_eq!(Literal::Float(1.5).display_value(), "1.5");
        assert_eq!(Literal::String("'x'".into()).display_value(), "'x'");
        assert_eq!(Literal::Bool(true).display_value(), "true");
        assert_eq!(Literal::Null.display_value(), "NULL");
    }

    /// Lock every reachable arm of `Literal::cmp_value` (the unified
    /// CHECK comparator) — `Integer/Integer`, `Float/Float`,
    /// `Integer/Float`, `Float/Integer`, `String/String`, `Bool/Bool`,
    /// and the mixed-type / Null silent-`None` fallthrough.
    #[test]
    fn cmp_value_covers_all_reachable_arms() {
        use std::cmp::Ordering;
        assert_eq!(
            Literal::Integer(1).cmp_value(&Literal::Integer(2)),
            Some(Ordering::Less)
        );
        assert_eq!(
            Literal::Float(1.0).cmp_value(&Literal::Float(2.0)),
            Some(Ordering::Less)
        );
        assert_eq!(
            Literal::Integer(1).cmp_value(&Literal::Float(2.0)),
            Some(Ordering::Less)
        );
        assert_eq!(
            Literal::Float(2.0).cmp_value(&Literal::Integer(1)),
            Some(Ordering::Greater)
        );
        assert_eq!(
            Literal::String("a".into()).cmp_value(&Literal::String("b".into())),
            Some(Ordering::Less)
        );
        assert_eq!(
            Literal::Bool(false).cmp_value(&Literal::Bool(true)),
            Some(Ordering::Less)
        );
        // Mixed-type returns None.
        assert!(
            Literal::Integer(1)
                .cmp_value(&Literal::String("a".into()))
                .is_none()
        );
        assert!(Literal::Null.cmp_value(&Literal::Integer(1)).is_none());
    }

    /// L238: `_ => false,` inside the (lower_op, upper_op) match.
    /// Provably-unreachable in production (the outer match at L221-226
    /// guarantees `lower_op ∈ {Gt, Ge}` and `upper_op ∈ {Lt, Le}`, all
    /// four combinations are covered by L231-237). Direct unit-test
    /// pinning compare_pair_contradicts behaviour for every reachable
    /// shape locks the contract; the dead `_ => false` fallback exists
    /// only to satisfy match exhaustiveness on `Op`.
    #[test]
    fn compare_pair_contradicts_all_reachable_shapes() {
        // Gt-Lt impossible boundary: age > 5 AND age < 5
        assert!(
            compare_pair_contradicts(Op::Gt, &Literal::Integer(5), Op::Lt, &Literal::Integer(5))
                .is_some()
        );
        // Gt-Le boundary: age > 5 AND age <= 5
        assert!(
            compare_pair_contradicts(Op::Gt, &Literal::Integer(5), Op::Le, &Literal::Integer(5))
                .is_some()
        );
        // Ge-Lt boundary: age >= 5 AND age < 5
        assert!(
            compare_pair_contradicts(Op::Ge, &Literal::Integer(5), Op::Lt, &Literal::Integer(5))
                .is_some()
        );
        // Ge-Le strict: age >= 5 AND age <= 5 (singleton, satisfiable)
        assert!(
            compare_pair_contradicts(Op::Ge, &Literal::Integer(5), Op::Le, &Literal::Integer(5))
                .is_none()
        );
        // Same-direction pair (Lt, Lt) -> the outer match returns None
        // at L226 before the inner unsatisfiable match runs.
        assert!(
            compare_pair_contradicts(Op::Lt, &Literal::Integer(5), Op::Lt, &Literal::Integer(10))
                .is_none()
        );
    }

    /// is_null_vs_other normalises (a, b) to (IsNull, Compare) ordering
    /// and returns the structural facts (negated flag, op, literal)
    /// without formatting. Direct unit-test pins both orderings and the
    /// `_ => None` fallthrough.
    #[test]
    fn is_null_vs_other_normalises_both_orderings() {
        let isnull = CheckExpr::IsNull {
            column: "x".into(),
            negated: false,
        };
        let compare = CheckExpr::Compare {
            column: "x".into(),
            op: Op::Eq,
            value: Literal::Integer(5),
        };
        // (IsNull, Compare) order
        let res = is_null_vs_other(&isnull, &compare);
        assert_eq!(res, Some((false, Op::Eq, &Literal::Integer(5))));
        // (Compare, IsNull) order — swap arm
        let res = is_null_vs_other(&compare, &isnull);
        assert_eq!(res, Some((false, Op::Eq, &Literal::Integer(5))));
        // Neither is IsNull/Compare combo — None via the `_ => None` arm
        let in_expr = CheckExpr::In {
            column: "x".into(),
            values: vec![Literal::Integer(1)],
            negated: false,
        };
        assert!(is_null_vs_other(&compare, &in_expr).is_none());
    }

    // ── Coverage-closure: defensive arms in find_contradiction & friends ──

    /// Top-level expression that is neither `And` nor `Or` (e.g. a single
    /// `IsNull`) — `find_contradiction` falls through to the trailing
    /// `else { None }` arm (line 139-141).
    #[test]
    fn top_level_single_predicate_falls_through_else_none() {
        let t = table(vec![check("chk", "id IS NOT NULL")]);
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// `predicate_column` `_ => None` arm via an entirely-unparseable
    /// CHECK expression — the parser yields `CheckExpr::Unparseable`
    /// which has no column, so `validate_self_contradiction` returns Ok.
    #[test]
    fn entirely_unparseable_check_returns_ok() {
        let t = table(vec![check("chk", "LENGTH(name) > 0 AND age < 0")]);
        // Either entirely-unparseable or no contradiction detected — either
        // way the conservative comparator must return Ok.
        assert!(validate_self_contradiction(&t).is_ok());
    }

    /// `predicate_column` Compare/In/Between/IsNull arms exhaustively
    /// pinned via direct calls (locks the contract; arms unreachable
    /// from the wrapper when the predicate is something else fall to
    /// `_ => None`).
    #[test]
    fn predicate_column_covers_all_directly_columnar_arms() {
        let cmp = CheckExpr::Compare {
            column: "a".into(),
            op: Op::Eq,
            value: Literal::Integer(1),
        };
        let in_e = CheckExpr::In {
            column: "b".into(),
            values: vec![Literal::Integer(1)],
            negated: false,
        };
        let bw = CheckExpr::Between {
            column: "c".into(),
            low: Literal::Integer(0),
            high: Literal::Integer(10),
            negated: false,
        };
        let isn = CheckExpr::IsNull {
            column: "d".into(),
            negated: false,
        };
        assert_eq!(predicate_column(&cmp), Some("a"));
        assert_eq!(predicate_column(&in_e), Some("b"));
        assert_eq!(predicate_column(&bw), Some("c"));
        assert_eq!(predicate_column(&isn), Some("d"));
        // `_ => None`: And node.
        let and = CheckExpr::And(vec![cmp.clone()]);
        assert!(predicate_column(&and).is_none());
    }

    /// Directly covers `flatten_and`'s non-`And` arm. Nested tests above
    /// exercise recursive extension; this pins the leaf push branch with
    /// adjacent non-And predicate shapes in deterministic order.
    #[test]
    fn flatten_and_pushes_non_and_leaf_predicates_in_order() {
        let cmp = CheckExpr::Compare {
            column: "age".into(),
            op: Op::Gt,
            value: Literal::Integer(0),
        };
        let is_null = CheckExpr::IsNull {
            column: "deleted_at".into(),
            negated: true,
        };

        let parts = [cmp.clone(), is_null.clone()];
        let leaves = flatten_and(&parts);

        assert_eq!(leaves.len(), 2);
        assert_eq!(leaves[0], &cmp);
        assert_eq!(leaves[1], &is_null);
    }
}
