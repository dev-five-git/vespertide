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
    println!(
        "  Migrations directory: {}",
        config.migrations_dir().display()
    );
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
        println!(
            "  - {} ({} columns, {} indexes)",
            model.name,
            model.columns.len(),
            model.indexes.len()
        );
    }
    println!();

    if !applied_plans.is_empty() {
        let baseline = schema_from_plans(&applied_plans)
            .map_err(|e| anyhow::anyhow!("schema reconstruction error: {}", e))?;

        let baseline_tables: HashSet<_> = baseline.iter().map(|t| &t.name).collect();
        let current_tables: HashSet<_> = current_models.iter().map(|t| &t.name).collect();

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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::{
        fs,
        path::PathBuf,
    };
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan, TableDef};

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
                    r#type: ColumnType::Integer,
                    nullable: false,
                    default: None,
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
}
