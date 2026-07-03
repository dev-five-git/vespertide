use super::*;
use crate::validate::validate_schema;
use vespertide_core::DefaultValue;

fn validate_one(table: TableDef) -> Result<(), PlannerError> {
    // F86 sits behind `validate_table_entry`, which is private; route every
    // assertion through the public `validate_schema` entry so the
    // check_default hook actually fires.
    validate_schema(&[table])
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn col_with_default(name: &str, ty: ColumnType, default: DefaultValue) -> ColumnDef {
    let mut c = col(name, ty);
    c.nullable = false;
    c.default = Some(default);
    c
}

fn check_constraint(name: &str, expr: &str) -> TableConstraint {
    TableConstraint::Check {
        name: name.to_string(),
        expr: expr.to_string(),
        strategy: vespertide_core::CheckViolationStrategy::default(),
    }
}

fn pk_col(name: &str) -> ColumnDef {
    let mut c = col(name, ColumnType::Simple(SimpleColumnType::Integer));
    c.nullable = false;
    c.primary_key = Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));
    c
}

fn table_with(name: &str, payload_col: ColumnDef, checks: Vec<TableConstraint>) -> TableDef {
    let mut constraints = checks;
    constraints.push(TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: vec!["id".into()],
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    });
    table("the_table", vec![pk_col("id"), payload_col], constraints).with_name_for_test(name)
}

trait WithNameForTest {
    fn with_name_for_test(self, _name: &str) -> Self;
}
impl WithNameForTest for TableDef {
    fn with_name_for_test(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }
}

fn is_default_violates_check(err: &PlannerError) -> bool {
    matches!(err, PlannerError::DefaultViolatesCheck { .. })
}

// ---------------------------------------------------------------------------
// Violations: each comparison op + IN list rejection
// ---------------------------------------------------------------------------

#[test]
fn integer_default_zero_violates_check_amount_gt_zero() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    let err = validate_one(table).expect_err("default 0 should violate amount > 0");
    assert!(is_default_violates_check(&err), "got: {err:?}");
}

#[test]
fn integer_default_zero_violates_check_amount_ge_one() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_one_plus", "amount >= 1")],
    );
    let err = validate_one(table).expect_err("default 0 should violate amount >= 1");
    assert!(is_default_violates_check(&err));
}

#[test]
fn integer_default_100_violates_check_amount_lt_50() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(100),
        ),
        vec![check_constraint("chk_max", "amount < 50")],
    );
    let err = validate_one(table).unwrap_err();
    assert!(is_default_violates_check(&err));
}

#[test]
fn string_default_violates_in_list() {
    let table = table_with(
        "users",
        col_with_default(
            "status",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'banned'".into()),
        ),
        vec![check_constraint(
            "chk_status",
            "status IN ('active', 'inactive', 'pending')",
        )],
    );
    let err = validate_one(table).unwrap_err();
    assert!(is_default_violates_check(&err));
    if let PlannerError::DefaultViolatesCheck { default_value, .. } = err {
        assert_eq!(default_value, "'banned'");
    }
}

#[test]
fn string_default_violates_equality() {
    let table = table_with(
        "users",
        col_with_default(
            "role",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'admin'".into()),
        ),
        vec![check_constraint("chk_role", "role = 'user'")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn integer_default_violates_ne() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_not_zero", "amount <> 0")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

// ---------------------------------------------------------------------------
// Boundary kills: each comparison op exactly at its threshold so the
// `<`/`>`/`==` and EPSILON-arithmetic mutations are distinguished.
// ---------------------------------------------------------------------------

fn boundary_table(ty: SimpleColumnType, default: DefaultValue, expr: &str) -> TableDef {
    table_with(
        "t",
        col_with_default("v", ColumnType::Simple(ty), default),
        vec![check_constraint("c", expr)],
    )
}

#[test]
fn integer_default_equal_violates_lt_boundary() {
    // 5 < 5 is false (violates). Kills apply_op_i64 `< -> <=` and `< -> ==`.
    let t = boundary_table(SimpleColumnType::Integer, DefaultValue::Integer(5), "v < 5");
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn float_default_equal_violates_lt_boundary() {
    // 5.0 < 5.0 is false. Kills apply_op_f64 Lt `< -> <=`.
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(5.0), "v < 5.0");
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn float_default_equal_violates_gt_boundary() {
    // 5.0 > 5.0 is false. Kills apply_op_f64 Gt `> -> >=`.
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(5.0), "v > 5.0");
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn float_default_equal_satisfies_eq() {
    // (2-2).abs() < EPS is true. Kills apply_op_f64 Eq `- -> +`, `- -> /`,
    // and `< -> ==`.
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(2.0), "v = 2.0");
    assert!(validate_one(t).is_ok());
}

#[test]
fn float_opposite_sign_satisfies_ne() {
    // 2 <> -2: (2-(-2)).abs()=4 >= EPS true. Kills apply_op_f64 Ne `- -> +`
    // (where (2+(-2)).abs()=0 would wrongly report "equal").
    let t = boundary_table(
        SimpleColumnType::Real,
        DefaultValue::Float(2.0),
        "v <> -2.0",
    );
    assert!(validate_one(t).is_ok());
}

#[test]
fn float_zero_satisfies_ne_nonzero() {
    // 0 <> 5: (0-5).abs()=5 >= EPS true. Kills apply_op_f64 Ne `- -> /`
    // (where (0/5).abs()=0 would wrongly report "equal").
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(0.0), "v <> 5.0");
    assert!(validate_one(t).is_ok());
}

#[test]
fn string_default_equal_violates_lt_boundary() {
    // "'m'" < "'m'" is false. Kills apply_op_str `< -> <=` and `< -> ==`.
    let t = boundary_table(
        SimpleColumnType::Text,
        DefaultValue::String("'m'".into()),
        "v < 'm'",
    );
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn string_default_equal_violates_gt_boundary() {
    // "'m'" > "'m'" is false. Kills apply_op_str `> -> >=`.
    let t = boundary_table(
        SimpleColumnType::Text,
        DefaultValue::String("'m'".into()),
        "v > 'm'",
    );
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn bool_default_equal_violates_ne() {
    // true <> true is false (violates). Kills apply_op_bool delete of the
    // `Op::Ne` arm (which would fall through to `_ => true`).
    let t = boundary_table(
        SimpleColumnType::Boolean,
        DefaultValue::Bool(true),
        "v <> true",
    );
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn float_default_int_literal_violates_evaluate_op_arm() {
    // (Float, Integer) arm: 5.0 < 3 is false. Kills evaluate_op delete of the
    // `(Float, Integer)` arm (which would fall through to `_ => true`).
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(5.0), "v < 3");
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn integer_in_list_match_exercises_literal_equals_int_arm() {
    // 5 matches the IN list -> literal_equals (Int,Int) arm. Kills the
    // delete of that arm (-> `_ => false` -> not in list -> would violate).
    let t = boundary_table(
        SimpleColumnType::Integer,
        DefaultValue::Integer(5),
        "v IN (1, 5, 9)",
    );
    assert!(validate_one(t).is_ok());
}

#[test]
fn float_in_list_matches_exercise_literal_equals_float_arms() {
    // Float==Float, Int==Float, Float==Int IN-list matches. Each
    // `(a-b).abs() < EPS` distinguishes `< -> >` (0 > EPS would miss).
    assert!(
        validate_one(boundary_table(
            SimpleColumnType::Real,
            DefaultValue::Float(2.0),
            "v IN (2.0)"
        ))
        .is_ok()
    );
    assert!(
        validate_one(boundary_table(
            SimpleColumnType::Integer,
            DefaultValue::Integer(2),
            "v IN (2.0)"
        ))
        .is_ok()
    );
    assert!(
        validate_one(boundary_table(
            SimpleColumnType::Real,
            DefaultValue::Float(2.0),
            "v IN (2)"
        ))
        .is_ok()
    );
}

#[test]
fn bool_in_list_match_exercises_literal_equals_bool_arm() {
    // true IN (true) -> literal_equals (Bool,Bool) `==`. Kills `== -> !=`.
    let t = boundary_table(
        SimpleColumnType::Boolean,
        DefaultValue::Bool(true),
        "v IN (true)",
    );
    assert!(validate_one(t).is_ok());
}

// `1.0000000000000002` is exactly `1.0 + f64::EPSILON` (the next representable
// double). A value exactly EPSILON away is treated as DISTINCT by the strict
// `(a-b).abs() < EPSILON` tolerance, so the default violates an `=`/`IN` check.
// These pin `<` against `<=` at the tolerance boundary (the `<=` mutant would
// treat the pair as equal and wrongly pass).
const ONE_PLUS_EPS: &str = "1.0000000000000002";

#[test]
fn float_eq_at_exact_epsilon_distance_violates() {
    // apply_op_f64 Eq line: `(a-b).abs() < EPSILON`. Kills `< -> <=`.
    let expr = format!("v = {ONE_PLUS_EPS}");
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(1.0), &expr);
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn float_float_in_list_at_exact_epsilon_distance_misses() {
    // literal_equals (Float,Float) EPSILON line. Kills `< -> <=`.
    let expr = format!("v IN ({ONE_PLUS_EPS})");
    let t = boundary_table(SimpleColumnType::Real, DefaultValue::Float(1.0), &expr);
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn int_float_in_list_at_exact_epsilon_distance_misses() {
    // literal_equals (Integer,Float) EPSILON line. Kills `< -> <=`.
    let expr = format!("v IN ({ONE_PLUS_EPS})");
    let t = boundary_table(SimpleColumnType::Integer, DefaultValue::Integer(1), &expr);
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

#[test]
fn float_int_in_list_at_exact_epsilon_distance_misses() {
    // literal_equals (Float,Integer) EPSILON line. Kills `< -> <=`.
    let t = boundary_table(
        SimpleColumnType::Real,
        DefaultValue::Float(1.000_000_000_000_000_2),
        "v IN (1)",
    );
    assert!(is_default_violates_check(&validate_one(t).unwrap_err()));
}

// ---------------------------------------------------------------------------
// Satisfied: every op passes when the default fits
// ---------------------------------------------------------------------------

#[test]
fn integer_default_one_satisfies_amount_gt_zero() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(1),
        ),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_satisfies_in_list() {
    let table = table_with(
        "users",
        col_with_default(
            "status",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'active'".into()),
        ),
        vec![check_constraint(
            "chk_status",
            "status IN ('active', 'inactive')",
        )],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn boolean_default_satisfies_equality() {
    let table = table_with(
        "flags",
        col_with_default(
            "enabled",
            ColumnType::Simple(SimpleColumnType::Boolean),
            DefaultValue::Bool(true),
        ),
        vec![check_constraint("chk_enabled", "enabled = true")],
    );
    assert!(validate_one(table).is_ok());
}

// ---------------------------------------------------------------------------
// Silent pass: complex expressions intentionally not evaluated
// ---------------------------------------------------------------------------

#[test]
fn function_call_check_is_silent_pass() {
    let table = table_with(
        "users",
        col_with_default(
            "email",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("''".into()),
        ),
        vec![check_constraint("chk_email_shape", "length(email) > 0")],
    );
    // The default '' has length 0 which *would* violate, but the checker
    // does not parse function calls. Silent pass is the design choice.
    assert!(validate_one(table).is_ok());
}

#[test]
fn and_composed_check_is_silent_pass() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(50),
        ),
        vec![check_constraint("chk_range", "amount > 0 AND amount < 100")],
    );
    // Default 50 *would* satisfy, but AND-composition isn't evaluated
    // either way — silent pass holds.
    assert!(validate_one(table).is_ok());
}

#[test]
fn check_referring_to_a_different_column_is_silent_pass() {
    // CHECK on `total` while `amount` has the default — the checker only
    // evaluates checks whose LHS matches the column being defaulted.
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_total", "total > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn check_with_function_call_default_is_silent_pass_on_default_side() {
    // `now()` style defaults are stored as a String value (not Integer/Float),
    // so even with a parseable CHECK the type mismatch path triggers silent pass.
    let table = table_with(
        "events",
        col_with_default(
            "at",
            ColumnType::Simple(SimpleColumnType::Timestamp),
            DefaultValue::String("now()".into()),
        ),
        vec![check_constraint("chk_some_int", "at > 0")],
    );
    assert!(validate_one(table).is_ok());
}

// ---------------------------------------------------------------------------
// Aggregation: only the right error fires
// ---------------------------------------------------------------------------

#[test]
fn no_default_means_no_check_against_check() {
    // Column has no default — F86 has nothing to evaluate.
    let table = table_with(
        "orders",
        col("amount", ColumnType::Simple(SimpleColumnType::Integer)),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn table_without_check_constraints_is_passthrough() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn error_message_includes_all_context() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_positive", "amount > 0")],
    );
    let err = validate_one(table).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("orders.amount"));
    assert!(msg.contains("amount > 0"));
    assert!(msg.contains('0'));
}

// =====================================================================
// COVERAGE-CLOSURE: exhaustively exercise every apply_op_* arm and
// literal_equals branch through the public validate_schema entry point.
// Each test crafts a (default, CHECK expr) pair such that the F86
// classifier reaches one of:
//   - apply_op_i64 (Eq/Ne/Lt/Le/Gt/Ge)        — L104-112
//   - apply_op_f64 (all six ops)              — L115-125
//   - apply_op_str (all six ops)              — L127-136
//   - apply_op_bool (Eq, Ne, ordering pass)   — L138-145
//   - literal_equals (Float/Int/Bool/String)  — L148-157
//   - check_satisfied SimpleColumnCheck::In   — L71-72
// =====================================================================

// -- apply_op_i64: cover Le explicitly (Eq/Ne/Lt/Gt/Ge handled by
//    existing tests). --------------------------------------------------

#[test]
fn integer_default_5_satisfies_le_check() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(5),
        ),
        vec![check_constraint("chk_le", "amount <= 10")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn integer_default_zero_violates_eq_one() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_eq_one", "amount = 1")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn integer_default_zero_satisfies_le_one() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_le_one", "amount <= 1")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn integer_default_ge_violates_check() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_ge_one", "amount >= 1")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

// -- apply_op_f64: Float default vs Float / Integer literals;
//    also Integer default vs Float literal (cross-kind). --------------

#[test]
fn float_default_violates_gt_check() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(0.1),
        ),
        vec![check_constraint("chk_gt_half", "ratio > 0.5")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn float_default_satisfies_lt_check() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(0.1),
        ),
        vec![check_constraint("chk_lt_one", "ratio < 1.0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn float_default_violates_eq_check() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(0.1),
        ),
        vec![check_constraint("chk_eq_pt5", "ratio = 0.5")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn float_default_satisfies_ne_check() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(0.1),
        ),
        vec![check_constraint("chk_ne_zero", "ratio <> 0.0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn float_default_violates_le_check() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(1.5),
        ),
        vec![check_constraint("chk_le_one", "ratio <= 1.0")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn float_default_satisfies_ge_check() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(2.0),
        ),
        vec![check_constraint("chk_ge_one", "ratio >= 1.0")],
    );
    assert!(validate_one(table).is_ok());
}

// Integer default vs Float literal (i64_to_f64 promotion).
#[test]
fn integer_default_against_float_literal_violates() {
    let table = table_with(
        "orders",
        col_with_default(
            "amount",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(0),
        ),
        vec![check_constraint("chk_gt_half", "amount > 0.5")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

// Float default vs Integer literal (i64_to_f64 promotion on RHS).
#[test]
fn float_default_against_integer_literal_satisfies() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(2.5),
        ),
        vec![check_constraint("chk_gt_int", "ratio > 1")],
    );
    assert!(validate_one(table).is_ok());
}

// -- apply_op_str: every op on a String default + String literal. ----

#[test]
fn string_default_violates_lt_check() {
    let table = table_with(
        "items",
        col_with_default(
            "code",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'zz'".into()),
        ),
        vec![check_constraint("chk_lt", "code < 'aa'")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn string_default_satisfies_gt_check() {
    let table = table_with(
        "items",
        col_with_default(
            "code",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'zz'".into()),
        ),
        vec![check_constraint("chk_gt", "code > 'aa'")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_satisfies_le_check() {
    let table = table_with(
        "items",
        col_with_default(
            "code",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'aa'".into()),
        ),
        vec![check_constraint("chk_le", "code <= 'zz'")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_satisfies_ge_check() {
    let table = table_with(
        "items",
        col_with_default(
            "code",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'zz'".into()),
        ),
        vec![check_constraint("chk_ge", "code >= 'aa'")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_satisfies_ne_check() {
    let table = table_with(
        "items",
        col_with_default(
            "code",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'aa'".into()),
        ),
        vec![check_constraint("chk_ne", "code <> 'bb'")],
    );
    assert!(validate_one(table).is_ok());
}

// -- apply_op_bool: Ne arm + ordering-fallthrough (Lt/Le/Gt/Ge return
//    true — boolean comparisons aren't judged). ----------------------

#[test]
fn bool_default_satisfies_ne_check() {
    let table = table_with(
        "flags",
        col_with_default(
            "enabled",
            ColumnType::Simple(SimpleColumnType::Boolean),
            DefaultValue::Bool(true),
        ),
        vec![check_constraint("chk_ne", "enabled <> false")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn bool_default_violates_eq_check() {
    let table = table_with(
        "flags",
        col_with_default(
            "enabled",
            ColumnType::Simple(SimpleColumnType::Boolean),
            DefaultValue::Bool(false),
        ),
        vec![check_constraint("chk_eq", "enabled = true")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn bool_default_silent_pass_on_ordering_check() {
    // Booleans don't compare with `<` semantically; the helper
    // intentionally returns true (satisfied) so the migration is
    // allowed through.
    let table = table_with(
        "flags",
        col_with_default(
            "enabled",
            ColumnType::Simple(SimpleColumnType::Boolean),
            DefaultValue::Bool(true),
        ),
        vec![check_constraint("chk_lt", "enabled < true")],
    );
    assert!(validate_one(table).is_ok());
}

// -- literal_equals: cover every typed pair through `IN` lists. ------

#[test]
fn integer_default_violates_int_in_list() {
    let table = table_with(
        "orders",
        col_with_default(
            "qty",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(99),
        ),
        vec![check_constraint("chk_in", "qty IN (1, 2, 3)")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}

#[test]
fn float_default_satisfies_float_in_list() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(1.5),
        ),
        vec![check_constraint("chk_in", "ratio IN (1.5, 2.5)")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn integer_default_satisfies_float_in_list_via_promotion() {
    let table = table_with(
        "rates",
        col_with_default(
            "qty",
            ColumnType::Simple(SimpleColumnType::Integer),
            DefaultValue::Integer(2),
        ),
        vec![check_constraint("chk_in", "qty IN (1.0, 2.0, 3.0)")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn float_default_satisfies_int_in_list_via_promotion() {
    let table = table_with(
        "rates",
        col_with_default(
            "ratio",
            ColumnType::Simple(SimpleColumnType::Real),
            DefaultValue::Float(2.0),
        ),
        vec![check_constraint("chk_in", "ratio IN (1, 2, 3)")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn bool_default_satisfies_bool_in_list() {
    let table = table_with(
        "flags",
        col_with_default(
            "enabled",
            ColumnType::Simple(SimpleColumnType::Boolean),
            DefaultValue::Bool(true),
        ),
        vec![check_constraint("chk_in", "enabled IN (true, false)")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_satisfies_string_in_list() {
    let table = table_with(
        "users",
        col_with_default(
            "role",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'admin'".into()),
        ),
        vec![check_constraint(
            "chk_in",
            "role IN ('admin', 'user', 'guest')",
        )],
    );
    assert!(validate_one(table).is_ok());
}

// Type-mismatched default vs literal (string default vs integer
// literal etc.) falls through the wildcard arms in evaluate_op and
// literal_equals to silently pass.
#[test]
fn string_default_against_integer_check_silent_pass() {
    let table = table_with(
        "users",
        col_with_default(
            "role",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'admin'".into()),
        ),
        vec![check_constraint("chk_gt", "role > 0")],
    );
    assert!(validate_one(table).is_ok());
}

#[test]
fn string_default_against_integer_in_list_violates_after_literal_mismatch() {
    let table = table_with(
        "users",
        col_with_default(
            "role",
            ColumnType::Simple(SimpleColumnType::Text),
            DefaultValue::String("'admin'".into()),
        ),
        vec![check_constraint("chk_in", "role IN (1, 2)")],
    );
    assert!(is_default_violates_check(&validate_one(table).unwrap_err()));
}
