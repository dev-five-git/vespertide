use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use vespertide_config::VespertideConfig;
use vespertide_core::TableDef;
use vespertide_planner::validate_schema;

use crate::parallel_config::map_paths_with_threshold;

/// Load all model definitions from the models directory (recursively).
pub fn load_models(config: &VespertideConfig) -> Result<Vec<TableDef>> {
    let models_dir = config.models_dir();
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tables = Vec::new();
    load_models_recursive(models_dir, &mut tables)?;

    // Validate schema integrity using normalized version
    // But return the original tables to preserve inline constraints
    if !tables.is_empty() {
        let normalized_tables: Vec<TableDef> = tables
            .iter()
            .map(|t| {
                t.normalize()
                    .map_err(|e| anyhow::anyhow!("Failed to normalize table '{}': {}", t.name, e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        validate_schema(&normalized_tables)
            .map_err(|e| anyhow::anyhow!("schema validation failed: {e}"))?;
    }

    Ok(tables)
}

/// Recursively walk directory and load model files.
fn load_models_recursive(dir: &Path, tables: &mut Vec<TableDef>) -> Result<()> {
    let paths = collect_model_paths(dir)?;
    let results = map_paths_with_threshold(&paths, load_model_file);

    for result in results {
        tables.push(result?);
    }

    Ok(())
}

fn collect_model_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("read models directory: {}", dir.display()))?;
    let mut paths = Vec::new();

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();

        // `DirEntry::file_type()` answers dir/file from the single `readdir`
        // result on most platforms (no extra stat). It does NOT follow
        // symlinks, whereas `Path::is_dir`/`is_file` DO — so for symlinked
        // entries we fall back to the path checks to keep behavior identical.
        let ft = entry.file_type().ok();
        let is_symlink = ft.is_some_and(|t| t.is_symlink());

        if ft.is_some_and(|t| t.is_dir()) || (is_symlink && path.is_dir()) {
            paths.extend(collect_model_paths(&path)?);
        } else if (ft.is_some_and(|t| t.is_file()) || (is_symlink && path.is_file()))
            && crate::has_supported_extension(&path)
        {
            paths.push(path);
        }
    }

    Ok(paths)
}

fn load_model_file(path: &Path) -> Result<TableDef> {
    let ext = path.extension().and_then(|s| s.to_str());
    let content =
        fs::read_to_string(path).with_context(|| format!("read model file: {}", path.display()))?;

    let table: TableDef = if ext == Some("json") {
        serde_json::from_str(&content)
            .with_context(|| format!("parse JSON model: {}", path.display()))?
    } else {
        serde_yaml::from_str(&content)
            .with_context(|| format!("parse YAML model: {}", path.display()))?
    };

    table
        .validate_unique_column_names()
        .with_context(|| format!("validate model: {}", path.display()))?;

    Ok(table)
}

/// Load models from a specific directory (for compile-time use in macros).
pub fn load_models_from_dir(
    project_root: Option<PathBuf>,
) -> Result<Vec<TableDef>, Box<dyn std::error::Error>> {
    // Locate project root from CARGO_MANIFEST_DIR or use provided path
    let project_root = if let Some(root) = project_root {
        root
    } else {
        PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR")
                .context("CARGO_MANIFEST_DIR environment variable not set")?,
        )
    };

    // Read vespertide.json or use defaults
    let config = crate::config::load_config_or_default(Some(project_root.clone()))
        .map_err(|e| format!("Failed to load config: {e}"))?;

    // Read models directory
    let models_dir = project_root.join(config.models_dir());
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tables = Vec::new();
    load_models_recursive_internal(&models_dir, &mut tables)
        .map_err(|e| format!("Failed to load models: {e}"))?;

    Ok(tables)
}

/// Internal recursive function for loading models (used by both runtime and compile-time).
fn load_models_recursive_internal(
    dir: &Path,
    tables: &mut Vec<TableDef>,
) -> Result<(), Box<dyn std::error::Error>> {
    let paths = collect_model_paths(dir).map_err(|e| e.to_string())?;
    let results = map_paths_with_threshold(&paths, load_normalized_model_file_internal);

    for result in results {
        tables.push(result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?);
    }

    Ok(())
}

/// Compile-time variant of [`load_model_file`]: same read → parse → validate
/// pipeline (delegated so extension dispatch and validation live in exactly
/// one place), followed by normalization — the macro path needs inline
/// constraints resolved to table level.
fn load_normalized_model_file_internal(path: &Path) -> Result<TableDef, String> {
    // anyhow's alternate format renders the context chain as
    // "parse JSON model: <path>: <source>"; prefixing "Failed to " keeps the
    // messages this pipeline has always produced.
    let table = load_model_file(path).map_err(|e| format!("Failed to {e:#}"))?;
    table
        .normalize()
        .map_err(|e| format!("Failed to normalize table '{}': {}", table.name, e))
}

/// Load models at compile time (for macro use).
pub fn load_models_at_compile_time() -> Result<Vec<TableDef>, Box<dyn std::error::Error>> {
    load_models_from_dir(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CwdGuard, write_default_config};
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_core::{
        ColumnDef, ColumnType, SimpleColumnType, TableConstraint,
        schema::foreign_key::ForeignKeySyntax,
    };

    #[test]
    #[serial]
    fn load_models_returns_empty_when_no_models_dir() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_default_config("vespertide.json");

        // Don't create models directory
        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 0);
    }

    #[test]
    #[serial]
    fn load_models_reads_yaml_and_validates() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_default_config("vespertide.json");

        fs::create_dir_all("models").unwrap();
        let table = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        };
        fs::write("models/users.yaml", serde_yaml::to_string(&table).unwrap()).unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    // `DirEntry::file_type()` describes the LINK, never its target, so symlinked
    // entries fall back to `Path::is_dir` / `Path::is_file`. Three shapes must be
    // told apart inside `models/`:
    //   * `order.json`   -> a real file OUTSIDE models/  => loaded
    //   * `nowhere.json` -> a missing target (dangling)  => skipped
    //   * `user.json`    -> a plain file                 => loaded
    // Misreading a file symlink as a directory would `read_dir` a file, and
    // misreading a dangling symlink as a file would `read_to_string` a path that
    // does not exist. Both surface as `Err`, so the successful two-table load
    // pins each `is_symlink && ...` conjunction.
    #[cfg(unix)]
    #[test]
    #[serial]
    fn load_models_follows_file_symlinks_and_skips_dangling_ones() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_default_config("vespertide.json");

        fs::create_dir_all("models").unwrap();
        fs::create_dir_all("external").unwrap();

        let write_model = |path: &str, name: &str| {
            let table = TableDef {
                name: name.into(),
                description: None,
                columns: vec![ColumnDef::new(
                    "id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                )],
                constraints: vec![TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                }],
            };
            fs::write(path, serde_json::to_string(&table).unwrap()).unwrap();
        };

        write_model("models/user.json", "users");
        write_model("external/order.json", "orders");

        symlink(tmp.path().join("external/order.json"), "models/order.json").unwrap();
        symlink(
            tmp.path().join("external/nowhere.json"),
            "models/nowhere.json",
        )
        .unwrap();

        let mut names: Vec<String> = load_models(&VespertideConfig::default())
            .unwrap()
            .into_iter()
            .map(|t| t.name.into_inner())
            .collect();
        names.sort();

        assert_eq!(names, ["orders", "users"]);
    }

    #[test]
    #[serial]
    fn load_models_recursive_processes_subdirectories() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_default_config("vespertide.json");

        fs::create_dir_all("models/subdir").unwrap();

        // Create model in subdirectory
        let table = TableDef {
            name: "subtable".into(),
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
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
        let _guard = CwdGuard::new(tmp.path());
        write_default_config("vespertide.json");

        fs::create_dir_all("models").unwrap();

        // Create a model with invalid FK string format (missing dot separator)
        let table = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                // Invalid FK format: should be "table.column" but missing the dot
                ColumnDef::new(
                    "user_id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                )
                .foreign_key(ForeignKeySyntax::String("invalid_format".into())),
            ],
            constraints: vec![],
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

    // A non-model-extension file (e.g. `.txt`) must be ignored by the
    // collector. Pins `path.is_file() && has_model_extension(&path)`: a
    // `&& -> ||` mutant would pick up the `.txt`, then fail to parse it as a
    // model and surface an error instead of an empty load.
    #[test]
    #[serial]
    fn load_models_ignores_non_model_extension_files() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_default_config("vespertide.json");
        fs::create_dir_all("models").unwrap();
        fs::write("models/README.txt", "not a model: {{{ invalid").unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 0, "the .txt file must be skipped");
    }

    #[test]
    #[serial]
    fn load_models_from_dir_ignores_non_model_extension_files() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(models_dir.join("notes.txt"), "not a model: {{{ invalid").unwrap();

        let result = load_models_from_dir(Some(temp_dir.path().to_path_buf()));
        assert!(
            result.is_ok(),
            "the .txt file must be skipped, not parsed: {result:?}"
        );
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    #[serial]
    fn test_load_models_from_dir_with_root() {
        let temp_dir = tempdir().unwrap();
        let models_dir = temp_dir.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();

        let table = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![],
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

        // Restore the original value if it existed
        if let Some(val) = original {
            unsafe {
                env::set_var("CARGO_MANIFEST_DIR", val);
            }
        }
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
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![],
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
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![],
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
            description: None,
            columns: vec![ColumnDef::new(
                "id",
                ColumnType::Simple(SimpleColumnType::Integer),
                false,
            )],
            constraints: vec![],
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

        fs::write(models_dir.join("invalid.yaml"), r"invalid: [yaml").unwrap();

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
            description: None,
            columns: vec![
                ColumnDef::new(
                    "user_id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                )
                .foreign_key(ForeignKeySyntax::String("invalid_format".into())),
            ],
            constraints: vec![],
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
