use std::fs;

use anyhow::{Context, Result};
use chrono::Utc;
use vespertide_planner::plan_next_migration;

use crate::utils::{
    load_config, load_migrations, load_models, migration_filename_with_format_and_pattern,
};

pub fn cmd_revision(message: String) -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let mut plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    if plan.actions.is_empty() {
        println!("No changes detected. Nothing to migrate.");
        return Ok(());
    }

    plan.comment = Some(message);
    if plan.created_at.is_none() {
        // Record creation time in RFC3339 (UTC).
        plan.created_at = Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }

    let migrations_dir = config.migrations_dir();
    if !migrations_dir.exists() {
        fs::create_dir_all(&migrations_dir)
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

    let text = match format {
        vespertide_config::FileFormat::Json => {
            serde_json::to_string_pretty(&plan).context("serialize migration plan")?
        }
        _ => serde_yaml::to_string(&plan).context("serialize migration plan")?,
    };

    fs::write(&path, text)
        .with_context(|| format!("write migration file: {}", path.display()))?;

    println!("Created migration: {}", path.display());
    println!("  Version: {}", plan.version);
    println!("  Actions: {}", plan.actions.len());
    if let Some(comment) = &plan.comment {
        println!("  Comment: {}", comment);
    }

    Ok(())
}
