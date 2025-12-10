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
        fs::create_dir_all(&migrations_dir).context("create migrations directory")?;
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

    fs::write(&path, text).with_context(|| format!("write migration file: {}", path.display()))?;

    println!("Created migration: {}", path.display());
    println!("  Version: {}", plan.version);
    println!("  Actions: {}", plan.actions.len());
    if let Some(comment) = &plan.comment {
        println!("  Comment: {}", comment);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, path::PathBuf};
    use tempfile::tempdir;
    use vespertide_config::{FileFormat, VespertideConfig};
    use vespertide_core::{ColumnDef, ColumnType, TableDef};

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

    fn write_config() -> VespertideConfig {
        write_config_with_format(None)
    }

    fn write_config_with_format(fmt: Option<FileFormat>) -> VespertideConfig {
        let mut cfg = VespertideConfig::default();
        if let Some(f) = fmt {
            cfg.migration_format = f;
        }
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
    #[serial_test::serial]
    fn cmd_revision_writes_migration() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        cmd_revision("init".into()).unwrap();

        let entries: Vec<_> = fs::read_dir(cfg.migrations_dir()).unwrap().collect();
        assert!(!entries.is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn cmd_revision_no_changes_short_circuits() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        // no models, no migrations -> plan with no actions -> early return
        assert!(cmd_revision("noop".into()).is_ok());
        // migrations dir should not be created
        assert!(!cfg.migrations_dir().exists());
    }

    #[test]
    #[serial_test::serial]
    fn cmd_revision_writes_yaml_when_configured() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config_with_format(Some(FileFormat::Yaml));
        write_model("users");
        // ensure migrations dir absent to exercise create_dir_all branch
        if cfg.migrations_dir().exists() {
            fs::remove_dir_all(cfg.migrations_dir()).unwrap();
        }

        cmd_revision("yaml".into()).unwrap();

        let entries: Vec<_> = fs::read_dir(cfg.migrations_dir()).unwrap().collect();
        assert!(!entries.is_empty());
        let has_yaml = entries.iter().any(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .map(|s| s == "yaml")
                .unwrap_or(false)
        });
        assert!(has_yaml);
    }
}
