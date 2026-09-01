//! Shared `#[cfg(test)]` helpers for the `vespertide-loader` crate.
//!
//! Hosts the RAII `CwdGuard` (used to enter a `tempfile::TempDir` for the
//! duration of a `#[serial]` test that exercises code reading
//! `vespertide.json` from the current directory) and a `write_default_config`
//! helper that serialises `VespertideConfig::default()` to a target path.

use std::fs;
use std::path::{Path, PathBuf};

use vespertide_config::VespertideConfig;

/// RAII guard that switches `std::env::current_dir()` into `dir` on
/// construction and restores the original directory on drop. Use with
/// `#[serial]` since `current_dir` is process-global state.
pub(crate) struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    pub(crate) fn new(dir: impl AsRef<Path>) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.as_ref()).unwrap();
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

/// Write a pretty-printed `VespertideConfig::default()` JSON to `at`.
pub(crate) fn write_default_config(at: impl AsRef<Path>) {
    let cfg = VespertideConfig::default();
    let text = serde_json::to_string_pretty(&cfg).unwrap();
    fs::write(at.as_ref(), text).unwrap();
}
