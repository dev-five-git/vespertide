use anyhow::Result;
use colored::Colorize;
use vespertide_planner::schema_from_plans;

use crate::utils::{load_config, load_migrations, load_models};
use std::collections::HashSet;

pub fn cmd_status() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    println!("{}", "Configuration:".bright_cyan().bold());
    println!(
        "  {} {}",
        "Models directory:".cyan(),
        format!("{}", config.models_dir().display()).bright_white()
    );
    println!(
        "  {} {}",
        "Migrations directory:".cyan(),
        format!("{}", config.migrations_dir().display()).bright_white()
    );
    println!(
        "  {} {:?}",
        "Table naming:".cyan(),
        config.table_naming_case
    );
    println!(
        "  {} {:?}",
        "Column naming:".cyan(),
        config.column_naming_case
    );
    println!("  {} {:?}", "Model format:".cyan(), config.model_format());
    println!(
        "  {} {:?}",
        "Migration format:".cyan(),
        config.migration_format()
    );
    println!(
        "  {} {}",
        "Migration filename pattern:".cyan(),
        config.migration_filename_pattern().bright_white()
    );
    println!();

    println!(
        "{} {}",
        "Applied migrations:".bright_cyan().bold(),
        applied_plans.len().to_string().bright_yellow()
    );
    if !applied_plans.is_empty() {
        let latest = applied_plans.last().unwrap();
        println!(
            "  {} {}",
            "Latest version:".cyan(),
            latest.version.to_string().bright_magenta()
        );
        if let Some(comment) = &latest.comment {
            println!("  {} {}", "Latest comment:".cyan(), comment.bright_white());
        }
        if let Some(created_at) = &latest.created_at {
            println!(
                "  {} {}",
                "Latest created at:".cyan(),
                created_at.bright_white()
            );
        }
    }
    println!();

    println!(
        "{} {}",
        "Current models:".bright_cyan().bold(),
        current_models.len().to_string().bright_yellow()
    );
    for model in &current_models {
        // Count Index constraints
        let index_count = model
            .constraints
            .iter()
            .filter(|c| matches!(c, vespertide_core::TableConstraint::Index { .. }))
            .count();
        // Count Unique constraints
        let unique_count = model
            .constraints
            .iter()
            .filter(|c| matches!(c, vespertide_core::TableConstraint::Unique { .. }))
            .count();
        print!(
            "  {} {} ({} {}, {} {}, {} {})",
            "-".bright_white(),
            model.name.bright_green(),
            model.columns.len().to_string().bright_blue(),
            "columns".bright_white(),
            index_count.to_string().bright_blue(),
            "indexes".bright_white(),
            unique_count.to_string().bright_blue(),
            "uniques".bright_white()
        );
        if let Some(description) = &model.description {
            println!(
                "\n    {} {}",
                "Description:".bright_black(),
                description.bright_white()
            );
        } else {
            println!();
        }
    }
    println!();

    if !applied_plans.is_empty() {
        let baseline = schema_from_plans(&applied_plans)
            .map_err(|e| anyhow::anyhow!("schema reconstruction error: {}", e))?;

        let baseline_tables: HashSet<_> = baseline.iter().map(|t| &t.name).collect();
        let current_tables: HashSet<_> = current_models.iter().map(|t| &t.name).collect();

        if baseline_tables == current_tables {
            println!(
                "{} {}",
                "Status:".bright_cyan().bold(),
                "Schema is synchronized with migrations.".bright_green()
            );
        } else {
            println!(
                "{} {}",
                "Status:".bright_cyan().bold(),
                "Schema differs from applied migrations.".bright_yellow()
            );
            println!(
                "  {} {} {}",
                "Run".bright_white(),
                "'vespertide diff'".bright_cyan().bold(),
                "to see details.".bright_white()
            );
        }
    } else if current_models.is_empty() {
        println!(
            "{} {}",
            "Status:".bright_cyan().bold(),
            "No models or migrations found.".bright_yellow()
        );
    } else {
        println!(
            "{} {}",
            "Status:".bright_cyan().bold(),
            "Models exist but no migrations have been applied.".bright_yellow()
        );
        println!(
            "  {} {} {}",
            "Run".bright_white(),
            "'vespertide revision -m \"initial\"'".bright_cyan().bold(),
            "to create the first migration.".bright_white()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::{fs, path::PathBuf};
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableConstraint,
        TableDef,
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
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };
        let path = models_dir.join(format!("{name}.json"));
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
    }

    fn write_migration(cfg: &VespertideConfig) {
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let plan = MigrationPlan {
            comment: Some("init".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }],
                constraints: vec![],
            }],
        };
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    }

    #[test]
    #[serial]
    fn cmd_status_with_matching_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");
        write_migration(&cfg);

        cmd_status().unwrap();
    }

    #[test]
    #[serial]
    fn cmd_status_no_models_no_migrations_prints_message() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let cfg = write_config();
        fs::create_dir_all(cfg.models_dir()).unwrap(); // empty models dir
        fs::create_dir_all(cfg.migrations_dir()).unwrap(); // empty migrations dir

        cmd_status().unwrap();
    }

    #[test]
    #[serial]
    fn cmd_status_models_no_migrations_prints_hint() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let cfg = write_config();
        write_model("users");
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        cmd_status().unwrap();
    }

    #[test]
    #[serial]
    fn cmd_status_differs_prints_diff_hint() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");
        // add another model to differ from baseline
        write_model("posts");
        write_migration(&cfg); // baseline only has users

        cmd_status().unwrap();
    }

    #[test]
    #[serial]
    fn cmd_status_model_with_description() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        fs::create_dir_all(cfg.models_dir()).unwrap();
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        // Create a model with a description to cover lines 102-105
        let table = TableDef {
            name: "users".to_string(),
            description: Some("User accounts table".to_string()),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };
        let path = cfg.models_dir().join("users.json");
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();

        cmd_status().unwrap();
    }
}
