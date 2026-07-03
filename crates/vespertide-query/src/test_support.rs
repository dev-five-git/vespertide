//! Shared test-only helpers for the vespertide-query crate.
//!
//! All items are gated under `#[cfg(test)]` via the parent module declaration
//! in `lib.rs` and exposed `pub(crate)` so every inline `mod tests` and
//! `sql/tests/mod.rs` entry can reuse the same implementation.

use vespertide_core::{ColumnDef, ColumnType};

/// Test column helper defaulting to `nullable: true`, matching the existing
/// convention in `crates/vespertide-query/src/sql/tests/mod.rs`.
pub(crate) fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

/// Test column helper with explicit nullability.
pub(crate) fn col_n(name: &str, ty: ColumnType, nullable: bool) -> ColumnDef {
    ColumnDef::new(name, ty, nullable)
}
