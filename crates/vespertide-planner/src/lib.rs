use thiserror::Error;
use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("not implemented")]
    NotImplemented,
}

/// Build the next migration plan given the current schema and existing plans.
pub fn plan_next_migration(
    _current: &[TableDef],
    _applied_plans: &[MigrationPlan],
) -> Result<MigrationPlan, PlannerError> {
    Err(PlannerError::NotImplemented)
}

/// Derive a schema snapshot from existing migration plans.
pub fn schema_from_plans(_plans: &[MigrationPlan]) -> Result<Vec<TableDef>, PlannerError> {
    Err(PlannerError::NotImplemented)
}

/// Diff two schema snapshots into a migration plan.
pub fn diff_schemas(_from: &[TableDef], _to: &[TableDef]) -> Result<MigrationPlan, PlannerError> {
    Err(PlannerError::NotImplemented)
}

/// Apply a single migration action to an in-memory schema snapshot.
pub fn apply_action(
    _schema: &mut Vec<TableDef>,
    _action: &MigrationAction,
) -> Result<(), PlannerError> {
    Err(PlannerError::NotImplemented)
}
