//! Empirically tuned Rayon thresholds.
//!
//! See `docs/PARALLELIZATION.md` for the Wave 6 measurement notes.
//!
//! Loader work is IO-bound; Wave 6 kept the Wave 1 threshold unchanged.

pub(crate) const LOAD_FILES_PAR_THRESHOLD: usize = 20;
pub(crate) const LOAD_FILES_PAR_MIN_LEN: usize = 4;
