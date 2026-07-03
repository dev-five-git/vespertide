use super::*;
use crate::validate::{
    TimezoneConversionDirection, TimezoneConversionWarning, find_timezone_conversions,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn baseline_with(old: ColumnType) -> Vec<TableDef> {
    vec![table(
        "events",
        vec![{
            let mut c = col("at", old);
            c.nullable = false;
            c
        }],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["at".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )]
}

fn modify_at_type(new_type: ColumnType, timezone: Option<&str>) -> MigrationAction {
    MigrationAction::ModifyColumnType {
        table: "events".into(),
        column: "at".into(),
        new_type,
        fill_with: None,
        narrowing_strategy: None,
        timezone: timezone.map(ToString::to_string),
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

fn ts() -> ColumnType {
    ColumnType::Simple(SimpleColumnType::Timestamp)
}
fn tstz() -> ColumnType {
    ColumnType::Simple(SimpleColumnType::Timestamptz)
}

// ---------------------------------------------------------------------------
// Detected: both directions
// ---------------------------------------------------------------------------

#[test]
fn timestamp_to_timestamptz_is_detected_as_naive_to_aware() {
    let baseline = baseline_with(ts());
    let plan = plan_with(vec![modify_at_type(tstz(), None)]);

    let warnings = find_timezone_conversions(&plan, &baseline);

    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];
    assert_eq!(w.action_index, 0);
    assert_eq!(w.table, "events");
    assert_eq!(w.column, "at");
    assert_eq!(w.direction, TimezoneConversionDirection::NaiveToAware);
    assert!(w.current_timezone.is_none());
    assert_eq!(w.direction.label(), "timestamp -> timestamptz");
}

#[test]
fn timestamptz_to_timestamp_is_detected_as_aware_to_naive() {
    let baseline = baseline_with(tstz());
    let plan = plan_with(vec![modify_at_type(ts(), None)]);

    let warnings = find_timezone_conversions(&plan, &baseline);

    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].direction,
        TimezoneConversionDirection::AwareToNaive
    );
    assert_eq!(warnings[0].direction.label(), "timestamptz -> timestamp");
}

#[test]
fn current_timezone_is_carried_through_when_action_already_has_one() {
    let baseline = baseline_with(ts());
    let plan = plan_with(vec![modify_at_type(tstz(), Some("Asia/Seoul"))]);

    let warnings = find_timezone_conversions(&plan, &baseline);

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].current_timezone.as_deref(), Some("Asia/Seoul"));
}

// ---------------------------------------------------------------------------
// Not detected: same type / unrelated swap / non-modify action
// ---------------------------------------------------------------------------

#[test]
fn timestamp_to_timestamp_is_not_detected() {
    let baseline = baseline_with(ts());
    let plan = plan_with(vec![modify_at_type(ts(), None)]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

#[test]
fn timestamptz_to_timestamptz_is_not_detected() {
    let baseline = baseline_with(tstz());
    let plan = plan_with(vec![modify_at_type(tstz(), None)]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

#[test]
fn unrelated_type_swap_is_not_detected() {
    // Date <-> Timestamp is a *time* swap but does not involve timezone semantics.
    let baseline = baseline_with(ColumnType::Simple(SimpleColumnType::Date));
    let plan = plan_with(vec![modify_at_type(ts(), None)]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

#[test]
fn varchar_narrowing_is_not_detected() {
    // F6 territory; the detector must not poach.
    let baseline = baseline_with(ColumnType::Complex(
        vespertide_core::ComplexColumnType::Varchar { length: 40 },
    ));
    let plan = plan_with(vec![modify_at_type(
        ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar { length: 30 }),
        None,
    )]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

#[test]
fn non_modify_column_type_actions_are_ignored() {
    let baseline = baseline_with(ts());
    let plan = plan_with(vec![MigrationAction::DeleteColumn {
        table: "events".into(),
        column: "at".into(),
    }]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

#[test]
fn missing_baseline_column_yields_no_warning() {
    let baseline = vec![table(
        "events",
        vec![{
            let mut c = col("id", ColumnType::Simple(SimpleColumnType::Integer));
            c.nullable = false;
            c
        }],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )];
    let plan = plan_with(vec![modify_at_type(tstz(), None)]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

#[test]
fn mixed_plan_returns_only_timezone_conversions_with_indices() {
    let baseline = vec![
        table(
            "events",
            vec![{
                let mut c = col("at", ts());
                c.nullable = false;
                c
            }],
            vec![],
        ),
        table(
            "audits",
            vec![{
                let mut c = col("ts", tstz());
                c.nullable = false;
                c
            }],
            vec![],
        ),
    ];
    let plan = plan_with(vec![
        // 0  unrelated DeleteColumn
        MigrationAction::DeleteColumn {
            table: "events".into(),
            column: "at".into(),
        },
        // 1  timestamp -> timestamptz on events.at — WARN
        MigrationAction::ModifyColumnType {
            table: "events".into(),
            column: "at".into(),
            new_type: tstz(),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        },
        // 2  timestamptz -> timestamp on audits.ts — WARN
        MigrationAction::ModifyColumnType {
            table: "audits".into(),
            column: "ts".into(),
            new_type: ts(),
            fill_with: None,
            narrowing_strategy: None,
            timezone: Some("UTC".into()),
        },
    ]);

    let warnings = find_timezone_conversions(&plan, &baseline);

    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].action_index, 1);
    assert_eq!(
        warnings[0].direction,
        TimezoneConversionDirection::NaiveToAware
    );
    assert!(warnings[0].current_timezone.is_none());
    assert_eq!(warnings[1].action_index, 2);
    assert_eq!(
        warnings[1].direction,
        TimezoneConversionDirection::AwareToNaive
    );
    assert_eq!(warnings[1].current_timezone.as_deref(), Some("UTC"));
}

#[test]
fn empty_plan_returns_empty_warnings() {
    let baseline = baseline_with(ts());
    let plan = plan_with(vec![]);
    assert!(find_timezone_conversions(&plan, &baseline).is_empty());
}

#[test]
fn warning_struct_round_trip_equality() {
    // The struct is used by CLI code that builds it manually for tests;
    // confirm `PartialEq` is value-based so unit tests can compare cleanly.
    let a = TimezoneConversionWarning {
        action_index: 0,
        table: "t".into(),
        column: "c".into(),
        direction: TimezoneConversionDirection::NaiveToAware,
        current_timezone: None,
    };
    let b = a.clone();
    assert_eq!(a, b);
}
