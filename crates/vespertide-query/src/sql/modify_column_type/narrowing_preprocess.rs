//! Pre-processing SQL that transforms existing rows so a subsequent
//! `ALTER COLUMN TYPE` cannot fail because of a narrowed value range.
//!
//! Emitted *before* the ALTER statement in the same migration. The
//! combinations matrix is governed by the user's chosen
//! [`NarrowingStrategy`] (set during `vespertide revision`) and the
//! detected [`NarrowingKind`] (computed from old vs new type):
//!
//! | Strategy        | string length | numeric scale | numeric int | integer | float / tz |
//! |-----------------|---------------|---------------|-------------|---------|------------|
//! | `truncate`      | `LEFT`/`substr` | `ROUND`     | ❌          | ❌      | ❌         |
//! | `delete`        | violating row | violating row | violating row | violating row | ❌ |
//! | `set_to_value`  | violating row | violating row | violating row | violating row | ❌ |
//!
//! Kinds marked ❌ return [`QueryError::UnsupportedAction`] — the CLI's
//! Select UI restricts strategies to applicable choices, so reaching an
//! unsupported combination indicates either a hand-edited migration file
//! or a future Vespertide version that added a kind without updating this
//! matrix.

use vespertide_core::{ColumnType, NarrowingStrategy, TableDef};
use vespertide_planner::{NarrowingKind, is_narrowing};

use super::super::helpers::quote_ident;
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Emit zero or more pre-processing statements (UPDATE / DELETE) that
/// transform every row violating the new type so the subsequent ALTER will
/// succeed.
///
/// Returns `Ok(vec![])` when there is no narrowing (e.g. widening, or the
/// new type lives in a different category and is not classified by the
/// detector). Returns `Err(QueryError::UnsupportedAction)` only when the
/// caller's chosen `strategy` is not implemented for the detected kind.
pub fn build_narrowing_preprocess(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    strategy: &NarrowingStrategy,
    baseline: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let Some(old_type) = lookup_old_type(table, column, baseline) else {
        // Column not present in baseline — typically a CreateTable +
        // ModifyColumnType in the same plan. Nothing to pre-process.
        return Ok(vec![]);
    };
    let Some(kind) = is_narrowing(&old_type, new_type) else {
        // User supplied a strategy but the type change is not actually a
        // narrowing (e.g. widening or unrelated swap). Silently skip.
        return Ok(vec![]);
    };

    if let NarrowingStrategy::Truncate = strategy {
        build_truncate(backend, table, column, &kind)
    } else if let NarrowingStrategy::SetToValue { value } = strategy {
        build_set_to_value(backend, table, column, &kind, value)
    } else {
        build_delete(backend, table, column, &kind)
    }
}

fn lookup_old_type(table: &str, column: &str, baseline: &[TableDef]) -> Option<ColumnType> {
    baseline
        .iter()
        .find(|t| t.name == table)?
        .columns
        .iter()
        .find(|c| c.name == column)
        .map(|c| c.r#type.clone())
}

// ---------------------------------------------------------------------------
// Strategy: Truncate
// ---------------------------------------------------------------------------

fn build_truncate(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    kind: &NarrowingKind,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let (new_len, predicate_length) = match kind {
        NarrowingKind::VarcharLength { to, .. }
        | NarrowingKind::CharLength { to, .. }
        | NarrowingKind::VarcharToCharShorter { to, .. }
        | NarrowingKind::CharToVarcharShorter { to, .. } => (*to, true),
        NarrowingKind::TextToVarchar { to_length } | NarrowingKind::TextToChar { to_length } => {
            (*to_length, true)
        }
        NarrowingKind::NumericScale { to_scale, .. } => {
            // Decimal-place trim via ROUND(col, new_scale). The same SQL
            // works on every backend.
            return Ok(vec![numeric_round_update(
                backend, table, column, *to_scale,
            )]);
        }
        NarrowingKind::NumericIntegerDigits { .. }
        | NarrowingKind::IntegerSize { .. }
        | NarrowingKind::FloatSize { .. }
        | NarrowingKind::TimestamptzToTimestamp => {
            return Err(QueryError::UnsupportedAction(format!(
                "narrowing_strategy=truncate is not defined for {kind:?}; \
                 use `delete` or `set_to_value` instead"
            )));
        }
    };
    let _ = predicate_length;
    Ok(vec![string_left_update(backend, table, column, new_len)])
}

/// Truncate every value longer than the new length:
///   PG:     `UPDATE table SET col = LEFT(col, N) WHERE LENGTH(col) > N`
///   `MySQL`:  `UPDATE table SET col = LEFT(col, N) WHERE CHAR_LENGTH(col) > N`
///   `SQLite`: `UPDATE table SET col = substr(col, 1, N) WHERE length(col) > N`
///
/// Both the WHERE clause and the `LEFT()`/`substr()` arguments measure in
/// *characters* on every backend, matching how each backend enforces
/// `VARCHAR(N)` length. Mismatching units (e.g. byte LENGTH on `MySQL`)
/// would silently miss multi-byte rows.
fn string_left_update(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_len: u32,
) -> BuiltQuery {
    let t = quote_ident(table, backend);
    let c = quote_ident(column, backend);
    let sql = match backend {
        DatabaseBackend::Postgres => {
            format!("UPDATE {t} SET {c} = LEFT({c}, {new_len}) WHERE LENGTH({c}) > {new_len}")
        }
        DatabaseBackend::MySql => {
            format!("UPDATE {t} SET {c} = LEFT({c}, {new_len}) WHERE CHAR_LENGTH({c}) > {new_len}")
        }
        DatabaseBackend::Sqlite => {
            format!("UPDATE {t} SET {c} = substr({c}, 1, {new_len}) WHERE length({c}) > {new_len}")
        }
    };
    BuiltQuery::Raw(RawSql::uniform(sql))
}

/// `UPDATE table SET col = ROUND(col, scale)` — universal syntax.
fn numeric_round_update(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_scale: u32,
) -> BuiltQuery {
    let t = quote_ident(table, backend);
    let c = quote_ident(column, backend);
    let sql = format!("UPDATE {t} SET {c} = ROUND({c}, {new_scale})");
    BuiltQuery::Raw(RawSql::uniform(sql))
}

// ---------------------------------------------------------------------------
// Strategy: Delete (entire row whose value violates)
// ---------------------------------------------------------------------------

fn build_delete(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    kind: &NarrowingKind,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let predicate = violation_predicate(backend, column, kind)?;
    let t = quote_ident(table, backend);
    let sql = format!("DELETE FROM {t} WHERE {predicate}");
    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

// ---------------------------------------------------------------------------
// Strategy: SetToValue
// ---------------------------------------------------------------------------

fn build_set_to_value(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    kind: &NarrowingKind,
    value: &str,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let predicate = violation_predicate(backend, column, kind)?;
    let t = quote_ident(table, backend);
    let c = quote_ident(column, backend);
    let sql = format!("UPDATE {t} SET {c} = {value} WHERE {predicate}");
    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

// ---------------------------------------------------------------------------
// Shared violation predicate
// ---------------------------------------------------------------------------

fn violation_predicate(
    backend: DatabaseBackend,
    column: &str,
    kind: &NarrowingKind,
) -> Result<String, QueryError> {
    let c = quote_ident(column, backend);
    let len_fn = match backend {
        // MySQL's LENGTH counts bytes; CHAR_LENGTH counts characters.
        // For type-narrowing safety we use CHAR_LENGTH on MySQL so
        // multi-byte UTF-8 strings are measured the same way the column
        // length constraint counts them.
        DatabaseBackend::MySql => "CHAR_LENGTH",
        DatabaseBackend::Postgres => "LENGTH",
        DatabaseBackend::Sqlite => "length",
    };
    match kind {
        NarrowingKind::VarcharLength { to, .. }
        | NarrowingKind::CharLength { to, .. }
        | NarrowingKind::VarcharToCharShorter { to, .. }
        | NarrowingKind::CharToVarcharShorter { to, .. } => Ok(format!("{len_fn}({c}) > {to}")),
        NarrowingKind::TextToVarchar { to_length } | NarrowingKind::TextToChar { to_length } => {
            Ok(format!("{len_fn}({c}) > {to_length}"))
        }
        NarrowingKind::NumericScale { to_scale, .. } => {
            // ROUND-based equality: a row violates when rounding actually
            // changes the value. Works identically on every backend.
            Ok(format!("{c} <> ROUND({c}, {to_scale})"))
        }
        NarrowingKind::NumericIntegerDigits { to_int_digits, .. } => {
            // |col| >= 10^to_int_digits implies integer-part overflow.
            let bound = ten_pow_string(*to_int_digits);
            Ok(format!("ABS({c}) >= {bound}"))
        }
        NarrowingKind::IntegerSize { to, .. } => {
            let (min, max) = integer_bounds(to);
            Ok(format!("{c} > {max} OR {c} < {min}"))
        }
        NarrowingKind::FloatSize { .. } => Err(QueryError::UnsupportedAction(format!(
            "narrowing_strategy={kind:?} cannot generate a violation predicate \
             (every value is affected by precision downcast)"
        ))),
        NarrowingKind::TimestamptzToTimestamp => Err(QueryError::UnsupportedAction(
            "narrowing_strategy on timestamptz->timestamp is not supported \
             (every row is reinterpreted; pre-clean via fill_with timezone prompt instead)"
                .into(),
        )),
    }
}

fn ten_pow_string(n: u32) -> String {
    // Render `10^n` literally so the SQL stays free of bind parameters.
    // n=0 → "1", n=4 → "10000". Cap defensively at 38 digits, which is the
    // largest NUMERIC precision any supported backend honours.
    let cap = n.min(38) as usize;
    let mut s = String::with_capacity(cap + 1);
    s.push('1');
    for _ in 0..cap {
        s.push('0');
    }
    s
}

fn integer_bounds(target: &str) -> (i64, i64) {
    match target {
        "smallint" => (i64::from(i16::MIN), i64::from(i16::MAX)),
        "integer" => (i64::from(i32::MIN), i64::from(i32::MAX)),
        // Unknown variant: fall back to bigint bounds, which means the
        // predicate matches nothing and the migration becomes a no-op
        // pre-process — safer than rejecting at this layer.
        _ => (i64::MIN, i64::MAX),
    }
}

#[cfg(test)]
mod integer_bounds_tests {
    use super::*;

    /// L205: direct cover for the `_ => (i64::MIN, i64::MAX)` fallback
    /// arm of `integer_bounds`. Production callers route through
    /// `NarrowingKind::IntegerSize { to: SmallInt | Integer }`, so the
    /// fallback is only reachable from a direct call with an unknown
    /// target string. This module-private fn is reachable from the
    /// child test module.
    #[test]
    fn integer_bounds_smallint_returns_i16_range() {
        assert_eq!(
            integer_bounds("smallint"),
            (i64::from(i16::MIN), i64::from(i16::MAX))
        );
    }

    #[test]
    fn integer_bounds_integer_returns_i32_range() {
        assert_eq!(
            integer_bounds("integer"),
            (i64::from(i32::MIN), i64::from(i32::MAX))
        );
    }

    /// L205 wildcard fallback.
    #[test]
    fn integer_bounds_unknown_target_falls_back_to_bigint_bounds() {
        assert_eq!(integer_bounds("bigint"), (i64::MIN, i64::MAX));
        assert_eq!(integer_bounds("future_int_kind"), (i64::MIN, i64::MAX));
        assert_eq!(integer_bounds(""), (i64::MIN, i64::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, SimpleColumnType, TableDef};

    // -----------------------------------------------------------------------
    // Fixture helpers
    // -----------------------------------------------------------------------

    fn baseline_with(old_type: ColumnType) -> Vec<TableDef> {
        vec![TableDef {
            name: "tbl".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "col".into(),
                r#type: old_type,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }]
    }

    fn varchar(n: u32) -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Varchar { length: n })
    }
    fn char_t(n: u32) -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Char { length: n })
    }
    fn numeric(p: u32, s: u32) -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Numeric {
            precision: p,
            scale: s,
        })
    }
    fn simple(t: SimpleColumnType) -> ColumnType {
        ColumnType::Simple(t)
    }

    fn run(
        backend: DatabaseBackend,
        old: ColumnType,
        new: &ColumnType,
        strategy: &NarrowingStrategy,
    ) -> Result<String, QueryError> {
        let baseline = baseline_with(old);
        let queries = build_narrowing_preprocess(backend, "tbl", "col", new, strategy, &baseline)?;
        Ok(queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n"))
    }

    fn snap(name: &str, sql: &str) {
        with_settings!(
            { snapshot_path => "../snapshots", snapshot_suffix => name },
            { assert_snapshot!(sql); }
        );
    }

    // -----------------------------------------------------------------------
    // Supported matrix: every (kind × strategy) that emits SQL, asserted on
    // all three backends so quoting / function-name divergences are locked.
    // Macro keeps each combination to a single line for auditability.
    // -----------------------------------------------------------------------

    macro_rules! supported_snap {
        ($name:ident, $old:expr, $new:expr, $strategy:expr) => {
            #[test]
            fn $name() {
                for (backend, tag) in [
                    (DatabaseBackend::Postgres, "postgres"),
                    (DatabaseBackend::MySql, "mysql"),
                    (DatabaseBackend::Sqlite, "sqlite"),
                ] {
                    let sql = run(backend, $old, &$new, &$strategy)
                        .expect("supported (kind, strategy) combo");
                    snap(&format!("preprocess_{}_{}", stringify!($name), tag), &sql);
                }
            }
        };
    }

    // --- VARCHAR length: varchar(40) -> varchar(30) ---
    supported_snap!(
        varchar_length_truncate,
        varchar(40),
        varchar(30),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        varchar_length_delete,
        varchar(40),
        varchar(30),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        varchar_length_set_to_value,
        varchar(40),
        varchar(30),
        NarrowingStrategy::SetToValue {
            value: "'TRUNCATED'".into()
        }
    );

    // --- CHAR length: char(10) -> char(5) ---
    supported_snap!(
        char_length_truncate,
        char_t(10),
        char_t(5),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        char_length_delete,
        char_t(10),
        char_t(5),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        char_length_set_to_value,
        char_t(10),
        char_t(5),
        NarrowingStrategy::SetToValue {
            value: "'X'".into()
        }
    );

    // --- VARCHAR -> CHAR (shorter): varchar(20) -> char(10) ---
    supported_snap!(
        varchar_to_char_shorter_truncate,
        varchar(20),
        char_t(10),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        varchar_to_char_shorter_delete,
        varchar(20),
        char_t(10),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        varchar_to_char_shorter_set_to_value,
        varchar(20),
        char_t(10),
        NarrowingStrategy::SetToValue {
            value: "'X'".into()
        }
    );

    // --- CHAR -> VARCHAR (shorter): char(20) -> varchar(10) ---
    supported_snap!(
        char_to_varchar_shorter_truncate,
        char_t(20),
        varchar(10),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        char_to_varchar_shorter_delete,
        char_t(20),
        varchar(10),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        char_to_varchar_shorter_set_to_value,
        char_t(20),
        varchar(10),
        NarrowingStrategy::SetToValue {
            value: "'X'".into()
        }
    );

    // --- TEXT -> VARCHAR(N): text -> varchar(255) ---
    supported_snap!(
        text_to_varchar_truncate,
        simple(SimpleColumnType::Text),
        varchar(255),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        text_to_varchar_delete,
        simple(SimpleColumnType::Text),
        varchar(255),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        text_to_varchar_set_to_value,
        simple(SimpleColumnType::Text),
        varchar(255),
        NarrowingStrategy::SetToValue {
            value: "'TRUNC'".into()
        }
    );

    // --- TEXT -> CHAR(N): text -> char(100) ---
    supported_snap!(
        text_to_char_truncate,
        simple(SimpleColumnType::Text),
        char_t(100),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        text_to_char_delete,
        simple(SimpleColumnType::Text),
        char_t(100),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        text_to_char_set_to_value,
        simple(SimpleColumnType::Text),
        char_t(100),
        NarrowingStrategy::SetToValue {
            value: "'TRUNC'".into()
        }
    );

    // --- NUMERIC scale shrink: numeric(10,4) -> numeric(10,2) ---
    supported_snap!(
        numeric_scale_truncate,
        numeric(10, 4),
        numeric(10, 2),
        NarrowingStrategy::Truncate
    );
    supported_snap!(
        numeric_scale_delete,
        numeric(10, 4),
        numeric(10, 2),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        numeric_scale_set_to_value,
        numeric(10, 4),
        numeric(10, 2),
        NarrowingStrategy::SetToValue { value: "0".into() }
    );

    // --- NUMERIC integer-digits shrink: numeric(12,4) -> numeric(8,4) ---
    // Truncate is UNSUPPORTED for integer-digits — covered in error matrix below.
    supported_snap!(
        numeric_int_digits_delete,
        numeric(12, 4),
        numeric(8, 4),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        numeric_int_digits_set_to_value,
        numeric(12, 4),
        numeric(8, 4),
        NarrowingStrategy::SetToValue { value: "0".into() }
    );

    // --- Integer size: bigint -> integer ---
    supported_snap!(
        bigint_to_integer_delete,
        simple(SimpleColumnType::BigInt),
        simple(SimpleColumnType::Integer),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        bigint_to_integer_set_to_value,
        simple(SimpleColumnType::BigInt),
        simple(SimpleColumnType::Integer),
        NarrowingStrategy::SetToValue { value: "0".into() }
    );

    // --- Integer size: integer -> smallint ---
    supported_snap!(
        integer_to_smallint_delete,
        simple(SimpleColumnType::Integer),
        simple(SimpleColumnType::SmallInt),
        NarrowingStrategy::Delete
    );
    supported_snap!(
        integer_to_smallint_set_to_value,
        simple(SimpleColumnType::Integer),
        simple(SimpleColumnType::SmallInt),
        NarrowingStrategy::SetToValue { value: "0".into() }
    );

    // -----------------------------------------------------------------------
    // Unsupported matrix: every (kind × strategy) that must return
    // `QueryError::UnsupportedAction`. Backend-agnostic; one PG run is enough
    // because the error path is shared across backends.
    // -----------------------------------------------------------------------

    macro_rules! unsupported_combo {
        ($name:ident, $old:expr, $new:expr, $strategy:expr) => {
            #[test]
            fn $name() {
                let result = run(DatabaseBackend::Postgres, $old, &$new, &$strategy);
                assert!(
                    matches!(result, Err(QueryError::UnsupportedAction(_))),
                    "expected UnsupportedAction, got: {result:?}"
                );
            }
        };
    }

    // Truncate has no natural meaning for integer overflow / float precision /
    // timezone reinterpretation.
    unsupported_combo!(
        truncate_unsupported_for_numeric_int_digits,
        numeric(12, 4),
        numeric(8, 4),
        NarrowingStrategy::Truncate
    );
    unsupported_combo!(
        truncate_unsupported_for_bigint_to_integer,
        simple(SimpleColumnType::BigInt),
        simple(SimpleColumnType::Integer),
        NarrowingStrategy::Truncate
    );
    unsupported_combo!(
        truncate_unsupported_for_integer_to_smallint,
        simple(SimpleColumnType::Integer),
        simple(SimpleColumnType::SmallInt),
        NarrowingStrategy::Truncate
    );
    unsupported_combo!(
        truncate_unsupported_for_float_size,
        simple(SimpleColumnType::DoublePrecision),
        simple(SimpleColumnType::Real),
        NarrowingStrategy::Truncate
    );
    unsupported_combo!(
        truncate_unsupported_for_timestamptz,
        simple(SimpleColumnType::Timestamptz),
        simple(SimpleColumnType::Timestamp),
        NarrowingStrategy::Truncate
    );

    // Delete / set_to_value rely on a violation predicate that does not exist
    // for float-precision loss or timezone reinterpretation (every row is
    // affected, so there's no `WHERE` to write).
    unsupported_combo!(
        delete_unsupported_for_float_size,
        simple(SimpleColumnType::DoublePrecision),
        simple(SimpleColumnType::Real),
        NarrowingStrategy::Delete
    );
    unsupported_combo!(
        delete_unsupported_for_timestamptz,
        simple(SimpleColumnType::Timestamptz),
        simple(SimpleColumnType::Timestamp),
        NarrowingStrategy::Delete
    );
    unsupported_combo!(
        set_to_value_unsupported_for_float_size,
        simple(SimpleColumnType::DoublePrecision),
        simple(SimpleColumnType::Real),
        NarrowingStrategy::SetToValue { value: "0".into() }
    );
    unsupported_combo!(
        set_to_value_unsupported_for_timestamptz,
        simple(SimpleColumnType::Timestamptz),
        simple(SimpleColumnType::Timestamp),
        NarrowingStrategy::SetToValue {
            value: "(now() AT TIME ZONE 'UTC')".into()
        }
    );

    // -----------------------------------------------------------------------
    // No-op paths: when there is no narrowing (widening or identical types)
    // or the baseline column is missing, the helper must return empty.
    // -----------------------------------------------------------------------

    #[test]
    fn widening_returns_empty() {
        let queries = build_narrowing_preprocess(
            DatabaseBackend::Postgres,
            "tbl",
            "col",
            &varchar(80),
            &NarrowingStrategy::Truncate,
            &baseline_with(varchar(30)),
        )
        .expect("widening is not narrowing");
        assert!(queries.is_empty());
    }

    #[test]
    fn missing_baseline_table_returns_empty() {
        let queries = build_narrowing_preprocess(
            DatabaseBackend::Postgres,
            "tbl",
            "col",
            &varchar(30),
            &NarrowingStrategy::Truncate,
            &[],
        )
        .expect("missing baseline must not error");
        assert!(queries.is_empty());
    }

    #[test]
    fn missing_baseline_column_returns_empty() {
        let baseline = baseline_with(varchar(80));
        let queries = build_narrowing_preprocess(
            DatabaseBackend::Postgres,
            "tbl",
            "missing",
            &varchar(30),
            &NarrowingStrategy::Truncate,
            &baseline,
        )
        .expect("missing column must not error");
        assert!(queries.is_empty());
    }

    #[test]
    fn predicate_quotes_column_for_each_backend() {
        for (backend, quoted) in [
            (DatabaseBackend::Postgres, "\"col\""),
            (DatabaseBackend::MySql, "`col`"),
            (DatabaseBackend::Sqlite, "\"col\""),
        ] {
            let predicate = violation_predicate(
                backend,
                "col",
                &NarrowingKind::IntegerSize {
                    from: "bigint",
                    to: "integer",
                },
            )
            .unwrap();
            assert!(
                predicate.contains(quoted),
                "expected {quoted} in {predicate}"
            );
        }
    }
}
