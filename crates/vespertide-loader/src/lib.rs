//! Filesystem loading of vespertide models and migrations.
//!
//! Supports both JSON and YAML model/migration files. Used by both the CLI
//! at runtime and the `vespertide_migration!` macro at compile time.

// Test-only env mutation in `migrations.rs` / `models.rs` uses
// `std::env::set_var` / `remove_var`, which became `unsafe` in Rust 2024.
// All sites are serialized via `serial_test::serial`, so expect unsafe in tests only.
#![cfg_attr(
    test,
    expect(
        unsafe_code,
        reason = "serial_test serializes Rust 2024 std::env var setters used by loader tests"
    )
)]

pub mod config;
pub mod migrations;
pub mod models;
mod parallel_config;
#[cfg(test)]
mod test_support;

pub use config::{load_config, load_config_from_path, load_config_or_default};

/// True when the path has a supported model/migration file extension
/// (`.json`, `.yaml`, `.yml`). Single source of truth for the loader's
/// supported-extension set.
pub(crate) fn has_supported_extension(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("json" | "yaml" | "yml")
    )
}
pub use migrations::{load_migrations, load_migrations_at_compile_time, load_migrations_from_dir};
pub use models::{load_models, load_models_at_compile_time, load_models_from_dir};
