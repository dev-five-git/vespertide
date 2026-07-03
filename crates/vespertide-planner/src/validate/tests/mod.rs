pub(super) use super::*;
pub(super) use crate::error::PlannerError;
pub(super) use crate::test_support::{col, col_nullable, idx, pk, table};
pub(super) use crate::validate::schema::validate_table;
pub(super) use rstest::rstest;
pub(super) use vespertide_core::schema::primary_key::{PrimaryKeyDef, PrimaryKeySyntax};
pub(super) use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, DefaultValue, EnumValues, MigrationAction,
    MigrationPlan, NumValue, SimpleColumnType, TableConstraint, TableDef,
};

fn is_duplicate(err: &PlannerError) -> bool {
    matches!(err, PlannerError::DuplicateTableName(_))
}

fn is_fk_table(err: &PlannerError) -> bool {
    matches!(err, PlannerError::ForeignKeyTableNotFound(_, _, _))
}

fn is_fk_column(err: &PlannerError) -> bool {
    matches!(err, PlannerError::ForeignKeyColumnNotFound(_, _, _, _))
}

fn is_index_column(err: &PlannerError) -> bool {
    matches!(err, PlannerError::IndexColumnNotFound(_, _, _))
}

fn is_constraint_column(err: &PlannerError) -> bool {
    matches!(err, PlannerError::ConstraintColumnNotFound(_, _, _))
}

fn is_empty_columns(err: &PlannerError) -> bool {
    matches!(err, PlannerError::EmptyConstraintColumns(_, _))
}

fn is_missing_pk(err: &PlannerError) -> bool {
    matches!(err, PlannerError::MissingPrimaryKey(_))
}

mod check_default;
mod constraint_drops;
mod dangling_fk_drops;
mod enum_fill_with;
mod fill_with;
mod fk_policy_changes;
mod fk_supporting_index;
mod plan_validation;
mod schema_cases;
mod timezone_conversion;
mod type_narrowing;
