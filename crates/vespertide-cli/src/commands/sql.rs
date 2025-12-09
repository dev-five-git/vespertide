use anyhow::Result;
use vespertide_planner::plan_next_migration;
use vespertide_query::build_plan_queries;

use crate::utils::{load_config, load_migrations, load_models};

pub fn cmd_sql() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    if plan.actions.is_empty() {
        println!("No differences found. Schema is up to date; no SQL to emit.");
        return Ok(());
    }

    let queries = build_plan_queries(&plan)
        .map_err(|e| anyhow::anyhow!("query build error: {}", e))?;

    println!("Plan version: {}", plan.version);
    if let Some(created_at) = &plan.created_at {
        println!("Created at: {}", created_at);
    }
    if let Some(comment) = &plan.comment {
        println!("Comment: {}", comment);
    }
    println!("Actions: {}", plan.actions.len());
    println!("SQL statements: {}", queries.len());
    println!();

    for (i, q) in queries.iter().enumerate() {
        println!("{}. {}", i + 1, q.sql.trim());
        if !q.binds.is_empty() {
            println!("   binds: {:?}", q.binds);
        }
    }

    Ok(())
}

