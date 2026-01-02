use std::fs;

use anyhow::{Context, Result, bail};
use colored::Colorize;
use serde_json::Value;
use vespertide_core::TableDef;

use crate::utils::load_config;
use vespertide_config::FileFormat;

pub fn cmd_new(name: String, format: Option<FileFormat>) -> Result<()> {
    let config = load_config()?;
    let format = format.unwrap_or_else(|| config.model_format());
    let dir = config.models_dir();
    if !dir.exists() {
        fs::create_dir_all(dir).context("create models directory")?;
    }

    let ext = match format {
        FileFormat::Json => "json",
        FileFormat::Yaml => "yaml",
        FileFormat::Yml => "yml",
    };

    let schema_url = schema_url_for(format);
    let path = dir.join(format!("{name}.{ext}"));
    if path.exists() {
        bail!("model file already exists: {}", path.display());
    }

    let table = TableDef {
        name: name.clone(),
        description: None,
        columns: Vec::new(),
        constraints: Vec::new(),
    };

    match format {
        FileFormat::Json => write_json_with_schema(&path, &table, &schema_url)?,
        FileFormat::Yaml | FileFormat::Yml => write_yaml(&path, &table, &schema_url)?,
    }

    println!(
        "{} {}",
        "Created model template:".bright_green().bold(),
        format!("{}", path.display()).bright_white()
    );
    Ok(())
}

fn schema_url_for(format: FileFormat) -> String {
    // If not set, default to public raw GitHub schema location.
    // Users can override via VESP_SCHEMA_BASE_URL.
    let base = std::env::var("VESP_SCHEMA_BASE_URL").ok();
    let base = base.as_deref().unwrap_or(
        "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas",
    );
    let base = base.trim_end_matches('/');
    match format {
        FileFormat::Json => format!("{}/model.schema.json", base),
        FileFormat::Yaml | FileFormat::Yml => format!("{}/model.schema.json", base),
    }
}

fn write_json_with_schema(
    path: &std::path::Path,
    table: &TableDef,
    schema_url: &str,
) -> Result<()> {
    let mut value = serde_json::to_value(table).context("serialize table to json")?;
    if let Value::Object(ref mut map) = value {
        map.insert("$schema".to_string(), Value::String(schema_url.to_string()));
    }
    let text = serde_json::to_string_pretty(&value).context("stringify json with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

fn write_yaml(path: &std::path::Path, table: &TableDef, schema_url: &str) -> Result<()> {
    let mut value = serde_yaml::to_value(table).context("serialize table to yaml value")?;
    if let serde_yaml::Value::Mapping(ref mut map) = value {
        map.insert(
            serde_yaml::Value::String("$schema".to_string()),
            serde_yaml::Value::String(schema_url.to_string()),
        );
    }
    let text = serde_yaml::to_string(&value).context("serialize yaml with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;

    struct CwdGuard {
        original: std::path::PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &std::path::Path) -> Self {
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

    fn write_config(model_format: FileFormat) {
        let cfg = VespertideConfig {
            model_format,
            ..VespertideConfig::default()
        };
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        std::fs::write("vespertide.json", text).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn cmd_new_creates_json_with_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        let expected_schema = schema_url_for(FileFormat::Json);
        write_config(FileFormat::Json);

        cmd_new("users".into(), None).unwrap();

        let cfg = VespertideConfig::default();
        let path = cfg.models_dir().join("users.json");
        assert!(path.exists());

        let text = fs::read_to_string(path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value.get("$schema"),
            Some(&serde_json::Value::String(expected_schema))
        );
    }

    #[test]
    #[serial_test::serial]
    fn cmd_new_creates_yaml_with_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        let expected_schema = schema_url_for(FileFormat::Yaml);
        write_config(FileFormat::Yaml);

        cmd_new("orders".into(), None).unwrap();

        let cfg = VespertideConfig {
            model_format: FileFormat::Yaml,
            ..VespertideConfig::default()
        };
        let path = cfg.models_dir().join("orders.yaml");
        assert!(path.exists());

        let text = fs::read_to_string(path).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        let schema = value
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("$schema".into())))
            .and_then(|v| v.as_str());
        assert_eq!(schema, Some(expected_schema.as_str()));
    }

    #[test]
    #[serial_test::serial]
    fn cmd_new_creates_yml_with_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        let expected_schema = schema_url_for(FileFormat::Yml);
        write_config(FileFormat::Yml);

        cmd_new("products".into(), None).unwrap();

        let cfg = VespertideConfig {
            model_format: FileFormat::Yml,
            ..VespertideConfig::default()
        };
        let path = cfg.models_dir().join("products.yml");
        assert!(path.exists());

        let text = fs::read_to_string(path).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        let schema = value
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("$schema".into())))
            .and_then(|v| v.as_str());
        assert_eq!(schema, Some(expected_schema.as_str()));
    }

    #[test]
    #[serial_test::serial]
    fn cmd_new_fails_if_model_file_exists() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_config(FileFormat::Json);

        let cfg = VespertideConfig::default();
        std::fs::create_dir_all(cfg.models_dir()).unwrap();
        let path = cfg.models_dir().join("users.json");
        std::fs::write(&path, "{}").unwrap();

        let err = cmd_new("users".into(), None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("model file already exists"));
        assert!(msg.contains("users.json"));
    }
}
