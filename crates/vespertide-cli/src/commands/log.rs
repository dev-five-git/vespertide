use anyhow::Result;
use vespertide_query::build_plan_queries;

use crate::utils::load_migrations;

pub fn cmd_log() -> Result<()> {
    let plans = load_migrations(&crate::utils::load_config()?)?;

    if plans.is_empty() {
        println!("No migrations found.");
        return Ok(());
    }

    println!("Migrations (oldest -> newest): {}", plans.len());
    println!();

    for plan in &plans {
        println!("Version: {}", plan.version);
        if let Some(created) = &plan.created_at {
            println!("Created at: {}", created);
        }
        if let Some(comment) = &plan.comment {
            println!("Comment: {}", comment);
        }
        println!("Actions: {}", plan.actions.len());

        let queries = build_plan_queries(plan)
            .map_err(|e| anyhow::anyhow!("query build error for v{}: {}", plan.version, e))?;
        println!("SQL statements: {}", queries.len());

        for (i, q) in queries.iter().enumerate() {
            println!("  {}. {}", i + 1, q.sql.trim());
            if !q.binds.is_empty() {
                println!("     binds: {:?}", q.binds);
            }
        }
        println!();
    }

    Ok(())
}

