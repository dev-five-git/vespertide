//! Detect `ModifyColumnType` actions that flip a column between `timestamp`
//! and `timestamptz`.
//!
//! This is fault **F20** in the data-dependent migration fault taxonomy:
//! converting between naive (`timestamp`) and timezone-aware (`timestamptz`)
//! representations is *semantically unsafe* unless the user explicitly
//! states which timezone the naive values are in (or should become).
//! Without that input the migration silently shifts every row by some
//! offset — on `PostgreSQL` the server's `timezone` GUC, on `MySQL` the
//! session's `time_zone`, on `SQLite` undefined.
//!
//! Vespertide closes this surface by:
//!   1. Detecting the conversion here (Phase 1).
//!   2. Requiring the user to pick a timezone in `vespertide revision`
//!      (Phase 2).
//!   3. Emitting `... USING col AT TIME ZONE '<tz>'` on `PostgreSQL` so the
//!      conversion is explicit and reproducible (Phase 3).
//!
//! `MySQL` and `SQLite` are unaffected because vespertide maps both
//! `timestamp` and `timestamptz` to the same underlying SQL type on those
//! backends — the `ALTER COLUMN TYPE` becomes a no-op and the timezone
//! choice is recorded in the migration JSON for portability but does not
//! influence the SQL emitted for those backends.

use vespertide_core::{ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableDef};

/// Direction of a `timestamp` ⇄ `timestamptz` conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimezoneConversionDirection {
    /// `timestamp` → `timestamptz`: existing naive values need to be
    /// *interpreted* as belonging to some timezone before being stored as
    /// a timezone-aware instant.
    NaiveToAware,
    /// `timestamptz` → `timestamp`: existing timezone-aware values need to
    /// be *projected* into some timezone, after which the timezone tag is
    /// dropped. The remaining wall-clock time is what gets stored.
    AwareToNaive,
}

impl TimezoneConversionDirection {
    /// Short label used by CLI prompts and snapshot tests.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            TimezoneConversionDirection::NaiveToAware => "timestamp -> timestamptz",
            TimezoneConversionDirection::AwareToNaive => "timestamptz -> timestamp",
        }
    }
}

/// A single `ModifyColumnType` action that converts a column between
/// `timestamp` and `timestamptz` without a recorded timezone choice.
///
/// Returned by [`find_timezone_conversions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimezoneConversionWarning {
    /// Index of the offending action in the migration plan.
    pub action_index: usize,
    /// Table that owns the column.
    pub table: String,
    /// Column being converted.
    pub column: String,
    /// Which direction the conversion runs.
    pub direction: TimezoneConversionDirection,
    /// Timezone already recorded on the action, if any. `None` means the
    /// user has not yet supplied one — the revision prompt will demand it.
    /// `Some(tz)` is surfaced here mostly so the diff command can show the
    /// user what they picked previously.
    pub current_timezone: Option<String>,
}

/// Scan a migration plan for `ModifyColumnType` actions that swap
/// `timestamp` and `timestamptz` (in either direction). Only emits a
/// warning when the action does not yet carry a `timezone` choice —
/// once the revision prompt records one, the same plan returns an empty
/// vector so re-running `vespertide diff` does not nag the user.
///
/// Static: this performs no data access; it only compares the
/// `MigrationAction`'s `new_type` against the baseline column type.
#[must_use]
pub fn find_timezone_conversions(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<TimezoneConversionWarning> {
    plan.actions
        .iter()
        .enumerate()
        .filter_map(|(idx, action)| warning_for_action(idx, action, baseline))
        .collect()
}

fn warning_for_action(
    idx: usize,
    action: &MigrationAction,
    baseline: &[TableDef],
) -> Option<TimezoneConversionWarning> {
    let MigrationAction::ModifyColumnType {
        table,
        column,
        new_type,
        timezone,
        ..
    } = action
    else {
        return None;
    };
    let old_type = baseline
        .iter()
        .find(|t| t.name == *table)?
        .columns
        .iter()
        .find(|c| c.name == *column)?
        .r#type
        .clone();
    let direction = classify_direction(&old_type, new_type)?;
    Some(TimezoneConversionWarning {
        action_index: idx,
        table: table.to_string(),
        column: column.to_string(),
        direction,
        current_timezone: timezone.clone(),
    })
}

fn classify_direction(from: &ColumnType, to: &ColumnType) -> Option<TimezoneConversionDirection> {
    match (from, to) {
        (
            ColumnType::Simple(SimpleColumnType::Timestamp),
            ColumnType::Simple(SimpleColumnType::Timestamptz),
        ) => Some(TimezoneConversionDirection::NaiveToAware),
        (
            ColumnType::Simple(SimpleColumnType::Timestamptz),
            ColumnType::Simple(SimpleColumnType::Timestamp),
        ) => Some(TimezoneConversionDirection::AwareToNaive),
        _ => None,
    }
}
