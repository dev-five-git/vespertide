use anyhow::{Context, Result, bail};
use colored::Colorize;
use tokio::fs;
use vespertide_core::TableDef;

use crate::utils::{load_config, schema_url, write_json_with_schema, write_yaml_with_schema};
use vespertide_config::FileFormat;

pub async fn cmd_new(name: String, format: Option<FileFormat>) -> Result<()> {
    let config = load_config()?;
    let format = format.unwrap_or_else(|| config.model_format());
    let dir = config.models_dir();
    if !dir.exists() {
        fs::create_dir_all(dir)
            .await
            .context("create models directory")?;
    }

    let schema_url = schema_url("model.schema.json");
    let path = dir.join(format!("{name}.vespertide.{ext}", ext = format.extension()));
    if path.exists() {
        bail!("model file already exists: {}", path.display());
    }

    let table = TableDef {
        name: name.into(),
        description: None,
        columns: Vec::new(),
        constraints: Vec::new(),
    };

    match format {
        FileFormat::Json => write_json_with_schema(&path, &table, &schema_url).await?,
        FileFormat::Yaml | FileFormat::Yml => {
            write_yaml_with_schema(&path, &table, &schema_url).await?;
        }
    }

    println!(
        "{} {}",
        "Created model template:".bright_green().bold(),
        path.display().to_string().bright_white()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::CwdGuard;
    use std::fs as std_fs;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;

    fn write_config(model_format: FileFormat) {
        let mut cfg = VespertideConfig::default();
        cfg.model_format = model_format;
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        std_fs::write("vespertide.json", text).unwrap();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_new_creates_json_with_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        let expected_schema = schema_url("model.schema.json");
        write_config(FileFormat::Json);

        cmd_new("users".into(), None).await.unwrap();

        let cfg = VespertideConfig::default();
        let path = cfg.models_dir().join("users.vespertide.json");
        assert!(path.exists());

        let text = std_fs::read_to_string(path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value.get("$schema"),
            Some(&serde_json::Value::String(expected_schema))
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_new_creates_yaml_with_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        let expected_schema = schema_url("model.schema.json");
        write_config(FileFormat::Yaml);

        cmd_new("orders".into(), None).await.unwrap();

        let mut cfg = VespertideConfig::default();
        cfg.model_format = FileFormat::Yaml;
        let path = cfg.models_dir().join("orders.vespertide.yaml");
        assert!(path.exists());

        let text = std_fs::read_to_string(path).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        let schema = value
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("$schema".into())))
            .and_then(|v| v.as_str());
        assert_eq!(schema, Some(expected_schema.as_str()));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_new_creates_yml_with_schema() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        let expected_schema = schema_url("model.schema.json");
        write_config(FileFormat::Yml);

        cmd_new("products".into(), None).await.unwrap();

        let mut cfg = VespertideConfig::default();
        cfg.model_format = FileFormat::Yml;
        let path = cfg.models_dir().join("products.vespertide.yml");
        assert!(path.exists());

        let text = std_fs::read_to_string(path).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        let schema = value
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("$schema".into())))
            .and_then(|v| v.as_str());
        assert_eq!(schema, Some(expected_schema.as_str()));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn cmd_new_fails_if_model_file_exists() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(tmp.path());
        write_config(FileFormat::Json);

        let cfg = VespertideConfig::default();
        std_fs::create_dir_all(cfg.models_dir()).unwrap();
        let path = cfg.models_dir().join("users.vespertide.json");
        std_fs::write(&path, "{}").unwrap();

        let err = cmd_new("users".into(), None).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("model file already exists"));
        assert!(msg.contains("users.vespertide.json"));
    }
}
