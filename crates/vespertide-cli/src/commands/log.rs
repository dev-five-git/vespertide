use anyhow::Result;
use colored::Colorize;
use vespertide_loader::load_config;
use vespertide_planner::apply_action;
use vespertide_query::{DatabaseBackend, PlanQueriesOptions, build_plan_queries_with_options};

use crate::utils::load_migrations;

pub async fn cmd_log(backend: DatabaseBackend, transaction: bool) -> Result<()> {
    let config = load_config()?;
    let plans = load_migrations(&config)?;

    if plans.is_empty() {
        println!("{}", "No migrations found.".bright_yellow());
        return Ok(());
    }

    // Apply prefix to all migration plans
    let prefix = config.prefix();
    let prefixed_plans: Vec<_> = plans.into_iter().map(|p| p.with_prefix(prefix)).collect();

    println!(
        "{} {} {}",
        "Migrations".bright_cyan().bold(),
        "(oldest -> newest):".bright_white(),
        prefixed_plans.len().to_string().bright_yellow().bold()
    );
    println!();

    // Build baseline schema incrementally as we iterate through migrations
    let mut baseline_schema = Vec::new();

    for plan in &prefixed_plans {
        println!(
            "{} {}",
            "Version:".bright_cyan().bold(),
            plan.version.to_string().bright_magenta().bold()
        );
        if let Some(created) = &plan.created_at {
            println!(
                "  {} {}",
                "Created at:".bright_cyan(),
                created.bright_white()
            );
        }
        if let Some(comment) = &plan.comment {
            println!("  {} {}", "Comment:".bright_cyan(), comment.bright_white());
        }
        println!(
            "  {} {}",
            "Actions:".bright_cyan(),
            plan.actions.len().to_string().bright_yellow()
        );

        // Use the current baseline schema (from all previous migrations).
        // Each applied migration is its own transaction at runtime, so
        // `--transaction` wraps each plan's statement stream independently.
        let plan_queries = build_plan_queries_with_options(
            plan,
            &baseline_schema,
            PlanQueriesOptions {
                wrap_in_transaction: transaction,
            },
        )
        .map_err(|e| anyhow::anyhow!("query build error for v{}: {}", plan.version, e))?;

        // Update baseline schema incrementally by applying each action
        for action in &plan.actions {
            let _ = apply_action(&mut baseline_schema, action);
        }

        for (i, pq) in plan_queries.iter().enumerate() {
            let queries = match backend {
                DatabaseBackend::Postgres => &pq.postgres,
                DatabaseBackend::MySql => &pq.mysql,
                DatabaseBackend::Sqlite => &pq.sqlite,
            };

            // Build non-empty SQL statements
            let sql_statements: Vec<String> = queries
                .iter()
                .map(|q| q.build(backend).trim().to_string())
                .filter(|sql| !sql.is_empty())
                .collect();

            // Print action description
            println!(
                "    {}. {}",
                (i + 1).to_string().bright_magenta().bold(),
                pq.action.to_string().bright_cyan()
            );

            // Print SQL statements with sub-numbering if multiple
            for (j, sql) in sql_statements.iter().enumerate() {
                let prefix = if sql_statements.len() > 1 {
                    format!("    {}-{}.", i + 1, j + 1)
                        .bright_magenta()
                        .bold()
                        .to_string()
                } else {
                    "      ".to_string()
                };
                println!("{} {}", prefix, sql.bright_white());
            }
        }

        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::CwdGuard;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{MigrationAction, MigrationPlan};

    fn write_config(cfg: &VespertideConfig) {
        let text = serde_json::to_string_pretty(cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
    }

    fn write_migration(cfg: &VespertideConfig) {
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let plan = MigrationPlan {
            id: String::new(),
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

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_with_single_migration_postgres() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        write_migration(&cfg);

        let result = cmd_log(DatabaseBackend::Postgres, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_with_single_migration_mysql() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        write_migration(&cfg);

        let result = cmd_log(DatabaseBackend::MySql, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_with_single_migration_sqlite() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        write_migration(&cfg);

        let result = cmd_log(DatabaseBackend::Sqlite, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_no_migrations_postgres() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        let result = cmd_log(DatabaseBackend::Postgres, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_no_migrations_mysql() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        let result = cmd_log(DatabaseBackend::MySql, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_no_migrations_sqlite() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        let result = cmd_log(DatabaseBackend::Sqlite, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_with_transaction_flag_runs() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        write_migration(&cfg);

        // --transaction parity: each migration's stream wraps independently.
        let result = cmd_log(DatabaseBackend::Postgres, true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_log_with_multiple_sql_statements() {
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = VespertideConfig::default();
        write_config(&cfg);
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        // Create a migration with ModifyColumnType for SQLite, which generates multiple SQL statements
        let plan = MigrationPlan {
            id: String::new(),
            comment: Some("modify column type".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![ColumnDef {
                        name: "id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: Some(PrimaryKeySyntax::Bool(true)),
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }],
                    constraints: vec![],
                },
                MigrationAction::ModifyColumnType {
                    table: "users".into(),
                    column: "id".into(),
                    new_type: ColumnType::Simple(SimpleColumnType::BigInt),
                    fill_with: None,
                    narrowing_strategy: None,
                    timezone: None,
                },
            ],
        };
        let path = cfg.migrations_dir().join("0001_modify_column_type.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        // SQLite backend will generate multiple SQL statements for ModifyColumnType (table recreation)
        // This exercises line 84 where sql_statements.len() > 1
        let result = cmd_log(DatabaseBackend::Sqlite, false).await;
        assert!(result.is_ok());
    }
}
