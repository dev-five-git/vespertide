use super::*;
use crate::validate::{NarrowingKind, find_type_narrowings, is_narrowing};
use vespertide_core::ComplexColumnType;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn baseline_col(name: &str, ty: ColumnType) -> ColumnDef {
    let mut c = col(name, ty);
    c.nullable = false;
    c
}

fn baseline_table(table_name: &str, column: ColumnDef) -> TableDef {
    table(
        table_name,
        vec![column],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )
}

fn modify_type(table: &str, column: &str, new_type: ColumnType) -> MigrationAction {
    MigrationAction::ModifyColumnType {
        table: table.into(),
        column: column.into(),
        new_type,
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }
}

fn plan_with(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    }
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

// ---------------------------------------------------------------------------
// VARCHAR / CHAR length narrowing — the headline F6 case
// ---------------------------------------------------------------------------

#[test]
fn varchar_length_shrink_is_detected() {
    let baseline = vec![baseline_table("users", baseline_col("email", varchar(40)))];
    let plan = plan_with(vec![modify_type("users", "email", varchar(30))]);

    let warnings = find_type_narrowings(&plan, &baseline);

    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];
    assert_eq!(w.action_index, 0);
    assert_eq!(w.table, "users");
    assert_eq!(w.column, "email");
    assert_eq!(w.from_display, "varchar(40)");
    assert_eq!(w.to_display, "varchar(30)");
    assert_eq!(w.kind, NarrowingKind::VarcharLength { from: 40, to: 30 });
}

// Equal-length is NOT narrowing: pins the `b < a` guard (mutated to `true`)
// and the `<` operator (mutated to `<=`) on the Varchar and Char arms.
#[test]
fn varchar_equal_length_is_not_narrowing() {
    assert_eq!(is_narrowing(&varchar(30), &varchar(30)), None);
}

#[test]
fn char_equal_length_is_not_narrowing() {
    assert_eq!(is_narrowing(&char_t(10), &char_t(10)), None);
}

#[test]
fn char_length_shrink_is_narrowing() {
    assert_eq!(
        is_narrowing(&char_t(10), &char_t(4)),
        Some(NarrowingKind::CharLength { from: 10, to: 4 })
    );
}

// Varchar -> Char: equal length is NOT narrowing (pins the `b < a` guard
// and `<` operator on the VarcharToCharShorter arm); shorter target is.
#[test]
fn varchar_to_char_equal_length_is_not_narrowing() {
    assert_eq!(is_narrowing(&varchar(5), &char_t(5)), None);
}

#[test]
fn varchar_to_shorter_char_is_narrowing() {
    assert_eq!(
        is_narrowing(&varchar(5), &char_t(3)),
        Some(NarrowingKind::VarcharToCharShorter { from: 5, to: 3 })
    );
}

// Exact-string assertions pin each per-backend impact description so the
// whole-function "xyzzy" mutants on postgres/mysql/sqlite_impact die.
#[test]
fn impact_descriptions_are_exact_per_backend() {
    let kind = NarrowingKind::FloatSize {
        from: "double precision",
        to: "real",
    };
    assert_eq!(
        kind.postgres_impact(),
        "silently loses precision (downcast)"
    );
    assert_eq!(kind.mysql_impact(), "silently loses precision (downcast)");
    assert_eq!(kind.sqlite_impact(), "REAL affinity — no size enforcement");

    let len = NarrowingKind::VarcharLength { from: 40, to: 30 };
    assert_eq!(
        len.postgres_impact(),
        "rejects ALTER with `value too long` if any row violates"
    );
    assert_eq!(
        len.mysql_impact(),
        "SILENTLY truncates values past the new length (warning only)"
    );
    assert_eq!(len.sqlite_impact(), "length advisory only — no enforcement");
}

#[test]
fn varchar_length_grow_is_not_detected() {
    let baseline = vec![baseline_table("users", baseline_col("email", varchar(30)))];
    let plan = plan_with(vec![modify_type("users", "email", varchar(40))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert!(warnings.is_empty(), "widening is safe, must not warn");
}

#[test]
fn varchar_length_unchanged_is_not_detected() {
    let baseline = vec![baseline_table("users", baseline_col("email", varchar(40)))];
    let plan = plan_with(vec![modify_type("users", "email", varchar(40))]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

#[test]
fn char_length_shrink_is_detected() {
    let baseline = vec![baseline_table(
        "users",
        baseline_col("country_code", char_t(3)),
    )];
    let plan = plan_with(vec![modify_type("users", "country_code", char_t(2))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::CharLength { from: 3, to: 2 }
    );
}

#[test]
fn varchar_to_char_shorter_is_detected() {
    let baseline = vec![baseline_table("users", baseline_col("code", varchar(10)))];
    let plan = plan_with(vec![modify_type("users", "code", char_t(5))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::VarcharToCharShorter { from: 10, to: 5 }
    );
}

#[test]
fn char_to_varchar_shorter_is_detected() {
    let baseline = vec![baseline_table("users", baseline_col("code", char_t(10)))];
    let plan = plan_with(vec![modify_type("users", "code", varchar(5))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::CharToVarcharShorter { from: 10, to: 5 }
    );
}

#[test]
fn char_to_varchar_same_or_longer_is_not_detected() {
    let baseline = vec![baseline_table("users", baseline_col("code", char_t(5)))];
    let plan = plan_with(vec![modify_type("users", "code", varchar(5))]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());

    let plan2 = plan_with(vec![modify_type("users", "code", varchar(10))]);
    assert!(find_type_narrowings(&plan2, &baseline).is_empty());
}

// ---------------------------------------------------------------------------
// TEXT -> bounded length (always a potential truncation)
// ---------------------------------------------------------------------------

#[test]
fn text_to_varchar_is_always_detected() {
    let baseline = vec![baseline_table(
        "articles",
        baseline_col("body", simple(SimpleColumnType::Text)),
    )];
    let plan = plan_with(vec![modify_type("articles", "body", varchar(255))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::TextToVarchar { to_length: 255 }
    );
}

#[test]
fn text_to_char_is_always_detected() {
    let baseline = vec![baseline_table(
        "articles",
        baseline_col("body", simple(SimpleColumnType::Text)),
    )];
    let plan = plan_with(vec![modify_type("articles", "body", char_t(255))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::TextToChar { to_length: 255 }
    );
}

#[test]
fn varchar_to_text_is_widening_and_not_detected() {
    let baseline = vec![baseline_table(
        "articles",
        baseline_col("body", varchar(255)),
    )];
    let plan = plan_with(vec![modify_type(
        "articles",
        "body",
        simple(SimpleColumnType::Text),
    )]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

// ---------------------------------------------------------------------------
// NUMERIC precision/scale
// ---------------------------------------------------------------------------

#[test]
fn numeric_scale_shrink_is_detected() {
    let baseline = vec![baseline_table(
        "accounts",
        baseline_col("balance", numeric(10, 4)),
    )];
    let plan = plan_with(vec![modify_type("accounts", "balance", numeric(10, 2))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::NumericScale {
            from_scale: 4,
            to_scale: 2
        }
    );
}

#[test]
fn numeric_integer_digits_shrink_is_detected() {
    // (12, 4) -> integer-part = 8 digits.
    // ( 8, 4) -> integer-part = 4 digits. Loss.
    let baseline = vec![baseline_table(
        "accounts",
        baseline_col("balance", numeric(12, 4)),
    )];
    let plan = plan_with(vec![modify_type("accounts", "balance", numeric(8, 4))]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::NumericIntegerDigits {
            from_int_digits: 8,
            to_int_digits: 4
        }
    );
}

#[test]
fn numeric_widening_is_not_detected() {
    let baseline = vec![baseline_table(
        "accounts",
        baseline_col("balance", numeric(10, 2)),
    )];
    let plan = plan_with(vec![modify_type("accounts", "balance", numeric(12, 4))]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

#[test]
fn numeric_unchanged_is_not_detected() {
    let baseline = vec![baseline_table(
        "accounts",
        baseline_col("balance", numeric(10, 2)),
    )];
    let plan = plan_with(vec![modify_type("accounts", "balance", numeric(10, 2))]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

// ---------------------------------------------------------------------------
// Integer size narrowing
// ---------------------------------------------------------------------------

#[test]
fn bigint_to_integer_is_detected() {
    let baseline = vec![baseline_table(
        "events",
        baseline_col("offset_id", simple(SimpleColumnType::BigInt)),
    )];
    let plan = plan_with(vec![modify_type(
        "events",
        "offset_id",
        simple(SimpleColumnType::Integer),
    )]);

    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::IntegerSize {
            from: "bigint",
            to: "integer"
        }
    );
}

#[test]
fn integer_to_smallint_is_detected() {
    let baseline = vec![baseline_table(
        "events",
        baseline_col("level", simple(SimpleColumnType::Integer)),
    )];
    let plan = plan_with(vec![modify_type(
        "events",
        "level",
        simple(SimpleColumnType::SmallInt),
    )]);
    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::IntegerSize {
            from: "integer",
            to: "smallint"
        }
    );
}

#[test]
fn bigint_to_smallint_is_detected() {
    let baseline = vec![baseline_table(
        "events",
        baseline_col("offset_id", simple(SimpleColumnType::BigInt)),
    )];
    let plan = plan_with(vec![modify_type(
        "events",
        "offset_id",
        simple(SimpleColumnType::SmallInt),
    )]);
    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::IntegerSize {
            from: "bigint",
            to: "smallint"
        }
    );
}

#[test]
fn integer_to_bigint_is_widening_and_not_detected() {
    let baseline = vec![baseline_table(
        "events",
        baseline_col("offset_id", simple(SimpleColumnType::Integer)),
    )];
    let plan = plan_with(vec![modify_type(
        "events",
        "offset_id",
        simple(SimpleColumnType::BigInt),
    )]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

// ---------------------------------------------------------------------------
// Float size + Timezone loss
// ---------------------------------------------------------------------------

#[test]
fn double_precision_to_real_is_detected() {
    let baseline = vec![baseline_table(
        "metrics",
        baseline_col("ratio", simple(SimpleColumnType::DoublePrecision)),
    )];
    let plan = plan_with(vec![modify_type(
        "metrics",
        "ratio",
        simple(SimpleColumnType::Real),
    )]);
    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(
        warnings[0].kind,
        NarrowingKind::FloatSize {
            from: "double precision",
            to: "real"
        }
    );
}

#[test]
fn timestamptz_to_timestamp_is_detected() {
    let baseline = vec![baseline_table(
        "events",
        baseline_col("at", simple(SimpleColumnType::Timestamptz)),
    )];
    let plan = plan_with(vec![modify_type(
        "events",
        "at",
        simple(SimpleColumnType::Timestamp),
    )]);
    let warnings = find_type_narrowings(&plan, &baseline);
    assert_eq!(warnings[0].kind, NarrowingKind::TimestamptzToTimestamp);
}

// ---------------------------------------------------------------------------
// Action filtering + edge cases
// ---------------------------------------------------------------------------

#[test]
fn non_modify_column_type_actions_are_ignored() {
    let baseline = vec![baseline_table("users", baseline_col("email", varchar(40)))];
    let plan = plan_with(vec![
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "alt_email".into(),
                r#type: varchar(10),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        },
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
    ]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

#[test]
fn missing_baseline_table_yields_no_warning() {
    // Action references a table not present in the baseline (e.g. CreateTable +
    // immediate ModifyColumnType in the same plan). Detector cannot compare,
    // so it must skip silently rather than panic.
    let plan = plan_with(vec![modify_type("orphan", "col", varchar(10))]);
    assert!(find_type_narrowings(&plan, &[]).is_empty());
}

#[test]
fn missing_baseline_column_yields_no_warning() {
    let baseline = vec![baseline_table("users", baseline_col("id", varchar(40)))];
    let plan = plan_with(vec![modify_type("users", "ghost", varchar(10))]);
    assert!(find_type_narrowings(&plan, &baseline).is_empty());
}

#[test]
fn mixed_plan_aggregates_only_narrowings_with_correct_indices() {
    let baseline = vec![
        baseline_table("users", baseline_col("email", varchar(40))),
        baseline_table("accounts", baseline_col("balance", numeric(10, 4))),
    ];
    let plan = plan_with(vec![
        // 0  WIDEN — not warned
        modify_type("users", "email", varchar(80)),
        // 1  NARROW VARCHAR — warned
        modify_type("users", "email", varchar(30)),
        // 2  Unrelated action — not warned
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        // 3  NARROW NUMERIC scale — warned
        modify_type("accounts", "balance", numeric(10, 2)),
    ]);
    let warnings = find_type_narrowings(&plan, &baseline);

    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].action_index, 1);
    assert_eq!(warnings[0].column, "email");
    assert_eq!(warnings[1].action_index, 3);
    assert_eq!(warnings[1].column, "balance");
}

#[test]
fn empty_plan_returns_empty_warnings() {
    assert!(find_type_narrowings(&plan_with(vec![]), &[]).is_empty());
}

// ---------------------------------------------------------------------------
// Direct is_narrowing matrix coverage
// ---------------------------------------------------------------------------

#[test]
fn is_narrowing_widening_returns_none() {
    assert!(is_narrowing(&varchar(30), &varchar(40)).is_none());
    assert!(is_narrowing(&numeric(8, 2), &numeric(12, 4)).is_none());
    assert!(
        is_narrowing(
            &simple(SimpleColumnType::Integer),
            &simple(SimpleColumnType::BigInt)
        )
        .is_none()
    );
}

#[test]
fn is_narrowing_unrelated_swap_returns_none() {
    // text -> integer is not classified as narrowing here (Phase 1 scope).
    // Future phases may add explicit cross-category warnings.
    assert!(
        is_narrowing(
            &simple(SimpleColumnType::Text),
            &simple(SimpleColumnType::Integer),
        )
        .is_none()
    );
}

#[test]
fn narrowing_kind_impacts_are_non_empty_for_every_variant() {
    let kinds = [
        NarrowingKind::VarcharLength { from: 40, to: 30 },
        NarrowingKind::CharLength { from: 3, to: 2 },
        NarrowingKind::VarcharToCharShorter { from: 10, to: 5 },
        NarrowingKind::CharToVarcharShorter { from: 10, to: 5 },
        NarrowingKind::NumericScale {
            from_scale: 4,
            to_scale: 2,
        },
        NarrowingKind::NumericIntegerDigits {
            from_int_digits: 8,
            to_int_digits: 4,
        },
        NarrowingKind::IntegerSize {
            from: "bigint",
            to: "integer",
        },
        NarrowingKind::FloatSize {
            from: "double precision",
            to: "real",
        },
        NarrowingKind::TextToVarchar { to_length: 255 },
        NarrowingKind::TextToChar { to_length: 255 },
        NarrowingKind::TimestamptzToTimestamp,
    ];
    for k in &kinds {
        assert!(
            !k.postgres_impact().is_empty(),
            "postgres impact empty for {k:?}"
        );
        assert!(!k.mysql_impact().is_empty(), "mysql impact empty for {k:?}");
        assert!(
            !k.sqlite_impact().is_empty(),
            "sqlite impact empty for {k:?}"
        );
    }
}
