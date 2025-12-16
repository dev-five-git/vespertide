use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vespertide_config::VespertideConfig;
use vespertide_core::MigrationPlan;
use vespertide_planner::validate_migration_plan;

/// Load all migration plans from the migrations directory, sorted by version.
pub fn load_migrations(config: &VespertideConfig) -> Result<Vec<MigrationPlan>> {
    let migrations_dir = config.migrations_dir();
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = fs::read_dir(migrations_dir).context("read migrations directory")?;

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("read migration file: {}", path.display()))?;

                let plan: MigrationPlan = if ext == Some("json") {
                    serde_json::from_str(&content)
                        .with_context(|| format!("parse migration: {}", path.display()))?
                } else {
                    serde_yaml::from_str(&content)
                        .with_context(|| format!("parse migration: {}", path.display()))?
                };

                // Validate the migration plan
                validate_migration_plan(&plan)
                    .with_context(|| format!("validate migration: {}", path.display()))?;

                plans.push(plan);
            }
        }
    }

    // Sort by version number
    plans.sort_by_key(|p| p.version);
    Ok(plans)
}

/// Load migrations from a specific directory (for compile-time use in macros).
pub fn load_migrations_from_dir(
    project_root: Option<PathBuf>,
) -> Result<Vec<MigrationPlan>, Box<dyn std::error::Error>> {
    // Locate project root from CARGO_MANIFEST_DIR or use provided path
    let project_root = if let Some(root) = project_root {
        root
    } else {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR")
            .map_err(|_| "CARGO_MANIFEST_DIR environment variable not set")?;
        PathBuf::from(manifest_dir)
    };

    // Read vespertide.json or use defaults
    let config = crate::config::load_config_or_default(Some(project_root.clone()))
        .map_err(|e| format!("Failed to load config: {}", e))?;

    // Read migrations directory
    let migrations_dir = project_root.join(config.migrations_dir());
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = fs::read_dir(&migrations_dir)
        .map_err(|e| format!("Failed to read migrations directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path).map_err(|e| {
                    format!("Failed to read migration file {}: {}", path.display(), e)
                })?;

                let plan: MigrationPlan = if ext == Some("json") {
                    serde_json::from_str(&content).map_err(|e| {
                        format!("Failed to parse JSON migration {}: {}", path.display(), e)
                    })?
                } else {
                    serde_yaml::from_str(&content).map_err(|e| {
                        format!("Failed to parse YAML migration {}: {}", path.display(), e)
                    })?
                };

                plans.push(plan);
            }
        }
    }

    // Sort by version
    plans.sort_by_key(|p| p.version);
    Ok(plans)
}

/// Load migrations at compile time (for macro use).
pub fn load_migrations_at_compile_time() -> Result<Vec<MigrationPlan>, Box<dyn std::error::Error>> {
    load_migrations_from_dir(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_migrations_from_dir_with_no_migrations_dir() {
        let temp_dir = TempDir::new().unwrap();
        let result = load_migrations_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_load_migrations_from_dir_with_empty_migrations_dir() {
        let temp_dir = TempDir::new().unwrap();
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();

        let result = load_migrations_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_load_migrations_from_dir_with_json_migration() {
        let temp_dir = TempDir::new().unwrap();
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();

        let migration_content = r#"{
            "version": 1,
            "actions": [
                {
                    "type": "create_table",
                    "table": "users",
                    "columns": [
                        {
                            "name": "id",
                            "type": "integer",
                            "nullable": false
                        }
                    ],
                    "constraints": []
                }
            ]
        }"#;

        fs::write(migrations_dir.join("0001_test.json"), migration_content).unwrap();

        let result = load_migrations_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let plans = result.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].version, 1);
    }

    #[test]
    fn test_load_migrations_from_dir_sorts_by_version() {
        let temp_dir = TempDir::new().unwrap();
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();

        let migration1 = r#"{"version": 2, "actions": []}"#;
        let migration2 = r#"{"version": 1, "actions": []}"#;
        let migration3 = r#"{"version": 3, "actions": []}"#;

        fs::write(migrations_dir.join("0002_second.json"), migration1).unwrap();
        fs::write(migrations_dir.join("0001_first.json"), migration2).unwrap();
        fs::write(migrations_dir.join("0003_third.json"), migration3).unwrap();

        let result = load_migrations_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let plans = result.unwrap();
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].version, 1);
        assert_eq!(plans[1].version, 2);
        assert_eq!(plans[2].version, 3);
    }
}
