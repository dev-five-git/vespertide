use anyhow::Result;
use colored::Colorize;
use vespertide_query::build_plan_queries;

use crate::utils::load_migrations;

pub fn cmd_log() -> Result<()> {
    let plans = load_migrations(&crate::utils::load_config()?)?;

    if plans.is_empty() {
        println!("{}", "No migrations found.".bright_yellow());
        return Ok(());
    }

    println!(
        "{} {} {}",
        "Migrations".bright_cyan().bold(),
        "(oldest -> newest):".bright_white(),
        plans.len().to_string().bright_yellow().bold()
    );
    println!();

    for plan in &plans {
        println!(
            "{} {}",
            "Version:".bright_cyan().bold(),
            plan.version.to_string().bright_magenta().bold()
        );
        if let Some(created) = &plan.created_at {
            println!("  {} {}", "Created at:".bright_cyan(), created.bright_white());
        }
        if let Some(comment) = &plan.comment {
            println!("  {} {}", "Comment:".bright_cyan(), comment.bright_white());
        }
        println!(
            "  {} {}",
            "Actions:".bright_cyan(),
            plan.actions.len().to_string().bright_yellow()
        );

        let queries = build_plan_queries(plan)
            .map_err(|e| anyhow::anyhow!("query build error for v{}: {}", plan.version, e))?;
        println!(
            "  {} {}",
            "SQL statements:".bright_cyan().bold(),
            queries.len().to_string().bright_yellow().bold()
        );

        for (i, q) in queries.iter().enumerate() {
            println!(
                "    {}. {}",
                (i + 1).to_string().bright_magenta().bold(),
                q.sql().trim().bright_white()
            );
            println!("       {} {:?}", "binds:".bright_cyan(), q.binds());
        }
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, path::PathBuf};
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{MigrationAction, MigrationPlan};

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &PathBuf) -> Self {
            let original = env::current_dir().unwrap();
            env::set_current_dir(dir).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    fn write_config(cfg: &VespertideConfig) {
        let text = serde_json::to_string_pretty(cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
    }

    fn write_migration(cfg: &VespertideConfig) {
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let plan = MigrationPlan {
            comment: Some("init".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![],
                constraints: vec![],
            }],
        };
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn cmd_log_with_single_migration() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        write_migration(&cfg);

        let result = cmd_log();
        assert!(result.is_ok());
    }

    #[test]
    #[serial_test::serial]
    fn cmd_log_no_migrations() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        let result = cmd_log();
        assert!(result.is_ok());
    }
}
