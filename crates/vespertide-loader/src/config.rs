use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vespertide_config::VespertideConfig;

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

/// Load config from a specific path.
pub fn load_config_from_path(path: PathBuf) -> Result<VespertideConfig> {
    if !path.exists() {
        anyhow::bail!("vespertide.json not found at: {}", path.display());
    }

    let content = fs::read_to_string(&path).context("read vespertide.json")?;
    let config: VespertideConfig =
        serde_json::from_str(&content).context("parse vespertide.json")?;
    Ok(config)
}

/// Load config from project root, with fallback to defaults.
pub fn load_config_or_default(project_root: Option<PathBuf>) -> Result<VespertideConfig> {
    let config_path = if let Some(root) = project_root {
        root.join("vespertide.json")
    } else {
        PathBuf::from("vespertide.json")
    };

    if config_path.exists() {
        load_config_from_path(config_path)
    } else {
        Ok(VespertideConfig::default())
    }
}
