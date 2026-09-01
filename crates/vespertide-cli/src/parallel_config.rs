//! Empirically tuned Rayon thresholds.
//!
//! Measured during the Wave 6 parallelization pass via the criterion
//! benchmarks in `crates/*/benches/`.
//!
//! CLI export iterates at most four ORM variants, but per-table render work is
//! CPU-bound. Wave 6 kept the Wave 1 threshold unchanged.

pub(crate) const EXPORT_RENDER_PAR_THRESHOLD: usize = 50;
pub(crate) const EXPORT_RENDER_PAR_MIN_LEN: usize = 32;
