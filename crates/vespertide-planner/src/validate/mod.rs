mod cascade_reach;
mod check_additions;
mod check_between_order;
mod check_default;
mod check_expr_parser;
mod check_self_contradiction;
mod check_strengthening;
mod check_type_mismatch;
mod constraint_drops;
mod constraint_type_changes;
mod dangling_fk_drops;
mod default_changes;
mod enums;
mod fk_addcolumn_nullable;
mod fk_orphan_additions;
mod fk_policy_changes;
mod foreign_keys;
mod pk_additions;
mod plan;
mod schema;
mod sequence_exhaustion;
mod timezone_conversion;
mod type_narrowing;
mod unique_additions;

pub use cascade_reach::{CascadeReachWarning, CascadeRiskLevel, find_cascade_reach_violations};
pub use check_additions::{CheckAdditionWarning, find_check_additions};
pub use check_between_order::find_between_boundary_reversals;
pub use check_expr_parser::{
    CheckExpr as CheckExprAst, CheckToken, CheckTokenKind, Literal as CheckExprLiteral,
    Op as CheckExprOp, lex_check_expr, parse as parse_check_expr,
};
pub use check_self_contradiction::find_self_contradictions;
pub use check_strengthening::{
    CheckStrengtheningKind, CheckStrengtheningWarning, find_check_strengthenings,
};
pub use check_type_mismatch::{CheckTypeMismatchWarning, find_check_type_mismatches};
pub use constraint_drops::{ConstraintDropWarning, find_constraint_drops_without_replacement};
pub use constraint_type_changes::{find_constraint_type_changes, find_primary_key_removals};
pub use dangling_fk_drops::{DanglingFkDrop, find_dangling_fk_drops};
pub use default_changes::{
    DefaultChangeKind, DefaultChangeWarning, RiskLevel, find_default_changes,
};
pub use fk_addcolumn_nullable::find_addcolumn_fk_nullable_violations;
pub use fk_orphan_additions::{FkOrphanAdditionWarning, find_fk_orphan_additions};
pub use fk_policy_changes::{
    FkPolicyChangeWarning, PolicyDelta, find_fk_policy_changes, render_reference_action,
};
pub use foreign_keys::{MissingFkSupportingIndex, find_missing_fk_supporting_indexes};
pub use pk_additions::{PkAdditionKind, PrimaryKeyAdditionWarning, find_primary_key_additions};
pub use plan::{
    EnumFillWithRequired, FillWithRequired, find_missing_enum_fill_with, find_missing_fill_with,
    find_plan_violations, validate_migration_plan,
};
pub use schema::{find_schema_violations, validate_schema};
pub use sequence_exhaustion::{
    SequenceExhaustionKind, SequenceExhaustionWarning, SequenceRiskLevel,
    find_sequence_exhaustion_risks,
};
pub use timezone_conversion::{
    TimezoneConversionDirection, TimezoneConversionWarning, find_timezone_conversions,
};
pub use type_narrowing::{NarrowingKind, TypeNarrowingWarning, find_type_narrowings, is_narrowing};
pub use unique_additions::{
    FkReference as UniqueAdditionFkReference, PkKind, UniqueAdditionWarning, find_unique_additions,
};

#[cfg(test)]
mod tests;
