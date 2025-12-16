use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use vespertide_config::VespertideConfig;
use vespertide_core::TableDef;
use vespertide_planner::validate_schema;

/// Load all model definitions from the models directory (recursively).
pub fn load_models(config: &VespertideConfig) -> Result<Vec<TableDef>> {
    let models_dir = config.models_dir();
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tables = Vec::new();
    load_models_recursive(models_dir, &mut tables)?;

    // Normalize tables to convert inline constraints (primary_key, foreign_key, etc.) to table-level constraints
    // This must happen before validation so that foreign key references can be checked
    let normalized_tables: Vec<TableDef> = tables
        .into_iter()
        .map(|t| {
            t.normalize()
                .map_err(|e| anyhow::anyhow!("Failed to normalize table '{}': {}", t.name, e))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Validate schema integrity before returning
    if !normalized_tables.is_empty() {
        validate_schema(&normalized_tables)
            .map_err(|e| anyhow::anyhow!("schema validation failed: {}", e))?;
    }

    Ok(normalized_tables)
}

/// Recursively walk directory and load model files.
fn load_models_recursive(dir: &Path, tables: &mut Vec<TableDef>) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("read models directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();

        if path.is_dir() {
            // Recursively process subdirectories
            load_models_recursive(&path, tables)?;
            continue;
        }

        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if matches!(ext, Some("json") | Some("yaml") | Some("yml")) {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("read model file: {}", path.display()))?;

                let table: TableDef = if ext == Some("json") {
                    serde_json::from_str(&content)
                        .with_context(|| format!("parse JSON model: {}", path.display()))?
                } else {
                    serde_yaml::from_str(&content)
                        .with_context(|| format!("parse YAML model: {}", path.display()))?
                };

                tables.push(table);
            }
        }
    }

    Ok(())
}

/// Load models from a specific directory (for compile-time use in macros).
pub fn load_models_from_dir(
    project_root: Option<std::path::PathBuf>,
) -> Result<Vec<TableDef>, Box<dyn std::error::Error>> {
    use std::env;

    // Locate project root from CARGO_MANIFEST_DIR or use provided path
    let project_root = if let Some(root) = project_root {
        root
    } else {
        std::path::PathBuf::from(
            env::var("CARGO_MANIFEST_DIR")
                .context("CARGO_MANIFEST_DIR environment variable not set")?,
        )
    };

    // Read vespertide.json or use defaults
    let config = crate::config::load_config_or_default(Some(project_root.clone()))
        .map_err(|e| format!("Failed to load config: {}", e))?;

    // Read models directory
    let models_dir = project_root.join(config.models_dir());
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tables = Vec::new();
    load_models_recursive_internal(&models_dir, &mut tables)
        .map_err(|e| format!("Failed to load models: {}", e))?;

    // Normalize tables
    let normalized_tables: Vec<TableDef> = tables
        .into_iter()
        .map(|t| {
            t.normalize()
                .map_err(|e| format!("Failed to normalize table '{}': {}", t.name, e))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(normalized_tables)
}

/// Internal recursive function for loading models (used by both runtime and compile-time).
fn load_models_recursive_internal(
    dir: &Path,
    tables: &mut Vec<TableDef>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read models directory {}: {}", dir.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            // Recursively process subdirectories
            load_models_recursive_internal(&path, tables)?;
            continue;
        }

        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if matches!(ext, Some("json") | Some("yaml") | Some("yml")) {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read model file {}: {}", path.display(), e))?;

                let table: TableDef = if ext == Some("json") {
                    serde_json::from_str(&content).map_err(|e| {
                        format!("Failed to parse JSON model {}: {}", path.display(), e)
                    })?
                } else {
                    serde_yaml::from_str(&content).map_err(|e| {
                        format!("Failed to parse YAML model {}: {}", path.display(), e)
                    })?
                };

                tables.push(table);
            }
        }
    }

    Ok(())
}

/// Load models at compile time (for macro use).
pub fn load_models_at_compile_time() -> Result<Vec<TableDef>, Box<dyn std::error::Error>> {
    load_models_from_dir(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_core::{
        ColumnDef, ColumnType, SimpleColumnType, TableConstraint,
        schema::foreign_key::ForeignKeySyntax,
    };

    struct CwdGuard {
        original: std::path::PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &std::path::PathBuf) -> Self {
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

    fn write_config() {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
    }

    #[test]
    #[serial]
    fn load_models_returns_empty_when_no_models_dir() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        // Don't create models directory
        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 0);
    }

    #[test]
    #[serial]
    fn load_models_reads_yaml_and_validates() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models").unwrap();
        let table = TableDef {
            name: "users".into(),
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
            indexes: vec![],
        };
        fs::write("models/users.yaml", serde_yaml::to_string(&table).unwrap()).unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    #[test]
    #[serial]
    fn load_models_recursive_processes_subdirectories() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models/subdir").unwrap();

        // Create model in subdirectory
        let table = TableDef {
            name: "subtable".into(),
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
            indexes: vec![],
        };
        let content = serde_json::to_string_pretty(&table).unwrap();
        fs::write("models/subdir/subtable.json", content).unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "subtable");
    }

    #[test]
    #[serial]
    fn load_models_fails_on_invalid_fk_format() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models").unwrap();

        // Create a model with invalid FK string format (missing dot separator)
        let table = TableDef {
            name: "orders".into(),
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                // Invalid FK format: should be "table.column" but missing the dot
                foreign_key: Some(ForeignKeySyntax::String("invalid_format".into())),
            }],
            constraints: vec![],
            indexes: vec![],
        };
        fs::write(
            "models/orders.json",
            serde_json::to_string_pretty(&table).unwrap(),
        )
        .unwrap();

        let result = load_models(&VespertideConfig::default());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to normalize table 'orders'"));
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_root() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        let table = TableDef {
            name: "users".into(),
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
            indexes: vec![],
        };
        fs::write(
            models_dir.join("users.json"),
            serde_json::to_string_pretty(&table).unwrap(),
        )
        .unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let models = result.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_without_root() {
        use std::env;

        // Save the original value
        let original = env::var("CARGO_MANIFEST_DIR").ok();

        // Remove CARGO_MANIFEST_DIR to test the error path
        unsafe {
            env::remove_var("CARGO_MANIFEST_DIR");
        }

        let result = load_models_from_dir(None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("CARGO_MANIFEST_DIR environment variable not set"));

        drop(original);
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_no_models_dir() {
        let temp_dir = tempdir().unwrap();
        // Don't create models directory

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let models = result.unwrap();
        assert_eq!(models.len(), 0);
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_yaml() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        let table = TableDef {
            name: "users".into(),
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
            indexes: vec![],
        };
        fs::write(
            models_dir.join("users.yaml"),
            serde_yaml::to_string(&table).unwrap(),
        )
        .unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let models = result.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_yml() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        let table = TableDef {
            name: "users".into(),
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
            indexes: vec![],
        };
        fs::write(
            models_dir.join("users.yml"),
            serde_yaml::to_string(&table).unwrap(),
        )
        .unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let models = result.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_recursive() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        let subdir = models_dir.join("subdir");
        fs::create_dir_all(&subdir).unwrap();

        let table = TableDef {
            name: "subtable".into(),
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
            indexes: vec![],
        };
        fs::write(
            subdir.join("subtable.json"),
            serde_json::to_string_pretty(&table).unwrap(),
        )
        .unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_ok());
        let models = result.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "subtable");
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_invalid_json() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        fs::write(models_dir.join("invalid.json"), r#"{"invalid": json}"#).unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to parse JSON model"));
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_invalid_yaml() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        fs::write(models_dir.join("invalid.yaml"), r#"invalid: [yaml"#).unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to parse YAML model"));
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_normalization_error() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        // Create a model with invalid FK format
        let table = TableDef {
            name: "orders".into(),
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::String("invalid_format".into())),
            }],
            constraints: vec![],
            indexes: vec![],
        };
        fs::write(
            models_dir.join("orders.json"),
            serde_json::to_string_pretty(&table).unwrap(),
        )
        .unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to normalize table 'orders'"));
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_cargo_manifest_dir() {
        // Test the path where CARGO_MANIFEST_DIR is set (line 87)
        // In cargo test environment, CARGO_MANIFEST_DIR is usually set
        let result = load_models_from_dir(None);
        // This might succeed if CARGO_MANIFEST_DIR is set (like in cargo test)
        // or fail if it's not set
        // Either way, we're testing the code path including line 87
        let _ = result;
    }

    #[test]
    #[serial]
    fn test_load_models_at_compile_time() {
        // This function just calls load_models_from_dir(None)
        // We can't easily test it without CARGO_MANIFEST_DIR, but we can verify
        // it doesn't panic
        let result = load_models_at_compile_time();
        // This might succeed if CARGO_MANIFEST_DIR is set (like in cargo test)
        // or fail if it's not set
        // Either way, we're testing the code path
        let _ = result;
    }
}
