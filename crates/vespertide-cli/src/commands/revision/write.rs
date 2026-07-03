use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::fs;
use vespertide_config::{FileFormat, VespertideConfig};
use vespertide_core::MigrationPlan;

use crate::utils::{migration_filename_with_format_and_pattern, schema_url};

pub(super) async fn write_migration_file(
    config: &VespertideConfig,
    plan: &MigrationPlan,
) -> Result<PathBuf> {
    let migrations_dir = config.migrations_dir();
    if !migrations_dir.exists() {
        fs::create_dir_all(&migrations_dir)
            .await
            .context("create migrations directory")?;
    }

    let format = config.migration_format();
    let filename = migration_filename_with_format_and_pattern(
        plan.version,
        plan.comment.as_deref(),
        format,
        config.migration_filename_pattern(),
    );
    let path = migrations_dir.join(&filename);

    let schema_url = schema_url("migration.schema.json");
    match format {
        FileFormat::Json => write_json_with_schema(&path, plan, &schema_url).await?,
        FileFormat::Yaml | FileFormat::Yml => write_yaml(&path, plan, &schema_url).await?,
    }

    Ok(path)
}

pub(super) async fn write_json_with_schema(
    path: &Path,
    plan: &MigrationPlan,
    schema_url: &str,
) -> Result<()> {
    let mut value = serde_json::to_value(plan).context("serialize migration plan to json")?;
    if let Value::Object(ref mut map) = value {
        map.insert("$schema".to_string(), Value::String(schema_url.to_string()));
    }
    let text = serde_json::to_string_pretty(&value).context("stringify json with schema")?;
    fs::write(path, text)
        .await
        .with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

pub(super) async fn write_yaml(path: &Path, plan: &MigrationPlan, schema_url: &str) -> Result<()> {
    let mut value = serde_yaml::to_value(plan).context("serialize migration plan to yaml value")?;
    if let serde_yaml::Value::Mapping(ref mut map) = value {
        map.insert(
            serde_yaml::Value::String("$schema".to_string()),
            serde_yaml::Value::String(schema_url.to_string()),
        );
    }
    let text = serde_yaml::to_string(&value).context("serialize yaml with schema")?;
    fs::write(path, text)
        .await
        .with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}
