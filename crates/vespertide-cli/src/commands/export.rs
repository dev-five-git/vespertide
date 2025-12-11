use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::ValueEnum;
use vespertide_config::VespertideConfig;
use vespertide_core::TableDef;
use vespertide_exporter::{Orm, render_entity};

use crate::utils::load_config;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum OrmArg {
    Seaorm,
    Sqlalchemy,
    Sqlmodel,
}

impl From<OrmArg> for Orm {
    fn from(value: OrmArg) -> Self {
        match value {
            OrmArg::Seaorm => Orm::SeaOrm,
            OrmArg::Sqlalchemy => Orm::SqlAlchemy,
            OrmArg::Sqlmodel => Orm::SqlModel,
        }
    }
}

pub fn cmd_export(orm: OrmArg, export_dir: Option<PathBuf>) -> Result<()> {
    let config = load_config()?;
    let models = load_models_recursive(config.models_dir()).context("load models recursively")?;

    let target_root = resolve_export_dir(export_dir, &config);
    if !target_root.exists() {
        fs::create_dir_all(&target_root)
            .with_context(|| format!("create export dir {}", target_root.display()))?;
    }

    let orm_kind: Orm = orm.into();

    for (table, rel_path) in &models {
        let code = render_entity(orm_kind, table).map_err(|e| anyhow::anyhow!(e))?;
        let out_path = build_output_path(&target_root, rel_path, orm_kind);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent dir {}", parent.display()))?;
        }
        fs::write(&out_path, code).with_context(|| format!("write {}", out_path.display()))?;
        println!("Exported {} -> {}", table.name, out_path.display());
    }

    Ok(())
}

fn resolve_export_dir(export_dir: Option<PathBuf>, config: &VespertideConfig) -> PathBuf {
    if let Some(dir) = export_dir {
        return dir;
    }
    // Prefer explicit model_export_dir from config, fallback to default inside config.
    config.model_export_dir().to_path_buf()
}

fn build_output_path(root: &Path, rel_path: &Path, orm: Orm) -> PathBuf {
    let mut out = root.join(rel_path);
    // swap extension based on ORM
    let ext = match orm {
        Orm::SeaOrm => "rs",
        Orm::SqlAlchemy | Orm::SqlModel => "py",
    };
    out.set_extension(ext);
    out
}

fn load_models_recursive(base: &Path) -> Result<Vec<(TableDef, PathBuf)>> {
    let mut out = Vec::new();
    if !base.exists() {
        return Ok(out);
    }
    walk_models(base, base, &mut out)?;
    Ok(out)
}

fn walk_models(root: &Path, current: &Path, acc: &mut Vec<(TableDef, PathBuf)>) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| format!("read {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_models(root, &path, acc)?;
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str());
        if !matches!(ext, Some("json") | Some("yaml") | Some("yml")) {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read model file: {}", path.display()))?;
        let table: TableDef = if ext == Some("json") {
            serde_json::from_str(&content)
                .with_context(|| format!("parse JSON model: {}", path.display()))?
        } else {
            serde_yaml::from_str(&content)
                .with_context(|| format!("parse YAML model: {}", path.display()))?
        };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        acc.push((table, rel));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_core::{ColumnDef, ColumnType, TableConstraint};

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

    fn write_config() {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
    }

    fn write_model(path: &Path, table: &TableDef) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, serde_json::to_string_pretty(table).unwrap()).unwrap();
    }

    fn sample_table(name: &str) -> TableDef {
        TableDef {
            name: name.to_string(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Integer,
                nullable: false,
                default: None,
            }],
            constraints: vec![TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
            }],
            indexes: vec![],
        }
    }

    #[test]
    #[serial]
    fn export_writes_seaorm_files_to_default_dir() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("users");
        write_model(Path::new("models/users.json"), &model);

        cmd_export(OrmArg::Seaorm, None).unwrap();

        let out = PathBuf::from("src/models/users.rs");
        assert!(out.exists());
        let content = fs::read_to_string(out).unwrap();
        assert!(content.contains("#[sea_orm(table_name = \"users\")]"));
    }

    #[test]
    #[serial]
    fn export_respects_custom_output_dir() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("posts");
        write_model(Path::new("models/blog/posts.json"), &model);

        let custom = PathBuf::from("out_dir");
        cmd_export(OrmArg::Seaorm, Some(custom.clone())).unwrap();

        let out = custom.join("blog/posts.rs");
        assert!(out.exists());
        let content = fs::read_to_string(out).unwrap();
        assert!(content.contains("#[sea_orm(table_name = \"posts\")]"));
    }

    #[test]
    #[serial]
    fn export_with_sqlalchemy_sets_py_extension() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("items");
        write_model(Path::new("models/items.json"), &model);

        cmd_export(OrmArg::Sqlalchemy, None).unwrap();

        let out = PathBuf::from("src/models/items.py");
        assert!(out.exists());
        let content = fs::read_to_string(out).unwrap();
        assert!(content.contains("items"));
    }

    #[test]
    #[serial]
    fn export_with_sqlmodel_sets_py_extension() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("orders");
        write_model(Path::new("models/orders.json"), &model);

        cmd_export(OrmArg::Sqlmodel, None).unwrap();

        let out = PathBuf::from("src/models/orders.py");
        assert!(out.exists());
        let content = fs::read_to_string(out).unwrap();
        assert!(content.contains("orders"));
    }

    #[test]
    #[serial]
    fn load_models_recursive_returns_empty_when_absent() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let models = load_models_recursive(Path::new("no_models")).unwrap();
        assert!(models.is_empty());
    }

    #[test]
    #[serial]
    fn load_models_recursive_ignores_non_model_files() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models").unwrap();
        fs::write("models/ignore.txt", "hello").unwrap();
        write_model(Path::new("models/valid.json"), &sample_table("valid"));

        let models = load_models_recursive(Path::new("models")).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].0.name, "valid");
    }

    #[test]
    #[serial]
    fn load_models_recursive_parses_yaml_branch() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models").unwrap();
        let table = sample_table("yaml_table");
        let yaml = serde_yaml::to_string(&table).unwrap();
        fs::write("models/yaml_table.yaml", yaml).unwrap();

        let models = load_models_recursive(Path::new("models")).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].0.name, "yaml_table");
    }

    #[test]
    #[serial]
    fn resolve_export_dir_prefers_override() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();
        let cfg = VespertideConfig::default();
        let override_dir = PathBuf::from("custom_out");
        let resolved = super::resolve_export_dir(Some(override_dir.clone()), &cfg);
        assert_eq!(resolved, override_dir);
    }

    #[test]
    fn orm_arg_maps_to_enum() {
        assert!(matches!(Orm::from(OrmArg::Seaorm), Orm::SeaOrm));
        assert!(matches!(Orm::from(OrmArg::Sqlalchemy), Orm::SqlAlchemy));
        assert!(matches!(Orm::from(OrmArg::Sqlmodel), Orm::SqlModel));
    }
}
