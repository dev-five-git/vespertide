//! Shared test-only helpers for the vespertide-cli crate.
//!
//! All items are gated under `#[cfg(test)]` via the parent module declaration
//! in `main.rs` and exposed `pub(crate)` so every inline `mod tests` and
//! `commands/<cmd>/tests/mod.rs` entry can reuse the same implementation.

use std::path::{Path, PathBuf};

/// RAII guard that swaps the process current directory for the duration of a
/// test and restores it on drop. Used in combination with `serial_test::serial`
/// for filesystem-isolated CLI integration tests.
pub(crate) struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    pub(crate) fn new(dir: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}
