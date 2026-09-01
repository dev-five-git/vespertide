//! Shared test-only helpers for the vespertide-cli crate.
//!
//! All items are gated under `#[cfg(test)]` via the parent module declaration
//! in `main.rs` and exposed `pub(crate)` so every inline `mod tests` and
//! `commands/<cmd>/tests/mod.rs` entry can reuse the same implementation.

use std::fs;
use std::path::{Path, PathBuf};

use vespertide_config::VespertideConfig;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};

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

/// Writes a default `vespertide.json` to the current directory and returns
/// the matching `VespertideConfig`. Centralises the byte-identical bootstrap
/// previously open-coded in every CLI command's test module.
pub(crate) fn write_default_config() -> VespertideConfig {
    let cfg = VespertideConfig::default();
    let text = serde_json::to_string_pretty(&cfg).unwrap();
    fs::write("vespertide.json", text).unwrap();
    cfg
}

/// Writes a single-column `{name}.json` model with an `id INTEGER NOT NULL`
/// PK to `models/{name}.json`. Centralises the byte-identical bootstrap
/// previously open-coded in every CLI command's test module.
pub(crate) fn write_simple_id_model(name: &str) {
    let models_dir = PathBuf::from("models");
    fs::create_dir_all(&models_dir).unwrap();
    let table = TableDef {
        name: name.into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let path = models_dir.join(format!("{name}.json"));
    fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
}
