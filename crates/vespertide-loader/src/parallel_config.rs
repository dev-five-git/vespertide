//! Empirically tuned Rayon thresholds.
//!
//! Measured during the Wave 6 parallelization pass via the criterion
//! benchmarks in `crates/*/benches/`.
//!
//! Loader work is IO-bound; Wave 6 kept the Wave 1 threshold unchanged.

use std::path::{Path, PathBuf};

use rayon::prelude::*;

pub(crate) const LOAD_FILES_PAR_THRESHOLD: usize = 20;
pub(crate) const LOAD_FILES_PAR_MIN_LEN: usize = 4;

/// Map `f` over `paths`, staying sequential below the tuned threshold and
/// dispatching to Rayon (with the tuned chunk floor) above it.
///
/// Single home for the sequential/parallel dispatch shape shared by every
/// file loader in this crate, so threshold tuning happens in one place.
pub(crate) fn map_paths_with_threshold<T, E, F>(paths: &[PathBuf], f: F) -> Vec<Result<T, E>>
where
    F: Fn(&Path) -> Result<T, E> + Sync,
    T: Send,
    E: Send,
{
    if paths.len() < LOAD_FILES_PAR_THRESHOLD {
        paths.iter().map(|path| f(path)).collect()
    } else {
        paths
            .par_iter()
            .with_min_len(LOAD_FILES_PAR_MIN_LEN)
            .map(|path| f(path))
            .collect()
    }
}
