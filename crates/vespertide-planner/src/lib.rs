//! Schema diffing and migration planning.
//!
//! - [`diff_schemas`]: compute typed `MigrationAction`s between two schemas
//! - [`apply_action`]: replay an action onto a baseline schema
//! - [`validate_schema`], [`validate_migration_plan`]: ensure invariants

pub mod apply;
pub mod diff;
pub mod drop_resolution;
pub mod error;
mod parallel_config;
pub mod plan;
pub mod schema;
#[cfg(test)]
mod test_support;
pub mod validate;

pub use apply::apply_action;
pub use diff::diff_schemas;
pub use drop_resolution::{
    DropChoice, DropResolution, DropTarget, Match, RenameCandidate, apply_drop_resolution,
    find_drop_resolutions,
};
pub use error::{MultipleErrors, PlannerError};
pub use plan::{plan_next_migration, plan_next_migration_with_baseline};
pub use schema::schema_from_plans;
pub use validate::{
    CascadeReachWarning, CascadeRiskLevel, CheckAdditionWarning, CheckExprAst, CheckExprLiteral,
    CheckExprOp, CheckStrengtheningKind, CheckStrengtheningWarning, CheckToken, CheckTokenKind,
    CheckTypeMismatchWarning, ConstraintDropWarning, DanglingFkDrop, DefaultChangeKind,
    DefaultChangeWarning, EnumFillWithRequired, FillWithRequired, FkOrphanAdditionWarning,
    FkPolicyChangeWarning, MissingFkSupportingIndex, NarrowingKind, PkAdditionKind, PkKind,
    PolicyDelta, PrimaryKeyAdditionWarning, RawSqlReplayHazard, RiskLevel, SequenceExhaustionKind,
    SequenceExhaustionWarning, SequenceRiskLevel, TimezoneConversionDirection,
    TimezoneConversionWarning, TypeNarrowingWarning, UniqueAdditionFkReference,
    UniqueAdditionWarning, find_addcolumn_fk_nullable_violations, find_between_boundary_reversals,
    find_cascade_reach_violations, find_check_additions, find_check_strengthenings,
    find_check_type_mismatches, find_constraint_drops_without_replacement,
    find_constraint_type_changes, find_dangling_fk_drops, find_default_changes,
    find_fk_orphan_additions, find_fk_policy_changes, find_missing_enum_fill_with,
    find_missing_fill_with, find_missing_fk_supporting_indexes, find_plan_violations,
    find_primary_key_additions, find_primary_key_removals, find_raw_sql_replay_hazards,
    find_schema_violations, find_self_contradictions, find_sequence_exhaustion_risks,
    find_timezone_conversions, find_type_narrowings, find_unique_additions, is_narrowing,
    lex_check_expr, parse_check_expr, render_reference_action, validate_migration_plan,
    validate_schema,
};
