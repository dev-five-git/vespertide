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

    let schema_url = schema_url_for(format);
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
        ModelFormat::Json => write_json_with_schema(&path, &table, &schema_url)?,
        ModelFormat::Yaml | ModelFormat::Yml => write_yaml(&path, &table, &schema_url)?,
    }

    println!("Created model template: {}", path.display());
    Ok(())
}

fn schema_url_for(format: ModelFormat) -> String {
    // If not set, default to public raw GitHub schema location.
    // Users can override via VESP_SCHEMA_BASE_URL.
    let base = std::env::var("VESP_SCHEMA_BASE_URL").ok();
    let base = base.as_deref().unwrap_or(
        "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas",
    );
    let base = base.trim_end_matches('/');
    match format {
        ModelFormat::Json => format!("{}/model.schema.json", base),
        ModelFormat::Yaml | ModelFormat::Yml => format!("{}/model.schema.json", base),
    }
}

fn write_json_with_schema(
    path: &std::path::Path,
    table: &TableDef,
    schema_url: &str,
) -> Result<()> {
    let mut value = serde_json::to_value(table).context("serialize table to json")?;
    if let Value::Object(ref mut map) = value {
        map.insert("$schema".to_string(), Value::String(schema_url.to_string()));
    }
    let text = serde_json::to_string_pretty(&value).context("stringify json with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

fn write_yaml(path: &std::path::Path, table: &TableDef, schema_url: &str) -> Result<()> {
    let mut value = serde_yaml::to_value(table).context("serialize table to yaml value")?;
    if let serde_yaml::Value::Mapping(ref mut map) = value {
        map.insert(
            serde_yaml::Value::String("$schema".to_string()),
            serde_yaml::Value::String(schema_url.to_string()),
        );
    }
    let text = serde_yaml::to_string(&value).context("serialize yaml with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}
