//! Empirically tuned Rayon thresholds.
//!
//! Measured during the Wave 6 parallelization pass via the criterion
//! benchmarks in `crates/*/benches/`.

/// `build_plan_queries` pays an up-front schema-preparation cost before the
/// per-action Rayon phase. It did not win through 1,000 actions, so ordinary
/// plans stay sequential and only very large plans attempt the parallel path.
/// Override via `VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD` in tests.
pub(crate) fn plan_query_par_action_threshold() -> usize {
    std::env::var("VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000)
}

pub(crate) const PLAN_QUERY_PAR_ACTION_MIN_LEN: usize = 8;
