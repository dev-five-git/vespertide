use anyhow::Result;
use vespertide_planner::schema_from_plans;

use crate::utils::{load_config, load_migrations, load_models};
use std::collections::HashSet;

pub fn cmd_status() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    println!("Configuration:");
    println!("  Models directory: {}", config.models_dir().display());
    println!("  Migrations directory: {}", config.migrations_dir().display());
    println!("  Table naming: {:?}", config.table_naming_case);
    println!("  Column naming: {:?}", config.column_naming_case);
    println!("  Model format: {:?}", config.model_format());
    println!("  Migration format: {:?}", config.migration_format());
    println!(
        "  Migration filename pattern: {}",
        config.migration_filename_pattern()
    );
    println!();

    println!("Applied migrations: {}", applied_plans.len());
    if !applied_plans.is_empty() {
        let latest = applied_plans.last().unwrap();
        println!("  Latest version: {}", latest.version);
        if let Some(comment) = &latest.comment {
            println!("  Latest comment: {}", comment);
        }
        if let Some(created_at) = &latest.created_at {
            println!("  Latest created at: {}", created_at);
        }
    }
    println!();

    println!("Current models: {}", current_models.len());
    for model in &current_models {
        println!("  - {} ({} columns, {} indexes)", 
            model.name, 
            model.columns.len(),
            model.indexes.len());
    }
    println!();

    if !applied_plans.is_empty() {
        let baseline = schema_from_plans(&applied_plans)
            .map_err(|e| anyhow::anyhow!("schema reconstruction error: {}", e))?;
        
        let baseline_tables: HashSet<_> = 
            baseline.iter().map(|t| &t.name).collect();
        let current_tables: HashSet<_> = 
            current_models.iter().map(|t| &t.name).collect();

        if baseline_tables == current_tables {
            println!("Status: Schema is synchronized with migrations.");
        } else {
            println!("Status: Schema differs from applied migrations.");
            println!("  Run 'vespertide diff' to see details.");
        }
    } else if current_models.is_empty() {
        println!("Status: No models or migrations found.");
    } else {
        println!("Status: Models exist but no migrations have been applied.");
        println!("  Run 'vespertide revision -m \"initial\"' to create the first migration.");
    }

    Ok(())
}
