//! Detect column defaults that demonstrably violate a table-level CHECK
//! constraint.
//!
//! This is fault **F86** in the data-dependent migration fault taxonomy:
//! every `INSERT` that relies on the column's default value is rejected by
//! the database at runtime. The migration itself succeeds — only the first
//! application `INSERT` discovers the mismatch.
//!
//! Vespertide rejects this *statically* during `validate_schema`. CHECK
//! expressions are tokenised by the shared
//! [`super::check_expr_parser`] (also used by F29) which recognises the
//! dialect-neutral subset of SQL boolean expressions. F86 only triggers
//! on the narrow per-column predicate shapes:
//!
//! ```text
//!   <column> <op>  <literal>          // op ∈ { > >= < <= = <> != }
//!   <column> IN (<lit>, <lit>, ...)
//! ```
//!
//! Anything else (compound expressions via AND/OR, BETWEEN, IS NULL,
//! function calls, casts, references to other columns) is treated as
//! *unparseable* for F86 purposes and silently passes — by design,
//! since misjudging a complex expression as violated would block
//! legitimate schemas. F29 (CHECK strengthening) consumes the same
//! parser but uses the full AST including AND/OR composition.

use vespertide_core::{DefaultValue, TableConstraint, TableDef};

use super::check_expr_parser::{
    Literal, Op, SimpleColumnCheck, extract_simple_column_check, parse,
};
use crate::error::PlannerError;

/// Inspect every column in `table`: if it has a default value AND there is
/// a table-level CHECK constraint that this checker can parse as a simple
/// pattern over the column, evaluate the default against the constraint
/// and raise [`PlannerError::DefaultViolatesCheck`] on mismatch.
///
/// Static: no data access. Pure structural / textual analysis.
pub(super) fn validate_default_vs_check(table: &TableDef) -> Result<(), PlannerError> {
    for column in &table.columns {
        let Some(default) = column.default.as_ref() else {
            continue;
        };
        let column_name = column.name.as_str();

        for constraint in &table.constraints {
            let TableConstraint::Check { name, expr, .. } = constraint else {
                continue;
            };
            let parsed = parse(expr);
            let Some(simple) = extract_simple_column_check(&parsed, column_name) else {
                continue; // unparseable for F86 — silent pass by design
            };
            if !check_satisfied(&simple, default) {
                return Err(PlannerError::DefaultViolatesCheck {
                    table: table.name.to_string(),
                    column: column_name.to_string(),
                    default_value: default.to_sql(),
                    check_name: name.clone(),
                    check_expr: expr.clone(),
                });
            }
        }
    }
    Ok(())
}

fn check_satisfied(check: &SimpleColumnCheck, default: &DefaultValue) -> bool {
    match check {
        SimpleColumnCheck::Op { op, value } => evaluate_op(*op, default, value),
        SimpleColumnCheck::In(list) => list.iter().any(|v| literal_equals(default, v)),
    }
}

fn evaluate_op(op: Op, default: &DefaultValue, target: &Literal) -> bool {
    match (default, target) {
        (DefaultValue::Integer(a), Literal::Integer(b)) => apply_op_i64(op, *a, *b),
        (DefaultValue::Float(a), Literal::Float(b)) => apply_op_f64(op, *a, *b),
        (DefaultValue::Integer(a), Literal::Float(b)) => apply_op_f64(op, i64_to_f64(*a), *b),
        (DefaultValue::Float(a), Literal::Integer(b)) => apply_op_f64(op, *a, i64_to_f64(*b)),
        (DefaultValue::String(a), Literal::String(b)) => apply_op_str(op, a, b),
        (DefaultValue::Bool(a), Literal::Bool(b)) => apply_op_bool(op, *a, *b),
        // Type mismatch — can't evaluate confidently. Treat as satisfied
        // to avoid false positives on `default: "now()"` style expressions
        // we don't recognise as a literal.
        _ => true,
    }
}

/// Lossy widening cast confined to this module so the precision-loss
/// `#[expect]` lives in exactly one place. `f64` only has 52-bit mantissa;
/// `i64` defaults outside `±2^53` will round, but checks involving
/// `2^53+`-sized defaults are vanishingly rare and the F86 detector
/// intentionally errs on the side of "silent pass" when ambiguous.
#[expect(
    clippy::cast_precision_loss,
    reason = "CHECK evaluation: rounding integers beyond 2^53 is acceptable since F86 silent-passes on ambiguity anyway"
)]
fn i64_to_f64(v: i64) -> f64 {
    v as f64
}

fn apply_op_i64(op: Op, a: i64, b: i64) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
    }
}

fn apply_op_f64(op: Op, a: f64, b: f64) -> bool {
    match op {
        // NaN handling: any comparison with NaN is false except !=.
        Op::Eq => (a - b).abs() < f64::EPSILON,
        Op::Ne => (a - b).abs() >= f64::EPSILON,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
    }
}

fn apply_op_str(op: Op, a: &str, b: &str) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
    }
}

fn apply_op_bool(op: Op, a: bool, b: bool) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        // Ordering on booleans is not idiomatic; refuse to judge so the
        // user keeps full control.
        _ => true,
    }
}

fn literal_equals(default: &DefaultValue, lit: &Literal) -> bool {
    match (default, lit) {
        (DefaultValue::Integer(a), Literal::Integer(b)) => a == b,
        (DefaultValue::Float(a), Literal::Float(b)) => (a - b).abs() < f64::EPSILON,
        (DefaultValue::Integer(a), Literal::Float(b)) => (i64_to_f64(*a) - b).abs() < f64::EPSILON,
        (DefaultValue::Float(a), Literal::Integer(b)) => (a - i64_to_f64(*b)).abs() < f64::EPSILON,
        (DefaultValue::String(a), Literal::String(b)) => a == b,
        (DefaultValue::Bool(a), Literal::Bool(b)) => a == b,
        _ => false,
    }
}
