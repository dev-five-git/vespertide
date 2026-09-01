#![expect(
    clippy::match_same_arms,
    reason = "type-compatibility match arms intentionally split by semantic policy (each variant pair documents a distinct backend behaviour) even when bodies coincide"
)]
//! Fault **F-novel-4** - CHECK literal type-mismatch detection.
//!
//! A CHECK constraint of the form `col <op> <literal>` where the
//! literal type is **demonstrably incompatible** with the column's
//! declared type. Examples:
//!
//! - `int_col = 'abc'` (integer column compared to a string literal)
//! - `text_col > 42` (text column compared to an integer)
//! - `bool_col = 'x'` (boolean column compared to a non-boolean string)
//! - `uuid_col > 0` (UUID column compared to a number)
//!
//! These are almost always authoring errors. Some backends silently
//! coerce (`MySQL` will cast aggressively, `SQLite` is dynamically
//! typed), but `PostgreSQL` rejects them at `ADD CONSTRAINT` time.
//! Surfacing them statically catches the bug on every backend.
//!
//! # Detection
//!
//! Parses each CHECK expression via [`super::check_expr_parser`] and
//! walks every `Compare`, `In`, and `Between` node. For each node:
//!
//! 1. Look up the referenced column's type in the **baseline** schema
//!    or, if not present there, in any same-plan `CreateTable` action.
//! 2. If the column is *unknown* (typo in column name, etc.), skip
//!    silently — F4 / `validate_schema` already cover unknown-column
//!    diagnostics for this constraint.
//! 3. Compare the literal kind against the column type using a
//!    deliberately *conservative* compatibility table:
//!    only flag combinations that are **definitely** incompatible
//!    on every supported backend. Anything ambiguous (numeric vs
//!    numeric, JSON vs anything, custom column types) silently
//!    passes.
//!
//! # Suppression rules (conservative)
//!
//! - `NULL` literal always passes (universally type-compatible).
//! - `JSON` / `JSONB` columns silently pass (values are polymorphic).
//! - `Custom` column types silently pass (backend-specific semantics).
//! - Numeric column + numeric literal (integer or float, including
//!   cross-kind like int column + float literal) silently passes —
//!   coercion is well-defined on all backends.
//! - Unknown column references silently pass (covered by other
//!   validators).
//! - `Unparseable` CHECK expressions silently pass (parser already
//!   excludes them from analysis, same as F29 / F86).
//!
//! # Severity
//!
//! Emitted as a `Warning` collected into `Vec<CheckTypeMismatchWarning>`
//! rather than a hard `PlannerError`. Rationale: while the literal is
//! definitely the wrong type, the *intent* may still be recoverable
//! (e.g. the user could intend a string column they forgot to declare,
//! or a defensive cast on the database side). The CLI presents the
//! warning interactively (Proceed / Cancel) following the F4 pattern.

use std::collections::HashMap;

use vespertide_core::{
    ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan, SimpleColumnType,
    TableConstraint, TableDef,
};

use super::check_expr_parser::{CheckExpr, Literal, parse};

/// One CHECK literal-type mismatch site needing user resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckTypeMismatchWarning {
    /// Plan-action index of the triggering action.
    pub action_index: usize,
    /// Table the CHECK lives on.
    pub table: String,
    /// CHECK constraint name (may be auto-generated for inline shapes).
    pub constraint_name: String,
    /// Column whose type was mismatched.
    pub column: String,
    /// Human-readable rendering of the column's declared type.
    pub column_type_label: String,
    /// As-written literal that triggered the mismatch.
    pub literal_text: String,
    /// Literal kind name (Integer / Float / String / Bool).
    pub literal_kind: String,
    /// Verbatim CHECK expression for context.
    pub expr: String,
}

/// Scan `plan` against `baseline` for CHECK literal type-mismatch
/// sites. Returns warnings in plan-order. Empty when every
/// detectable CHECK literal type-matches its column.
#[must_use]
pub fn find_check_type_mismatches(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<CheckTypeMismatchWarning> {
    let type_map = build_column_type_map(plan, baseline);
    let mut out = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        match action {
            MigrationAction::CreateTable {
                table, constraints, ..
            } => {
                for constraint in constraints {
                    if let TableConstraint::Check { name, expr, .. } = constraint {
                        scan_check(idx, table.as_str(), name, expr, &type_map, &mut out);
                    }
                }
            }
            MigrationAction::AddConstraint {
                table,
                constraint: TableConstraint::Check { name, expr, .. },
            } => {
                scan_check(idx, table.as_str(), name, expr, &type_map, &mut out);
            }
            MigrationAction::ReplaceConstraint {
                table,
                to: TableConstraint::Check { name, expr, .. },
                ..
            } => {
                scan_check(idx, table.as_str(), name, expr, &type_map, &mut out);
            }
            _ => {}
        }
    }
    out
}

fn scan_check(
    action_index: usize,
    table: &str,
    check_name: &str,
    expr: &str,
    type_map: &HashMap<(&str, &str), &ColumnType>,
    out: &mut Vec<CheckTypeMismatchWarning>,
) {
    let parsed = parse(expr);
    walk_check_expr(
        &parsed,
        action_index,
        table,
        check_name,
        expr,
        type_map,
        out,
    );
}

fn walk_check_expr(
    expr_node: &CheckExpr,
    action_index: usize,
    table: &str,
    check_name: &str,
    expr_text: &str,
    type_map: &HashMap<(&str, &str), &ColumnType>,
    out: &mut Vec<CheckTypeMismatchWarning>,
) {
    match expr_node {
        // For each predicate node the column type is resolved ONCE here;
        // unknown columns skip silently (other validators diagnose them),
        // and `check_one` becomes a pure literal-vs-type check.
        CheckExpr::Compare { column, value, .. } => {
            if let Some(&col_type) = type_map.get(&(table, column.as_str())) {
                check_one(
                    action_index,
                    table,
                    check_name,
                    column,
                    col_type,
                    value,
                    expr_text,
                    out,
                );
            }
        }
        CheckExpr::In { column, values, .. } => {
            if let Some(&col_type) = type_map.get(&(table, column.as_str())) {
                for v in values {
                    check_one(
                        action_index,
                        table,
                        check_name,
                        column,
                        col_type,
                        v,
                        expr_text,
                        out,
                    );
                }
            }
        }
        CheckExpr::Between {
            column, low, high, ..
        } => {
            if let Some(&col_type) = type_map.get(&(table, column.as_str())) {
                for boundary in [low, high] {
                    check_one(
                        action_index,
                        table,
                        check_name,
                        column,
                        col_type,
                        boundary,
                        expr_text,
                        out,
                    );
                }
            }
        }
        CheckExpr::And(parts) | CheckExpr::Or(parts) => {
            for p in parts {
                walk_check_expr(p, action_index, table, check_name, expr_text, type_map, out);
            }
        }
        CheckExpr::Not(inner) => {
            walk_check_expr(
                inner,
                action_index,
                table,
                check_name,
                expr_text,
                type_map,
                out,
            );
        }
        CheckExpr::IsNull { .. } | CheckExpr::Unparseable => {}
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "F-novel-4 per-literal checker threads context (table / check_name / column / col_type / literal / expr / out) through a single call; bundling into a struct would scatter the call sites without aiding clarity"
)]
fn check_one(
    action_index: usize,
    table: &str,
    check_name: &str,
    column: &str,
    col_type: &ColumnType,
    literal: &Literal,
    expr_text: &str,
    out: &mut Vec<CheckTypeMismatchWarning>,
) {
    if !is_definitely_mismatch(col_type, literal) {
        return;
    }
    out.push(CheckTypeMismatchWarning {
        action_index,
        table: table.to_string(),
        constraint_name: check_name.to_string(),
        column: column.to_string(),
        column_type_label: column_type_label(col_type),
        literal_text: literal.display_value(),
        literal_kind: literal_kind_name(literal).to_string(),
        expr: expr_text.to_string(),
    });
}

/// Borrow-keyed lookup map: `(table, column) -> column type`, borrowing
/// from `plan` + `baseline` so per-predicate lookups in
/// [`walk_check_expr`] are allocation-free. Owned strings are produced
/// only when a warning fires.
fn build_column_type_map<'a>(
    plan: &'a MigrationPlan,
    baseline: &'a [TableDef],
) -> HashMap<(&'a str, &'a str), &'a ColumnType> {
    let mut map = HashMap::new();
    for table in baseline {
        for col in &table.columns {
            map.insert((table.name.as_str(), col.name.as_str()), &col.r#type);
        }
    }
    // Plan-added tables / columns supersede baseline so a CreateTable
    // in the same plan is visible to its own inline CHECKs.
    for action in &plan.actions {
        match action {
            MigrationAction::CreateTable { table, columns, .. } => {
                for col in columns {
                    map.insert((table.as_str(), col.name.as_str()), &col.r#type);
                }
            }
            MigrationAction::AddColumn { table, column, .. } => {
                map.insert((table.as_str(), column.name.as_str()), &column.r#type);
            }
            MigrationAction::ModifyColumnType {
                table,
                column,
                new_type,
                ..
            } => {
                map.insert((table.as_str(), column.as_str()), new_type);
            }
            _ => {}
        }
    }
    map
}

/// Conservative type-compatibility check. Returns `true` only when
/// the literal is *definitely* incompatible with the column type on
/// every supported backend. Ambiguous / coercible / polymorphic
/// combinations silently pass to avoid false positives.
fn is_definitely_mismatch(col_type: &ColumnType, lit: &Literal) -> bool {
    // NULL is universally compatible.
    if matches!(lit, Literal::Null) {
        return false;
    }
    match col_type {
        ColumnType::Simple(simple) => match (simple, lit) {
            // Integer-family columns + non-numeric literal = mismatch.
            (
                SimpleColumnType::SmallInt
                | SimpleColumnType::Integer
                | SimpleColumnType::BigInt
                | SimpleColumnType::Real
                | SimpleColumnType::DoublePrecision,
                Literal::String(_) | Literal::Bool(_),
            ) => true,
            // Text + non-string literal = mismatch.
            (
                SimpleColumnType::Text,
                Literal::Integer(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // Boolean + Float / String = mismatch.
            (SimpleColumnType::Boolean, Literal::Float(_) | Literal::String(_)) => true,
            // Boolean + Integer is borderline: 0/1 are accepted as
            // bool aliases on MySQL / SQLite (and PG with explicit
            // cast). Be conservative: only flag integers that are
            // demonstrably *not* 0 or 1.
            (SimpleColumnType::Boolean, Literal::Integer(i)) => *i != 0 && *i != 1,
            // UUID + non-string = mismatch.
            (
                SimpleColumnType::Uuid,
                Literal::Integer(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // Date / Time / Timestamp / Interval + non-string = mismatch.
            (
                SimpleColumnType::Date
                | SimpleColumnType::Time
                | SimpleColumnType::Timestamp
                | SimpleColumnType::Timestamptz
                | SimpleColumnType::Interval,
                Literal::Integer(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // Binary + Bool = mismatch. (String literals may be hex
            // / escape-encoded bytea so we leave those alone.)
            (SimpleColumnType::Bytea, Literal::Bool(_)) => true,
            // Network / XML types: any non-string is a clear mismatch.
            (
                SimpleColumnType::Inet
                | SimpleColumnType::Cidr
                | SimpleColumnType::Macaddr
                | SimpleColumnType::Xml,
                Literal::Integer(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // JSON column is polymorphic — any literal can be a valid
            // JSON scalar fragment. Always pass.
            (SimpleColumnType::Json, _) => false,
            // Every other combination is permissible or ambiguous.
            _ => false,
        },
        ColumnType::Complex(complex) => match (complex, lit) {
            // Varchar / Char + non-string = mismatch.
            (
                ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. },
                Literal::Integer(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // Numeric + non-numeric = mismatch.
            (ComplexColumnType::Numeric { .. }, Literal::String(_) | Literal::Bool(_)) => true,
            // String enum + non-string = mismatch.
            (
                ComplexColumnType::Enum {
                    values: EnumValues::String(_),
                    ..
                },
                Literal::Integer(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // Integer enum + non-integer = mismatch.
            (
                ComplexColumnType::Enum {
                    values: EnumValues::Integer(_),
                    ..
                },
                Literal::String(_) | Literal::Float(_) | Literal::Bool(_),
            ) => true,
            // Custom column types: backend-specific semantics, skip.
            (ComplexColumnType::Custom { .. }, _) => false,
            _ => false,
        },
    }
}

fn column_type_label(col_type: &ColumnType) -> String {
    // Wire-format spelling (`small_int`, not `smallint`) so the warning
    // echoes exactly what the user wrote in the model file. Delegates to
    // the shared renderer beside `SimpleColumnType::model_name` in core.
    col_type.display_label()
}

fn literal_kind_name(lit: &Literal) -> &'static str {
    match lit {
        Literal::Integer(_) => "Integer",
        Literal::Float(_) => "Float",
        Literal::String(_) => "String",
        Literal::Bool(_) => "Bool",
        Literal::Null => "Null",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_check, check, col, plan};
    use vespertide_core::{ColumnDef, ColumnType, EnumValues, SimpleColumnType, TableDef};

    fn baseline_table(table: &str, cols: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: table.into(),
            description: None,
            columns: cols,
            constraints: Vec::new(),
        }
    }

    // A numeric column compared to a string literal is a mismatch; the
    // warning's type label must render as `numeric(P, S)`. Pins the
    // `complex_type_label` Numeric arm (deleting it falls through to the
    // `_ => "complex"` catch-all).
    #[test]
    fn numeric_column_mismatch_labels_precision_and_scale() {
        use vespertide_core::ComplexColumnType;
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "amt",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "amt = 'x'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column_type_label, "numeric(10, 2)");
    }

    // -- Definite mismatches (warning emitted) --------------------------

    #[test]
    fn integer_column_compared_to_string_literal_is_mismatch() {
        let baseline = vec![baseline_table(
            "users",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![add_check("users", "chk_age", "age = 'abc'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column, "age");
        assert_eq!(warnings[0].column_type_label, "integer");
        assert_eq!(warnings[0].literal_kind, "String");
    }

    #[test]
    fn text_column_compared_to_integer_literal_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("name", ColumnType::Simple(SimpleColumnType::Text))],
        )];
        let p = plan(vec![add_check("t", "chk", "name > 42")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].literal_kind, "Integer");
    }

    #[test]
    fn boolean_column_with_non_bool_string_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("flag", ColumnType::Simple(SimpleColumnType::Boolean))],
        )];
        let p = plan(vec![add_check("t", "chk", "flag = 'x'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn boolean_column_with_borderline_integer_passes() {
        // 0 and 1 are accepted as boolean aliases on most backends.
        let baseline = vec![baseline_table(
            "t",
            vec![col("flag", ColumnType::Simple(SimpleColumnType::Boolean))],
        )];
        let p = plan(vec![add_check("t", "chk", "flag = 1")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert!(warnings.is_empty());
    }

    #[test]
    fn boolean_column_with_out_of_range_integer_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("flag", ColumnType::Simple(SimpleColumnType::Boolean))],
        )];
        let p = plan(vec![add_check("t", "chk", "flag = 5")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn uuid_column_compared_to_number_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Uuid))],
        )];
        let p = plan(vec![add_check("t", "chk", "id > 0")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn date_column_compared_to_number_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("d", ColumnType::Simple(SimpleColumnType::Date))],
        )];
        let p = plan(vec![add_check("t", "chk", "d > 100")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn varchar_column_compared_to_integer_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "name",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "name > 42")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column_type_label, "varchar(100)");
    }

    #[test]
    fn numeric_column_compared_to_string_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "price",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "price = 'free'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn string_enum_column_with_integer_literal_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "status = 42")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn in_list_each_literal_checked() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        // Two of three literals are strings.
        let p = plan(vec![add_check("t", "chk", "age IN (1, 'two', 'three')")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn between_both_boundaries_checked() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![add_check("t", "chk", "age BETWEEN 'low' AND 'high'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn and_composition_collects_per_branch() {
        let baseline = vec![baseline_table(
            "t",
            vec![
                col("a", ColumnType::Simple(SimpleColumnType::Integer)),
                col("b", ColumnType::Simple(SimpleColumnType::Text)),
            ],
        )];
        let p = plan(vec![add_check("t", "chk", "a = 'x' AND b = 99")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 2);
    }

    // -- Silently passes (conservative) ---------------------------------

    #[test]
    fn numeric_column_with_integer_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![add_check("t", "chk", "age > 0")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn numeric_column_with_float_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("ratio", ColumnType::Simple(SimpleColumnType::Real))],
        )];
        let p = plan(vec![add_check("t", "chk", "ratio > 0.5")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn text_column_with_string_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("name", ColumnType::Simple(SimpleColumnType::Text))],
        )];
        let p = plan(vec![add_check("t", "chk", "name = 'alice'")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn uuid_column_with_string_passes() {
        // String might be a UUID literal — we can't validate format easily.
        let baseline = vec![baseline_table(
            "t",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Uuid))],
        )];
        let p = plan(vec![add_check("t", "chk", "id = 'abc-def-ghi'")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn null_literal_always_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![add_check("t", "chk", "age = NULL")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn json_column_with_any_literal_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("data", ColumnType::Simple(SimpleColumnType::Json))],
        )];
        let p = plan(vec![add_check("t", "chk", "data = 'whatever'")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn custom_type_silently_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "x",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "TSVECTOR".into(),
                }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "x = 42")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn unknown_column_silently_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![add_check("t", "chk", "missing_col = 'x'")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    /// Unknown-column skip is decided ONCE per predicate node — pin the
    /// `In` and `Between` arms of `walk_check_expr` (not just `Compare`).
    #[test]
    fn unknown_column_in_list_and_between_silently_pass() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![
            add_check("t", "chk_in", "missing_col IN ('a', 'b')"),
            add_check("t", "chk_between", "missing_col BETWEEN 'x' AND 'y'"),
        ]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn unparseable_check_silently_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("name", ColumnType::Simple(SimpleColumnType::Text))],
        )];
        let p = plan(vec![add_check("t", "chk", "LOWER(name) = 'x'")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    #[test]
    fn create_table_inline_check_uses_plan_columns() {
        // baseline is empty; the column type comes from the
        // CreateTable action itself.
        let p = plan(vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![check("chk_age", "age = 'abc'")],
        }]);
        let warnings = find_check_type_mismatches(&p, &[]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column_type_label, "integer");
    }

    // ── Coverage-closure: ReplaceConstraint(Check), AddColumn / ModifyColumnType
    // type-map paths, plus remaining sub-arms of is_definitely_mismatch ──

    use rstest::rstest;

    /// `ReplaceConstraint { to: Check { .. }, .. }` triggers `scan_check`
    /// on the replacement expression. Exercises the `ReplaceConstraint`
    /// arm in `find_check_type_mismatches` (line 117-123).
    #[rstest]
    fn replace_constraint_check_is_scanned() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![MigrationAction::ReplaceConstraint {
            table: "t".into(),
            from: check("old", "age > 0"),
            to: check("new", "age = 'bad'"),
        }]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].constraint_name, "new");
    }

    /// `MigrationAction::AddColumn` feeds its column into the type-map so
    /// a downstream `AddConstraint(Check)` referencing the freshly-added
    /// column resolves correctly. Covers `build_column_type_map` AddColumn
    /// arm (line 287-292).
    #[rstest]
    fn add_column_then_check_on_same_column_uses_plan_type_map() {
        let baseline = vec![baseline_table("t", vec![])];
        let new_col = col("status", ColumnType::Simple(SimpleColumnType::Boolean));
        let p = plan(vec![
            MigrationAction::AddColumn {
                table: "t".into(),
                column: Box::new(new_col),
                fill_with: None,
            },
            MigrationAction::AddConstraint {
                table: "t".into(),
                constraint: check("chk_status", "status = 'x'"),
            },
        ]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column, "status");
    }

    /// `MigrationAction::ModifyColumnType` updates the type-map so a
    /// CHECK in the same plan compares against the new type. Covers
    /// `build_column_type_map` ModifyColumnType arm (line 293-300).
    #[rstest]
    fn modify_column_type_updates_type_map_for_check() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("v", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![
            MigrationAction::ModifyColumnType {
                table: "t".into(),
                column: "v".into(),
                new_type: ColumnType::Simple(SimpleColumnType::Text),
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            },
            MigrationAction::AddConstraint {
                table: "t".into(),
                constraint: check("chk_v", "v > 42"),
            },
        ]);
        // Post-modify type is Text → integer literal 42 mismatches.
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column_type_label, "text");
        assert_eq!(warnings[0].literal_kind, "Integer");
    }

    /// `Real` / `DoublePrecision` column + non-numeric literal → mismatch
    /// (line 319-326).
    #[rstest]
    #[case::real(SimpleColumnType::Real)]
    #[case::double(SimpleColumnType::DoublePrecision)]
    fn real_family_column_with_string_literal_is_mismatch(#[case] ty: SimpleColumnType) {
        let baseline = vec![baseline_table("t", vec![col("v", ColumnType::Simple(ty))])];
        let p = plan(vec![add_check("t", "chk", "v = 'abc'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
    }

    /// `Bytea` + `Bool` literal → mismatch (line 354-355).
    #[rstest]
    fn bytea_column_with_bool_literal_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("blob", ColumnType::Simple(SimpleColumnType::Bytea))],
        )];
        let p = plan(vec![add_check("t", "chk", "blob = TRUE")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].literal_kind, "Bool");
    }

    /// Network / XML column families + non-string literal → mismatch
    /// (lines 357-363). Exercises Inet / Cidr / Macaddr / Xml.
    #[rstest]
    #[case::inet(SimpleColumnType::Inet)]
    #[case::cidr(SimpleColumnType::Cidr)]
    #[case::macaddr(SimpleColumnType::Macaddr)]
    #[case::xml(SimpleColumnType::Xml)]
    fn network_xml_column_with_integer_literal_is_mismatch(#[case] ty: SimpleColumnType) {
        let baseline = vec![baseline_table("t", vec![col("v", ColumnType::Simple(ty))])];
        let p = plan(vec![add_check("t", "chk", "v = 42")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].literal_kind, "Integer");
    }

    /// `Char` complex column + numeric literal → mismatch (line 372-375).
    #[rstest]
    fn char_column_with_integer_literal_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "code",
                ColumnType::Complex(ComplexColumnType::Char { length: 2 }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "code = 99")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].column_type_label, "char(2)");
    }

    /// `Integer enum` complex column + string literal → mismatch
    /// (line 387-393).
    #[rstest]
    fn integer_enum_column_with_string_literal_is_mismatch() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        vespertide_core::NumValue {
                            name: "low".into(),
                            value: 0,
                        },
                        vespertide_core::NumValue {
                            name: "high".into(),
                            value: 10,
                        },
                    ]),
                }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "priority = 'low'")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].literal_kind, "String");
        assert_eq!(warnings[0].column_type_label, "enum(priority_level)");
    }

    /// Numeric column + numeric literal → passes (Complex `_ => false`
    /// fallthrough at line 396).
    #[rstest]
    fn numeric_column_with_integer_literal_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "amount > 100")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    /// Varchar column + string literal → passes (Complex `_ => false`
    /// fallthrough at line 396, via the (Varchar, _) shape that does NOT
    /// match the explicit "Varchar/Char + non-string" arm).
    #[rstest]
    fn varchar_column_with_string_literal_passes() {
        let baseline = vec![baseline_table(
            "t",
            vec![col(
                "name",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
            )],
        )];
        let p = plan(vec![add_check("t", "chk", "name = 'alice'")]);
        assert!(find_check_type_mismatches(&p, &baseline).is_empty());
    }

    /// Direct unit test for `literal_kind_name` Null arm (line 425) and
    /// the unified `Literal::display_value` Null arm: both are unreachable
    /// from the public flow because `is_definitely_mismatch` returns
    /// false for `Null` literals before the formatters run.
    #[rstest]
    fn literal_kind_name_and_display_value_cover_null_and_float_arms() {
        assert_eq!(literal_kind_name(&Literal::Integer(1)), "Integer");
        assert_eq!(literal_kind_name(&Literal::Float(1.0)), "Float");
        assert_eq!(literal_kind_name(&Literal::String("x".into())), "String");
        assert_eq!(literal_kind_name(&Literal::Bool(true)), "Bool");
        assert_eq!(literal_kind_name(&Literal::Null), "Null");

        assert_eq!(Literal::Integer(7).display_value(), "7");
        assert_eq!(Literal::Float(1.5).display_value(), "1.5");
        assert_eq!(Literal::String("alice".into()).display_value(), "alice");
        assert_eq!(Literal::Bool(false).display_value(), "false");
        assert_eq!(Literal::Null.display_value(), "NULL");
    }

    #[rstest]
    fn not_wrapped_check_expression_is_recursed() {
        let baseline = vec![baseline_table(
            "t",
            vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        )];
        let p = plan(vec![add_check("t", "chk", "NOT (age = 'bad')")]);
        let warnings = find_check_type_mismatches(&p, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].expr, "NOT (age = 'bad')");
    }

    #[rstest]
    fn column_type_label_custom_renders_type_name() {
        let label = column_type_label(&ColumnType::Complex(ComplexColumnType::Custom {
            custom_type: "TSVECTOR".into(),
        }));
        assert_eq!(label, "custom(TSVECTOR)");
    }

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "small_int")]
    #[case::big_int(SimpleColumnType::BigInt, "big_int")]
    #[case::double_precision(SimpleColumnType::DoublePrecision, "double_precision")]
    fn column_type_label_uses_wire_format_for_multi_word_simple_types(
        #[case] simple: SimpleColumnType,
        #[case] expected: &str,
    ) {
        // A user who wrote `"small_int"` must be warned about `small_int`,
        // not the Debug-derived `smallint`.
        assert_eq!(column_type_label(&ColumnType::Simple(simple)), expected);
    }
}
