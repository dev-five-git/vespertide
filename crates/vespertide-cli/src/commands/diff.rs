use anyhow::Result;
use vespertide_planner::plan_next_migration;

use crate::utils::{load_config, load_migrations, load_models};
use vespertide_core::MigrationAction;

pub fn cmd_diff() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    if plan.actions.is_empty() {
        println!("No differences found. Schema is up to date.");
        return Ok(());
    }

    println!("Found {} change(s) to apply:", plan.actions.len());
    println!();

    for (i, action) in plan.actions.iter().enumerate() {
        println!("{}. {}", i + 1, format_action(action));
    }

    Ok(())
}

fn format_action(action: &MigrationAction) -> String {
    match action {
        MigrationAction::CreateTable { table, .. } => {
            format!("Create table: {}", table)
        }
        MigrationAction::DeleteTable { table } => {
            format!("Delete table: {}", table)
        }
        MigrationAction::AddColumn { table, column, .. } => {
            format!("Add column: {}.{}", table, column.name)
        }
        MigrationAction::RenameColumn { table, from, to } => {
            format!("Rename column: {}.{} -> {}", table, from, to)
        }
        MigrationAction::DeleteColumn { table, column } => {
            format!("Delete column: {}.{}", table, column)
        }
        MigrationAction::ModifyColumnType { table, column, .. } => {
            format!("Modify column type: {}.{}", table, column)
        }
        MigrationAction::AddIndex { table, index } => {
            format!("Add index: {} on {}", index.name, table)
        }
        MigrationAction::RemoveIndex { table, name } => {
            format!("Remove index: {} from {}", name, table)
        }
        MigrationAction::RenameTable { from, to } => {
            format!("Rename table: {} -> {}", from, to)
        }
    }
}
