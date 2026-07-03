//! Deterministic synthetic workspace for profiling.
//!
//! Generates a tempdir with a Vespertide config, JSON model files, and a
//! partially-applied migration history so drift detection has real work to do.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const MODEL_SCHEMA: &str = "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json";
const MIGRATION_SCHEMA: &str = "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/migration.schema.json";

pub struct Scenario {
    pub _tmp: TempDir, // kept alive for the duration of the run
    pub root: PathBuf,
    pub model_uris: Vec<String>, // file:// URIs
}

pub fn build_workspace(n_tables: usize) -> Result<Scenario> {
    let tmp_dir = TempDir::new().context("create temporary LSP profile workspace")?;
    let root = tmp_dir.path().to_path_buf();
    let models_dir = root.join("models");
    let migrations_dir = root.join("migrations");

    fs::create_dir_all(&models_dir)
        .with_context(|| format!("create models dir: {}", models_dir.display()))?;
    fs::create_dir_all(&migrations_dir)
        .with_context(|| format!("create migrations dir: {}", migrations_dir.display()))?;

    write_json(
        &root.join("vespertide.json"),
        &json!({
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
        }),
    )?;

    let mut model_uris = Vec::with_capacity(n_tables);
    for table_number in 1..=n_tables {
        let model_path = models_dir.join(format!("table_{table_number:03}.json"));
        write_json(&model_path, &model_table(table_number))?;
        model_uris.push(path_to_file_uri(&model_path));
    }

    let actions: Vec<Value> = (1..=n_tables.min(80))
        .map(|table_number| create_table_action(table_number, table_number != 1))
        .collect();
    write_json(
        &migrations_dir.join("0001_init.vespertide.json"),
        &json!({
            "$schema": MIGRATION_SCHEMA,
            "version": 1,
            "id": "init",
            "comment": "init",
            "actions": actions,
        }),
    )?;

    Ok(Scenario {
        _tmp: tmp_dir,
        root,
        model_uris,
    })
}

fn model_table(table_number: usize) -> Value {
    let table_name = table_name(table_number);
    json!({
        "$schema": MODEL_SCHEMA,
        "name": table_name,
        "columns": columns(table_number, true),
    })
}

fn create_table_action(table_number: usize, include_tag: bool) -> Value {
    json!({
        "type": "create_table",
        "table": table_name(table_number),
        "columns": columns(table_number, include_tag),
        "constraints": [],
    })
}

fn columns(table_number: usize, include_tag: bool) -> Vec<Value> {
    let current_table_name = table_name(table_number);
    let mut columns = vec![
        json!({
            "name": "id",
            "type": "integer",
            "nullable": false,
            "primary_key": true,
        }),
        json!({
            "name": "created_at",
            "type": "timestamptz",
            "nullable": true,
            "default": "NOW()",
        }),
        json!({
            "name": "name",
            "type": "text",
            "nullable": true,
        }),
        json!({
            "name": "description",
            "type": "text",
            "nullable": true,
        }),
        json!({
            "name": "status",
            "type": {
                "kind": "enum",
                "name": format!("{current_table_name}_status"),
                "values": ["active", "inactive", "archived"],
            },
            "nullable": true,
        }),
        json!({
            "name": "priority",
            "type": "integer",
            "nullable": true,
            "default": 0,
        }),
    ];

    if include_tag {
        columns.push(json!({
            "name": "tag",
            "type": { "kind": "varchar", "length": 64 },
            "nullable": true,
        }));
    }

    if table_number > 1 {
        columns.push(json!({
            "name": "parent_id",
            "type": "integer",
            "nullable": true,
            "foreign_key": {
                "ref_table": table_name(table_number - 1),
                "ref_columns": ["id"],
                "on_delete": "set_null",
            },
        }));
    }

    columns
}

fn table_name(table_number: usize) -> String {
    format!("table_{table_number:03}")
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value).context("serialize fixture JSON")?;
    fs::write(path, format!("{text}\n"))
        .with_context(|| format!("write fixture JSON: {}", path.display()))
}

fn path_to_file_uri(path: &Path) -> String {
    let mut path_text = path.to_string_lossy().replace('\\', "/");
    if !path_text.starts_with('/') {
        path_text = format!("/{path_text}");
    }
    format!("file://{path_text}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;

    use super::*;

    #[test]
    fn build_workspace_writes_config_models_and_seed_migration() {
        let scenario = build_workspace(3).expect("workspace fixture should build");

        assert_eq!(scenario.model_uris.len(), 3);
        assert!(scenario.root.join("vespertide.json").exists());
        assert!(scenario.root.join("models/table_001.json").exists());
        assert!(scenario.root.join("models/table_002.json").exists());
        assert!(
            scenario
                .root
                .join("migrations/0001_init.vespertide.json")
                .exists()
        );

        let config_text = fs::read_to_string(scenario.root.join("vespertide.json"))
            .expect("config should be readable");
        let config: Value = serde_json::from_str(&config_text).expect("config should be JSON");
        assert_eq!(config["modelsDir"], "models");
        assert_eq!(config["migrationsDir"], "migrations");
        assert_eq!(config["tableNamingCase"], "snake");
        assert_eq!(config["columnNamingCase"], "snake");

        let table_002_text = fs::read_to_string(scenario.root.join("models/table_002.json"))
            .expect("table_002 should be readable");
        let table_002: Value = serde_json::from_str(&table_002_text).expect("model should be JSON");
        assert_eq!(table_002["name"], "table_002");
        assert_eq!(table_002["columns"][7]["name"], "parent_id");
        assert_eq!(
            table_002["columns"][7]["foreign_key"]["ref_table"],
            "table_001"
        );

        let migration_text =
            fs::read_to_string(scenario.root.join("migrations/0001_init.vespertide.json"))
                .expect("migration should be readable");
        let migration: Value =
            serde_json::from_str(&migration_text).expect("migration should be JSON");
        assert_eq!(migration["version"], 1);
        assert_eq!(
            migration["actions"]
                .as_array()
                .expect("actions array")
                .len(),
            3
        );
        let table_001_columns = migration["actions"][0]["columns"]
            .as_array()
            .expect("columns array");
        assert!(
            table_001_columns
                .iter()
                .all(|column| column["name"] != "tag"),
            "table_001 migration should omit tag to create add-column drift"
        );
    }
}
