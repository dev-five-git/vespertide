use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use schemars::schema_for;
use vespertide_core::{MigrationPlan, TableDef};

#[derive(Debug, Parser)]
#[command(name = "vespertide-schema-gen", about = "Emit JSON Schemas for vespertide models and migrations.")]
struct Args {
    /// Output directory for schema files.
    #[arg(short = 'o', long = "out", default_value = "schemas")]
    out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let out = args.out;

    if !out.exists() {
        fs::create_dir_all(&out).with_context(|| format!("create dir {}", out.display()))?;
    }

    let model_schema = schema_for!(TableDef);
    let migration_schema = schema_for!(MigrationPlan);

    let model_path = out.join("model.schema.json");
    let migration_path = out.join("migration.schema.json");

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

    println!("Wrote schemas:");
    println!("  {}", model_path.display());
    println!("  {}", migration_path.display());
    Ok(())
}

