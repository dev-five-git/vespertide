//! Empirically tuned Rayon thresholds.
//!
//! Measured during the Wave 6 parallelization pass via the criterion
//! benchmarks in `crates/*/benches/`.

/// `diff_schemas` per-table work breaks even just above 5,000 tables.
/// Use the >5,000 rule's safety threshold so small/medium diffs stay sequential.
/// Override via `VESPERTIDE_DIFF_PAR_THRESHOLD` in tests to exercise the
/// parallel path without constructing production-sized schemas.
pub(crate) fn diff_par_table_threshold() -> usize {
    std::env::var("VESPERTIDE_DIFF_PAR_THRESHOLD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000)
}

pub(crate) const DIFF_PAR_TABLE_MIN_LEN: usize = 16;

/// `validate_schema` did not beat sequential validation through N=1,000.
/// Keep normal schemas on the zero-overhead sequential path.
/// Override via `VESPERTIDE_VALIDATE_SCHEMA_PAR_THRESHOLD` in tests.
pub(crate) fn validate_schema_par_threshold() -> usize {
    std::env::var("VESPERTIDE_VALIDATE_SCHEMA_PAR_THRESHOLD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000)
}

pub(crate) const VALIDATE_SCHEMA_PAR_MIN_LEN: usize = 16;

/// `validate_migration_plan` action checks are too cheap for Rayon below the
/// measured range; keep ordinary revisions sequential.
/// Override via `VESPERTIDE_VALIDATE_PLAN_PAR_THRESHOLD` in tests.
pub(crate) fn validate_plan_par_threshold() -> usize {
    std::env::var("VESPERTIDE_VALIDATE_PLAN_PAR_THRESHOLD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000)
}

pub(crate) const VALIDATE_PLAN_PAR_ACTION_MIN_LEN: usize = 32;
