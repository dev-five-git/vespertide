use std::fs;
use std::path::{Path, PathBuf};

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

#[cfg(not(tarpaulin_include))]
fn main() -> Result<()> {
    let args = Args::parse();
    run(&args.out)
}

fn run(out: &Path) -> Result<()> {
    if !out.exists() {
        fs::create_dir_all(out).with_context(|| format!("create dir {}", out.display()))?;
    }

    let model_schema = schema_for!(TableDef);
    let migration_schema = schema_for!(MigrationPlan);
    let config_schema = schema_for!(VespertideConfig);

    let model_path = out.join("model.schema.json");
    let migration_path = out.join("migration.schema.json");
    let config_path = out.join("config.schema.json");

    // **Model-only strip**: migration-time concern fields (`strategy`,
    // `orphan_strategy`) are valid in `migration.schema.json` (vespertide
    // stamps them via the revision CLI) but must never appear in
    // `model.schema.json` — user-facing model files should never carry
    // them and IDE autocompletion would otherwise wrongly suggest them.
    //
    // Done as a post-process JSON walk because the same Rust
    // `TableConstraint` type backs both schemas and a single
    // `#[schemars(skip)]` attribute would hide the fields in both.
    let mut model_value: serde_json::Value =
        serde_json::to_value(&model_schema).context("serialize model schema to value")?;
    strip_migration_fields(&mut model_value);

    fs::write(
        &model_path,
        serde_json::to_string_pretty(&model_value).context("serialize stripped model schema")?,
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

/// Field names that vespertide treats as **migration-time concerns**
/// stamped by the revision CLI. They are valid in migration JSON
/// (vespertide writes them) but must never appear in user-facing
/// model JSON.
const MIGRATION_ONLY_FIELDS: &[&str] = &["strategy", "orphan_strategy"];

/// Recursively walk a JSON Schema document and remove every occurrence
/// of `MIGRATION_ONLY_FIELDS` from `properties` and `required` blocks.
/// Operates on the schemars output as plain `serde_json::Value` so it
/// survives schemars major-version changes.
fn strip_migration_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            // 1) Remove from any `"properties": { "<field>": ... }` map.
            if let Some(serde_json::Value::Object(props)) = map.get_mut("properties") {
                for field in MIGRATION_ONLY_FIELDS {
                    props.remove(*field);
                }
            }
            // 2) Remove from any `"required": ["<field>", ...]` array.
            if let Some(serde_json::Value::Array(req)) = map.get_mut("required") {
                req.retain(|v| {
                    let s = v.as_str().unwrap_or("");
                    !MIGRATION_ONLY_FIELDS.contains(&s)
                });
            }
            // 3) Recurse into every child value.
            for (_, v) in map.iter_mut() {
                strip_migration_fields(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_migration_fields(v);
            }
        }
        _ => {}
    }
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
        run(&out).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn run_generates_model_schema_file() {
        let temp_dir = TempDir::new().unwrap();
        let out = temp_dir.path();

        run(out).unwrap();

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

        run(out).unwrap();

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

        run(out).unwrap();

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
        run(out).unwrap();

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

        run(out).unwrap();

        let config_path = out.join("config.schema.json");
        assert!(config_path.exists());

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("VespertideConfig"));
        assert!(content.contains("modelsDir"));
        assert!(content.contains("migrationsDir"));
    }
}
