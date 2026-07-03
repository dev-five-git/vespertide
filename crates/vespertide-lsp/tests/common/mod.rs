//! Shared helpers for the `vespertide-lsp` integration test binaries.
//!
//! Cargo does NOT compile `tests/common/mod.rs` as its own test binary
//! (only top-level `tests/*.rs` files become binaries), so this module
//! is included from each integration test via `mod common;` without
//! creating an extra empty test target.

use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;

/// Construct a `file:///{path}` URI. Centralised here so the nine
/// integration test files do not each carry an identical copy.
pub fn uri(path: &str) -> Uri {
    Uri::from_str(&format!("file:///{path}")).unwrap()
}
