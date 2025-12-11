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
    let models = load_models_recursive(config.models_dir())
        .context("load models recursively")?;

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
