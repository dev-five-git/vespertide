use std::env;
use std::fs;
use std::path::PathBuf;
use vespertide_config::VespertideConfig;
use vespertide_core::MigrationPlan;

pub fn load_migrations_at_compile_time() -> Result<Vec<MigrationPlan>, Box<dyn std::error::Error>> {
    load_migrations_from_dir(None)
}

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

    // Read vespertide.json
    let config_path = project_root.join("vespertide.json");
    let config: VespertideConfig = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        serde_json::from_str(&content)?
    } else {
        // Fall back to defaults if config is missing
        VespertideConfig::default()
    };

    // Read migrations directory
    let migrations_dir = project_root.join(config.migrations_dir());
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = fs::read_dir(&migrations_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path)?;

                let plan: MigrationPlan = if ext == Some("json") {
                    serde_json::from_str(&content)?
                } else {
                    serde_yaml::from_str(&content)?
                };

                plans.push(plan);
            }
        }
    }

    // Sort by version
    plans.sort_by_key(|p| p.version);
    Ok(plans)
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
    fn test_load_migrations_from_dir_with_yaml_migration() {
        let temp_dir = TempDir::new().unwrap();
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();

        let migration_content = r#"version: 1
actions:
  - type: create_table
    table: users
    columns:
      - name: id
        type: integer
        nullable: false
    constraints: []
"#;

        fs::write(migrations_dir.join("0001_test.yaml"), migration_content).unwrap();

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

    #[test]
    fn test_load_migrations_from_dir_with_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let migrations_dir = temp_dir.path().join("custom_migrations");
        fs::create_dir_all(&migrations_dir).unwrap();

        let config_content = r#"{
            "modelsDir": "models",
            "migrationsDir": "custom_migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake"
        }"#;
        fs::write(temp_dir.path().join("vespertide.json"), config_content).unwrap();

        let migration_content = r#"{"version": 1, "actions": []}"#;
        fs::write(migrations_dir.join("0001_test.json"), migration_content).unwrap();

        let result = load_migrations_from_dir(Some(temp_dir.path().to_path_buf()));
        match result {
            Ok(plans) => {
                assert_eq!(plans.len(), 1);
                assert_eq!(plans[0].version, 1);
            }
            Err(e) => panic!("Failed to load migrations: {}", e),
        }
    }

    #[test]
    fn test_load_migrations_from_dir_ignores_non_migration_files() {
        let temp_dir = TempDir::new().unwrap();
        let migrations_dir = temp_dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();

        let migration_content = r#"{"version": 1, "actions": []}"#;
        fs::write(migrations_dir.join("0001_test.json"), migration_content).unwrap();
        fs::write(migrations_dir.join("README.txt"), "not a migration").unwrap();
        fs::write(
            migrations_dir.join("0002_test.xml"),
            "<migration></migration>",
        )
        .unwrap();

        let result = load_migrations_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let plans = result.unwrap();
        assert_eq!(plans.len(), 1);
    }
}
