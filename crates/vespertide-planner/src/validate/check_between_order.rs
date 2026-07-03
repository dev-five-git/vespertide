//! Fault **F-novel-15** - CHECK BETWEEN boundary order detection.
//!
//! A CHECK constraint of the form `col BETWEEN low AND high` where
//! the *literal* `low` value is greater than the *literal* `high`
//! value defines an empty acceptance set: SQL standard defines
//! `BETWEEN` as `col >= low AND col <= high`, so when `low > high`
//! the conjunction is always false. Every `INSERT` against the
//! table fails. Almost always an authoring error - the user
//! transposed the boundaries.
//!
//! Detected by parsing the CHECK expression with the shared
//! [`super::check_expr_parser`] and inspecting every `Between` node
//! for boundary order. Backend-neutral (SQL standard semantics
//! identical on `PostgreSQL`, `MySQL`, `SQLite`). Walks `And` /
//! `Or` / `Not` composition so a nested `BETWEEN` inside a larger
//! expression is also caught.
//!
//! # Suppression rules
//!
//! - `NOT BETWEEN` is *not* flagged: a reversed `NOT BETWEEN low AND
//!   high` with `low > high` is *always true*, which is harmless
//!   (the constraint accepts every row). Not an error worth blocking.
//! - Mixed-type boundaries (e.g. `low = 5` int, `high = 'x'` string)
//!   are silently skipped - the comparator returns `None` and we
//!   can't decide which is larger. Conservative: never false-flag.
//! - `Unparseable` CHECK expressions silently pass - the parser
//!   already excludes them from analysis, same as F29 / F86.
//!
//! # Why hard error and not warning + prompt
//!
//! Mirrors the F86 (default-violates-check) pattern: this is a
//! *deterministic* failure - the constraint rejects every row by
//! construction, no data-dependent ambiguity. Surfacing as a hard
//! `PlannerError::BetweenBoundaryReversed` is more useful than a
//! prompt because the only correct fix is to edit the model. The
//! prompt would add friction without offering a meaningful choice.

use std::cmp::Ordering;

use vespertide_core::{TableConstraint, TableDef};

use super::check_expr_parser::{CheckExpr, Literal, parse};
use crate::error::PlannerError;

/// Inspect every table-level CHECK constraint on `table`: if the
/// expression contains a `BETWEEN low AND high` node with
/// `low > high` literal boundaries, raise
/// [`PlannerError::BetweenBoundaryReversed`] on the first such
/// violation.
///
/// Static: no data access. Pure structural / textual analysis.
pub(super) fn validate_between_boundary_order(table: &TableDef) -> Result<(), PlannerError> {
    find_between_boundary_reversals(table)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

/// Inspect every table-level CHECK constraint on `table` and collect every
/// `BETWEEN low AND high` node whose literal boundaries are reversed.
///
/// Unlike `validate_between_boundary_order`, this does not stop at the first
/// violation. It is used by editor diagnostics so independent CHECK mistakes in
/// one model all get their own squiggle.
pub fn find_between_boundary_reversals(table: &TableDef) -> Vec<PlannerError> {
    let mut errors = Vec::new();
    for constraint in &table.constraints {
        let TableConstraint::Check { name, expr, .. } = constraint else {
            continue;
        };
        let parsed = parse(expr);
        let mut reversed = Vec::new();
        collect_reversed_between(&parsed, &mut reversed);
        for (column, low, high) in reversed {
            errors.push(PlannerError::BetweenBoundaryReversed {
                table: table.name.to_string(),
                column,
                check_name: name.clone(),
                low: format_literal(&low),
                high: format_literal(&high),
            });
        }
    }
    errors
}

/// Walk the parsed CHECK expression and push every reversed `Between` node.
fn collect_reversed_between(expr: &CheckExpr, out: &mut Vec<(String, Literal, Literal)>) {
    match expr {
        CheckExpr::Between {
            column,
            low,
            high,
            negated: false,
        } if literal_compare(low, high) == Some(Ordering::Greater) => {
            out.push((column.clone(), low.clone(), high.clone()));
        }
        CheckExpr::And(parts) | CheckExpr::Or(parts) => {
            for part in parts {
                collect_reversed_between(part, out);
            }
        }
        CheckExpr::Not(inner) => collect_reversed_between(inner, out),
        _ => {}
    }
}

fn literal_compare(a: &Literal, b: &Literal) -> Option<Ordering> {
    match (a, b) {
        (Literal::Integer(x), Literal::Integer(y)) => Some(x.cmp(y)),
        (Literal::Float(x), Literal::Float(y)) => x.partial_cmp(y),
        (Literal::Integer(x), Literal::Float(y)) => i64_to_f64(*x).partial_cmp(y),
        (Literal::Float(x), Literal::Integer(y)) => x.partial_cmp(&i64_to_f64(*y)),
        (Literal::String(x), Literal::String(y)) => Some(x.cmp(y)),
        // Mixed / Bool / Null: cannot order without ambiguity.
        _ => None,
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "F-novel-15 BETWEEN boundary comparison: rounding integers beyond 2^53 acceptable; conservative comparator silently skips ambiguous cases anyway"
)]
fn i64_to_f64(v: i64) -> f64 {
    v as f64
}

fn format_literal(lit: &Literal) -> String {
    match lit {
        Literal::Integer(i) => i.to_string(),
        Literal::Float(f) => f.to_string(),
        Literal::String(s) => s.clone(),
        Literal::Bool(b) => b.to_string(),
        Literal::Null => "NULL".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::{
        CheckViolationStrategy, ColumnDef, ColumnType, SimpleColumnType, TableDef,
    };

    fn check_constraint(name: &str, expr: &str) -> TableConstraint {
        TableConstraint::Check {
            name: name.to_string(),
            expr: expr.to_string(),
            strategy: CheckViolationStrategy::default(),
        }
    }

    fn table_with_check(name: &str, check_expr: &str) -> TableDef {
        TableDef {
            name: name.into(),
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
            constraints: vec![check_constraint("chk_test", check_expr)],
        }
    }

    #[test]
    fn reversed_integer_between_is_error() {
        let table = table_with_check("t", "age BETWEEN 100 AND 0");
        let result = validate_between_boundary_order(&table);
        let err = result.expect_err("expected BetweenBoundaryReversed");
        let PlannerError::BetweenBoundaryReversed {
            table: tbl,
            column,
            check_name,
            low,
            high,
        } = err
        else {
            panic!("expected BetweenBoundaryReversed variant, got {err:?}");
        };
        assert_eq!(tbl, "t");
        assert_eq!(column, "age");
        assert_eq!(check_name, "chk_test");
        assert_eq!(low, "100");
        assert_eq!(high, "0");
    }

    #[test]
    fn correctly_ordered_between_passes() {
        let table = table_with_check("t", "age BETWEEN 0 AND 100");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn equal_boundaries_pass() {
        // BETWEEN 5 AND 5 = singleton {5}, valid (non-empty).
        let table = table_with_check("t", "age BETWEEN 5 AND 5");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn reversed_float_between_is_error() {
        let table = table_with_check("t", "ratio BETWEEN 1.5 AND 0.5");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn reversed_string_between_is_error() {
        // Lexicographic comparison: 'z' > 'a' so BETWEEN 'z' AND 'a' is reversed.
        let table = table_with_check("t", "code BETWEEN 'z' AND 'a'");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn integer_float_mixed_reversed_is_error() {
        // i64 vs f64 cross-comparison works.
        let table = table_with_check("t", "x BETWEEN 100 AND 0.5");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn not_between_reversed_is_silently_passed() {
        // `NOT BETWEEN 100 AND 0` with low > high is always TRUE
        // (no row falls inside an empty set, so NOT empty = all).
        // Harmless — don't block.
        let table = table_with_check("t", "age NOT BETWEEN 100 AND 0");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn between_in_and_composition_is_detected() {
        let table = table_with_check("t", "age > 0 AND age BETWEEN 100 AND 0");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn between_in_or_composition_is_detected() {
        let table = table_with_check("t", "age < 0 OR age BETWEEN 100 AND 0");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn between_under_not_is_detected() {
        // Note: this is NOT (col BETWEEN reversed) where the inner
        // Between has negated=false, so the violation still fires.
        // (A semantically careful user can wrap reversed BETWEEN in
        // NOT to mean "always true" but they should write
        // `NOT BETWEEN` instead — the more idiomatic form.)
        let table = table_with_check("t", "NOT (age BETWEEN 100 AND 0)");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn unparseable_check_silently_passes() {
        let table = table_with_check("t", "LENGTH(name) > 5");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn no_check_constraint_passes() {
        let table = TableDef {
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
            constraints: Vec::new(),
        };
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn boolean_boundaries_silently_pass() {
        // Bool BETWEEN doesn't have a natural ordering in our
        // comparator (Bool is not in literal_compare's Some-arms),
        // so the conservative comparator skips it.
        let table = table_with_check("t", "flag BETWEEN TRUE AND FALSE");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn first_violation_wins_when_multiple_present() {
        // Two reversed BETWEENs in separate checks — only the first
        // is reported (validate_default_vs_check pattern).
        let table = TableDef {
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
            constraints: vec![
                check_constraint("chk_first", "age BETWEEN 100 AND 0"),
                check_constraint("chk_second", "score BETWEEN 50 AND 10"),
            ],
        };
        let err = validate_between_boundary_order(&table).unwrap_err();
        let PlannerError::BetweenBoundaryReversed { check_name, .. } = err else {
            panic!("expected BetweenBoundaryReversed");
        };
        assert_eq!(check_name, "chk_first");
    }

    #[test]
    fn finder_collects_two_reversed_constraints() {
        let table = TableDef {
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
            constraints: vec![
                check_constraint("chk_first", "age BETWEEN 100 AND 0"),
                check_constraint("chk_second", "score BETWEEN 50 AND 10"),
            ],
        };

        let errors = find_between_boundary_reversals(&table);
        assert_eq!(errors.len(), 2);
        assert!(matches!(
            &errors[0],
            PlannerError::BetweenBoundaryReversed { check_name, .. } if check_name == "chk_first"
        ));
        assert!(matches!(
            &errors[1],
            PlannerError::BetweenBoundaryReversed { check_name, .. } if check_name == "chk_second"
        ));
    }

    // ── Coverage-closure ──────────────────────────────────────────────

    /// `collect_reversed_between` walks `CheckExpr::Or` by recursing into
    /// each disjunct (line 99 — `for part in parts`). Multiple BETWEENs
    /// inside an OR yield one error per reversed branch.
    #[test]
    fn or_with_two_reversed_betweens_collects_both() {
        let table = TableDef {
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
            constraints: vec![check_constraint(
                "chk_or_both",
                "age BETWEEN 100 AND 0 OR score BETWEEN 50 AND 10",
            )],
        };

        let errors = find_between_boundary_reversals(&table);
        assert_eq!(errors.len(), 2, "{errors:?}");
    }

    /// `literal_compare` returns `None` on Bool literals (line 117 default
    /// arm) — `BETWEEN TRUE AND FALSE` silently passes because the
    /// comparator cannot order booleans.
    #[test]
    fn bool_between_silently_passes_via_literal_compare_none() {
        let table = table_with_check("t", "flag BETWEEN TRUE AND FALSE");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    /// `literal_compare` returns `None` when one side is Null — silent pass.
    #[test]
    fn null_between_silently_passes() {
        let table = table_with_check("t", "x BETWEEN NULL AND 100");
        assert!(validate_between_boundary_order(&table).is_ok());
    }

    #[test]
    fn finder_collects_one_reversed_among_valid_constraints() {
        let table = TableDef {
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
            constraints: vec![
                check_constraint("chk_valid", "age BETWEEN 0 AND 100"),
                check_constraint("chk_reversed", "score BETWEEN 50 AND 10"),
            ],
        };

        let errors = find_between_boundary_reversals(&table);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            PlannerError::BetweenBoundaryReversed { check_name, .. } if check_name == "chk_reversed"
        ));
    }

    #[test]
    fn literal_formatting_covers_bool_and_null_labels() {
        assert_eq!(format_literal(&Literal::Bool(true)), "true");
        assert_eq!(format_literal(&Literal::Null), "NULL");
    }

    /// L99: `literal_compare(Float, Integer)` arm. Existing tests
    /// cover the `(Integer, Float)` arm via `BETWEEN 100 AND 0.5`;
    /// this case writes the boundaries in `Float AND Integer` order
    /// so the parser yields `Literal::Float` for `low` and
    /// `Literal::Integer` for `high`, hitting the L99 cross arm.
    #[test]
    fn reversed_float_then_integer_between_is_error() {
        // Float (100.5) > Integer (0) → reversed → error.
        let table = table_with_check("t", "x BETWEEN 100.5 AND 0");
        assert!(validate_between_boundary_order(&table).is_err());
    }

    #[test]
    fn correctly_ordered_float_then_integer_between_passes() {
        // Float (0.5) < Integer (100) → in order → ok.
        let table = table_with_check("t", "x BETWEEN 0.5 AND 100");
        assert!(validate_between_boundary_order(&table).is_ok());
    }
}
