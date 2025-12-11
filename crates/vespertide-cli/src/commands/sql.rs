use anyhow::Result;
use colored::Colorize;
use vespertide_planner::plan_next_migration;
use vespertide_query::build_plan_queries;

use crate::utils::{load_config, load_migrations, load_models};

pub fn cmd_sql() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    emit_sql(&plan)
}

fn emit_sql(plan: &vespertide_core::MigrationPlan) -> Result<()> {
    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No differences found.".bright_green(),
            "Schema is up to date; no SQL to emit.".bright_white()
        );
        return Ok(());
    }

    let queries =
        build_plan_queries(plan).map_err(|e| anyhow::anyhow!("query build error: {}", e))?;

    println!(
        "{} {}",
        "Plan version:".bright_cyan().bold(),
        plan.version.to_string().bright_magenta()
    );
    if let Some(created_at) = &plan.created_at {
        println!(
            "{} {}",
            "Created at:".bright_cyan(),
            created_at.bright_white()
        );
    }
    if let Some(comment) = &plan.comment {
        println!("{} {}", "Comment:".bright_cyan(), comment.bright_white());
    }
    println!(
        "{} {}",
        "Actions:".bright_cyan(),
        plan.actions.len().to_string().bright_yellow()
    );
    println!(
        "{} {}",
        "SQL statements:".bright_cyan().bold(),
        queries.len().to_string().bright_yellow().bold()
    );
    println!();

    for (i, q) in queries.iter().enumerate() {
        println!(
            "{}. {}",
            (i + 1).to_string().bright_magenta().bold(),
            q.sql.trim().bright_white()
        );
        if !q.binds.is_empty() {
            println!("   {} {:?}", "binds:".bright_cyan(), q.binds);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, TableConstraint, TableDef,
    };

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &PathBuf) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    fn write_config() -> VespertideConfig {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
        cfg
    }

    fn write_model(name: &str) {
        let models_dir = PathBuf::from("models");
        fs::create_dir_all(&models_dir).unwrap();
        let table = TableDef {
            name: name.to_string(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Integer,
                nullable: false,
                default: None,
            }],
            constraints: vec![],
            indexes: vec![],
        };
        let path = models_dir.join(format!("{name}.json"));
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
    }

    #[test]
    #[serial]
    fn cmd_sql_emits_queries() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        let result = cmd_sql();
        assert!(result.is_ok());
    }

    #[test]
    fn emit_sql_no_actions_early_return() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![],
        };
        assert!(emit_sql(&plan).is_ok());
    }

    #[test]
    fn emit_sql_with_metadata() {
        let plan = MigrationPlan {
            comment: Some("init".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Integer,
                    nullable: false,
                    default: None,
                }],
                constraints: vec![TableConstraint::PrimaryKey {
                    columns: vec!["id".into()],
                }],
            }],
        };
        assert!(emit_sql(&plan).is_ok());
    }
}
