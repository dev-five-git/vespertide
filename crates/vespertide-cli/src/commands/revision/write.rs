use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::fs;
use vespertide_config::{FileFormat, VespertideConfig};
use vespertide_core::MigrationPlan;

use crate::utils::{
    migration_filename_with_format_and_pattern, schema_url, write_json_with_schema,
    write_yaml_with_schema,
};

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
        FileFormat::Yaml | FileFormat::Yml => {
            write_yaml_with_schema(&path, plan, &schema_url).await?;
        }
    }

    Ok(path)
}
