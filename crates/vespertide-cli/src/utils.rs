use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vespertide_config::VespertideConfig;
use vespertide_core::{MigrationPlan, TableDef};

/// Load vespertide.json config from current directory.
pub fn load_config() -> Result<VespertideConfig> {
    let path = PathBuf::from("vespertide.json");
    if !path.exists() {
        anyhow::bail!("vespertide.json not found. Run 'vespertide init' first.");
    }

    let content = fs::read_to_string(&path).context("read vespertide.json")?;
    let config: VespertideConfig =
        serde_json::from_str(&content).context("parse vespertide.json")?;
    Ok(config)
}

/// Load all model definitions from the models directory.
pub fn load_models(config: &VespertideConfig) -> Result<Vec<TableDef>> {
    let models_dir = config.models_dir();
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tables = Vec::new();
    let entries = fs::read_dir(models_dir).context("read models directory")?;

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path).with_context(|| {
                    format!("read model file: {}", path.display())
                })?;

                let table: TableDef = if ext == Some("json") {
                    serde_json::from_str(&content)
                        .with_context(|| format!("parse JSON model: {}", path.display()))?
                } else {
                    // For now, only JSON is supported. YAML support can be added later.
                    anyhow::bail!("YAML support not yet implemented: {}", path.display());
                };

                tables.push(table);
            }
        }
    }

    Ok(tables)
}

/// Load all migration plans from the migrations directory, sorted by version.
pub fn load_migrations(config: &VespertideConfig) -> Result<Vec<MigrationPlan>> {
    let migrations_dir = config.migrations_dir();
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = fs::read_dir(migrations_dir).context("read migrations directory")?;

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") {
                let content = fs::read_to_string(&path).with_context(|| {
                    format!("read migration file: {}", path.display())
                })?;

                let plan: MigrationPlan = serde_json::from_str(&content)
                    .with_context(|| format!("parse migration: {}", path.display()))?;

                plans.push(plan);
            }
        }
    }

    // Sort by version number
    plans.sort_by_key(|p| p.version);
    Ok(plans)
}

/// Generate a migration filename from version and optional comment.
pub fn migration_filename(version: u32, comment: Option<&str>) -> String {
    let sanitized = comment
        .map(|c| {
            c.to_lowercase()
                .chars()
                .map(|ch| if ch.is_alphanumeric() || ch == ' ' { ch } else { '_' })
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("_")
        })
        .unwrap_or_default();

    if sanitized.is_empty() {
        format!("{:04}_migration.json", version)
    } else {
        format!("{:04}_{}.json", version, sanitized)
    }
}

