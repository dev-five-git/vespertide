use std::fs;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use vespertide_core::TableDef;

use crate::{ModelFormat, utils::load_config};

pub fn cmd_new(name: String, format: ModelFormat) -> Result<()> {
    let config = load_config()?;
    let dir = config.models_dir();
    if !dir.exists() {
        fs::create_dir_all(dir).context("create models directory")?;
    }

    let ext = match format {
        ModelFormat::Json => "json",
        ModelFormat::Yaml => "yaml",
        ModelFormat::Yml => "yml",
    };

    let path = dir.join(format!("{name}.{ext}"));
    if path.exists() {
        bail!("model file already exists: {}", path.display());
    }

    let table = TableDef {
        name: name.clone(),
        columns: Vec::new(),
        constraints: Vec::new(),
        indexes: Vec::new(),
    };

    match format {
        ModelFormat::Json => write_json_with_schema(&path, &table)?,
        ModelFormat::Yaml | ModelFormat::Yml => write_yaml(&path, &table)?,
    }

    println!("Created model template: {}", path.display());
    Ok(())
}

fn write_json_with_schema(path: &std::path::Path, table: &TableDef) -> Result<()> {
    let mut value = serde_json::to_value(table).context("serialize table to json")?;
    if let Value::Object(ref mut map) = value {
        map.insert(
            "$schema".to_string(),
            Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
        );
    }
    let text = serde_json::to_string_pretty(&value).context("stringify json with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

fn write_yaml(path: &std::path::Path, table: &TableDef) -> Result<()> {
    let text = serde_yaml::to_string(table).context("serialize table to yaml")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}
