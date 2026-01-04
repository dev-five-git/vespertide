use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use schemars::schema_for;
use vespertide_config::VespertideConfig;
use vespertide_core::{MigrationPlan, TableDef};

#[derive(Debug, Parser)]
#[command(
    name = "vespertide-schema-gen",
    about = "Emit JSON Schemas for vespertide models and migrations."
)]
struct Args {
    /// Output directory for schema files.
    #[arg(short = 'o', long = "out", default_value = "schemas")]
    out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    run(args.out)
}

fn run(out: PathBuf) -> Result<()> {
    if !out.exists() {
        fs::create_dir_all(&out).with_context(|| format!("create dir {}", out.display()))?;
    }

    let model_schema = schema_for!(TableDef);
    let migration_schema = schema_for!(MigrationPlan);
    let config_schema = schema_for!(VespertideConfig);

    let model_path = out.join("model.schema.json");
    let migration_path = out.join("migration.schema.json");
    let config_path = out.join("config.schema.json");

    fs::write(
        &model_path,
        serde_json::to_string_pretty(&model_schema).context("serialize model schema")?,
    )
    .with_context(|| format!("write {}", model_path.display()))?;

    fs::write(
        &migration_path,
        serde_json::to_string_pretty(&migration_schema).context("serialize migration schema")?,
    )
    .with_context(|| format!("write {}", migration_path.display()))?;

    fs::write(
        &config_path,
        serde_json::to_string_pretty(&config_schema).context("serialize config schema")?,
    )
    .with_context(|| format!("write {}", config_path.display()))?;

    println!("Wrote schemas:");
    println!("  {}", model_path.display());
    println!("  {}", migration_path.display());
    println!("  {}", config_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn run_creates_output_directory_if_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path().join("test_schemas");

        assert!(!out.exists());
        run(out.clone()).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn run_generates_model_schema_file() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path();

        run(out.to_path_buf()).unwrap();

        let model_path = out.join("model.schema.json");
        assert!(model_path.exists());

        let content = fs::read_to_string(&model_path).unwrap();
        assert!(content.contains("TableDef"));
        assert!(content.contains("ColumnDef"));
    }

    #[test]
    fn run_generates_migration_schema_file() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path();

        run(out.to_path_buf()).unwrap();

        let migration_path = out.join("migration.schema.json");
        assert!(migration_path.exists());

        let content = fs::read_to_string(&migration_path).unwrap();
        assert!(content.contains("MigrationPlan"));
        assert!(content.contains("MigrationAction"));
    }

    #[test]
    fn run_generates_all_schema_files() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path();

        run(out.to_path_buf()).unwrap();

        let model_path = out.join("model.schema.json");
        let migration_path = out.join("migration.schema.json");
        let config_path = out.join("config.schema.json");

        assert!(model_path.exists());
        assert!(migration_path.exists());
        assert!(config_path.exists());

        // Verify files are valid JSON
        let model_content = fs::read_to_string(&model_path).unwrap();
        let migration_content = fs::read_to_string(&migration_path).unwrap();
        let config_content = fs::read_to_string(&config_path).unwrap();

        serde_json::from_str::<serde_json::Value>(&model_content).unwrap();
        serde_json::from_str::<serde_json::Value>(&migration_content).unwrap();
        serde_json::from_str::<serde_json::Value>(&config_content).unwrap();
    }

    #[test]
    fn run_works_with_existing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path();

        // Create directory first
        fs::create_dir_all(out).unwrap();
        assert!(out.exists());

        // Should still work
        run(out.to_path_buf()).unwrap();

        let model_path = out.join("model.schema.json");
        let migration_path = out.join("migration.schema.json");
        let config_path = out.join("config.schema.json");
        assert!(model_path.exists());
        assert!(migration_path.exists());
        assert!(config_path.exists());
    }

    #[test]
    fn run_generates_config_schema_file() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path();

        run(out.to_path_buf()).unwrap();

        let config_path = out.join("config.schema.json");
        assert!(config_path.exists());

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("VespertideConfig"));
        assert!(content.contains("modelsDir"));
        assert!(content.contains("migrationsDir"));
    }
}
