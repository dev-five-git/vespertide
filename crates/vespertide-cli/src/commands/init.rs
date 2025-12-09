use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use vespertide_config::VespertideConfig;

pub fn cmd_init() -> Result<()> {
    let path = PathBuf::from("vespertide.json");
    if path.exists() {
        bail!("vespertide.json already exists");
    }

    let config = VespertideConfig::default();
    let json = serde_json::to_string_pretty(&config).context("serialize default config")?;
    fs::write(&path, json).context("write vespertide.json")?;
    println!("created {:?}", path);
    Ok(())
}

