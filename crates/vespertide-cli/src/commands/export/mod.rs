use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures::future::try_join_all;
use rayon::prelude::*;
use tokio::fs;
use vespertide_config::VespertideConfig;
use vespertide_core::TableDef;
use vespertide_exporter::{
    Orm, django::DjangoExporterWithConfig, gorm::GormExporterWithConfig, prisma,
    python_naming::to_pascal_case, render_entity_with_schema, seaorm::SeaOrmExporterWithConfig,
};
use vespertide_naming::{IdentifierStart, sanitize_identifier, seaorm_module_name};

use crate::parallel_config::{EXPORT_RENDER_PAR_MIN_LEN, EXPORT_RENDER_PAR_THRESHOLD};
use crate::utils::load_config;

pub async fn cmd_export(orm: Orm, export_dir: Option<PathBuf>) -> Result<()> {
    let config = load_config()?;
    let models = load_models_recursive(config.models_dir())
        .await
        .context("load models recursively")?;

    // Normalize tables to convert inline constraints (primary_key, foreign_key, etc.) to table-level constraints
    let normalized_models: Vec<(TableDef, PathBuf)> = models
        .into_iter()
        .map(|(table, rel_path)| {
            table
                .normalize()
                .map_err(|e| anyhow::anyhow!("Failed to normalize table '{}': {}", table.name, e))
                .map(|normalized| (normalized, rel_path))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let target_root = resolve_export_dir(export_dir, &config);

    // Prisma uses a single-file output strategy
    if matches!(orm, Orm::Prisma) {
        return cmd_export_prisma(normalized_models, target_root).await;
    }

    // Clean the export directory before regenerating
    clean_export_dir(&target_root, orm).await?;

    if !target_root.exists() {
        fs::create_dir_all(&target_root)
            .await
            .with_context(|| format!("create export dir {}", target_root.display()))?;
    }

    // Extract all tables for schema context (used for FK chain resolution)
    let all_tables: Vec<TableDef> = normalized_models.iter().map(|(t, _)| t.clone()).collect();

    // Build module path mappings for SeaORM cross-directory relation resolution.
    // Maps table_name -> module path segments (e.g., "admin" -> ["admin", "admin"])
    let module_paths: HashMap<String, Vec<String>> = normalized_models
        .iter()
        .map(|(table, rel_path)| {
            let segments = rel_path_to_module_segments(rel_path);
            (table.name.to_string(), segments)
        })
        .collect();

    // Derive crate:: prefix from export directory (e.g., "src/models" -> "crate::models")
    let crate_prefix = export_dir_to_crate_prefix(&target_root);

    // Create per-ORM exporters that honor their `vespertide.json` config section
    let seaorm_exporter = SeaOrmExporterWithConfig::new(config.seaorm(), config.prefix());
    let django_exporter = DjangoExporterWithConfig::new(config.django());
    let gorm_package_name = config.gorm_package_name(&target_root);
    let gorm_exporter = GormExporterWithConfig::new(&gorm_package_name);
    let render_context = ExportRenderContext {
        target_root: &target_root,
        all_tables: &all_tables,
        module_paths: &module_paths,
        crate_prefix: &crate_prefix,
        seaorm_exporter: &seaorm_exporter,
        django_exporter: &django_exporter,
        gorm_exporter: &gorm_exporter,
        orm_kind: orm,
    };

    // Generate all entity code (CPU-bound, done synchronously)
    let entities: Vec<(String, PathBuf, String)> =
        if normalized_models.len() < EXPORT_RENDER_PAR_THRESHOLD {
            normalized_models
                .iter()
                .map(|model| render_export_entity(model, &render_context))
                .collect::<Result<Vec<_>>>()?
        } else {
            normalized_models
                .par_iter()
                .with_min_len(EXPORT_RENDER_PAR_MIN_LEN)
                .map(|model| render_export_entity(model, &render_context))
                .collect::<Result<Vec<_>>>()?
        };

    // Write all files in parallel
    let write_futures: Vec<_> = entities
        .iter()
        .map(|(name, out_path, code)| {
            let name = name.clone();
            let out_path = out_path.clone();
            let code = code.clone();
            async move {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)
                        .await
                        .with_context(|| format!("create parent dir {}", parent.display()))?;
                }
                fs::write(&out_path, &code)
                    .await
                    .with_context(|| format!("write {}", out_path.display()))?;
                println!("Exported {} -> {}", name, out_path.display());
                Ok::<(), anyhow::Error>(())
            }
        })
        .collect();

    try_join_all(write_futures).await?;

    // Ensure mod chain for SeaORM (must be done after all files are written)
    if matches!(orm, Orm::SeaOrm) {
        for (_table, rel_path) in &normalized_models {
            let out_path = build_output_path(&target_root, rel_path, orm);
            ensure_mod_chain(&target_root, rel_path)
                .await
                .with_context(|| format!("ensure mod chain for {}", out_path.display()))?;
        }
    }

    Ok(())
}

struct ExportRenderContext<'a> {
    target_root: &'a Path,
    all_tables: &'a [TableDef],
    module_paths: &'a HashMap<String, Vec<String>>,
    crate_prefix: &'a str,
    seaorm_exporter: &'a SeaOrmExporterWithConfig<'a>,
    django_exporter: &'a DjangoExporterWithConfig<'a>,
    gorm_exporter: &'a GormExporterWithConfig<'a>,
    orm_kind: Orm,
}

fn render_export_entity(
    (table, rel_path): &(TableDef, PathBuf),
    context: &ExportRenderContext<'_>,
) -> Result<(String, PathBuf, String)> {
    let code = match context.orm_kind {
        Orm::SeaOrm => context
            .seaorm_exporter
            .render_entity_with_schema_and_paths(
                table,
                context.all_tables,
                context.module_paths,
                context.crate_prefix,
            )
            .map_err(|e| anyhow::anyhow!(e)),
        Orm::Django => context
            .django_exporter
            .render_entity_with_schema(table, context.all_tables)
            .map_err(|e| anyhow::anyhow!(e)),
        Orm::Gorm => context
            .gorm_exporter
            .render_entity_with_schema(table, context.all_tables)
            .map_err(|e| anyhow::anyhow!(e)),
        _ => render_entity_with_schema(context.orm_kind, table, context.all_tables)
            .map_err(|e| anyhow::anyhow!(e)),
    }?;
    let out_path = build_output_path(context.target_root, rel_path, context.orm_kind);

    Ok((table.name.to_string(), out_path, code))
}

/// Derive `crate::` prefix from the export directory path.
///
/// For example: `src/models` → `crate::models`, `src/db/entities` → `crate::db::entities`.
/// If the path doesn't start with `src/`, returns empty string (fallback to `super::` behavior).
fn export_dir_to_crate_prefix(export_dir: &Path) -> String {
    let normalized = export_dir.to_string_lossy().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);

    if let Some(after_src) = stripped.strip_prefix("src/") {
        let module_path = after_src.trim_end_matches('/').replace('/', "::");
        format!("crate::{module_path}")
    } else {
        String::new()
    }
}

/// Convert a relative model file path to Rust module path segments.
///
/// For example: `admin/admin.json` → `["admin", "admin"]`
/// `estimate/estimate_checker.vespertide.json` → `["estimate", "estimate_checker"]`
fn rel_path_to_module_segments(rel_path: &Path) -> Vec<String> {
    let mut segments = vec![];

    // Add directory components
    if let Some(parent) = rel_path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component
                && let Some(s) = name.to_str()
            {
                segments.push(seaorm_module_name(s));
            }
        }
    }

    // Add file stem (strip extensions and .vespertide suffix)
    if let Some(file_name) = rel_path.file_name().and_then(|n| n.to_str()) {
        let (stem, _) = if let Some(dot_idx) = file_name.rfind('.') {
            file_name.split_at(dot_idx)
        } else {
            (file_name, "")
        };
        let stem = stem.strip_suffix(".vespertide").unwrap_or(stem);
        segments.push(sanitize_filename(stem));
    }

    segments
}

fn resolve_export_dir(export_dir: Option<PathBuf>, config: &VespertideConfig) -> PathBuf {
    if let Some(dir) = export_dir {
        return dir;
    }
    // Prefer explicit model_export_dir from config, fallback to default inside config.
    config.model_export_dir().to_path_buf()
}

/// Clean the export directory by removing all generated files.
/// This ensures no stale files remain from previous exports.
async fn clean_export_dir(root: &Path, orm: Orm) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    clean_dir_recursive(root, orm.file_extension()).await?;
    Ok(())
}

/// Recursively remove files with the given extension and empty directories.
/// Uses async I/O for parallel file operations.
async fn clean_dir_recursive(dir: &Path, ext: &str) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .await
        .with_context(|| format!("read dir {}", dir.display()))?;

    let mut subdirs = vec![];
    let mut files_to_remove = vec![];

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            files_to_remove.push(path);
        }
    }

    let remove_futures: Vec<_> = files_to_remove
        .into_iter()
        .map(|path| async move {
            fs::remove_file(&path)
                .await
                .with_context(|| format!("remove file {}", path.display()))
        })
        .collect();

    let subdir_futures: Vec<_> = subdirs
        .iter()
        .map(|subdir| clean_dir_recursive(subdir, ext))
        .collect();

    // Files and subdirectories are disjoint, so both sweeps run together.
    futures::try_join!(try_join_all(remove_futures), try_join_all(subdir_futures))?;

    // Remove empty directories
    for subdir in subdirs {
        let mut entries = fs::read_dir(&subdir).await?;
        if entries.next_entry().await?.is_none() {
            fs::remove_dir(&subdir)
                .await
                .with_context(|| format!("remove empty dir {}", subdir.display()))?;
        }
    }

    Ok(())
}

fn build_output_path(root: &Path, rel_path: &Path, orm: Orm) -> PathBuf {
    // Sanitize file name: replace spaces with underscores
    let mut out = root.to_path_buf();

    // Reconstruct path with sanitized file name
    for component in rel_path.components() {
        if let std::path::Component::Normal(name) = component {
            out.push(name);
        } else {
            out.push(component.as_os_str());
        }
    }

    // Sanitize the file name (last component)
    if let Some(file_name) = out.file_name().and_then(|n| n.to_str()) {
        // Remove extension, sanitize, then add new extension
        let (stem, _ext) = if let Some(dot_idx) = file_name.rfind('.') {
            file_name.split_at(dot_idx)
        } else {
            (file_name, "")
        };

        // Strip ".vespertide" suffix if present (e.g., "user.vespertide" -> "user")
        let stem = stem.strip_suffix(".vespertide").unwrap_or(stem);

        let sanitized = sanitize_filename(stem);
        let ext = orm.file_extension();
        let file_stem = match orm {
            // Java requires the file name to match the public class name,
            // including the escaping the renderer applies.
            Orm::Jpa => {
                sanitize_identifier(&to_pascal_case(&sanitized), IdentifierStart::Underscore)
            }
            // SeaORM files are Rust modules, so the stem doubles as a `mod` name.
            Orm::SeaOrm => seaorm_module_name(&sanitized),
            _ => sanitized,
        };
        out.set_file_name(format!("{file_stem}.{ext}"));
    }

    out
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

async fn load_models_recursive(base: &Path) -> Result<Vec<(TableDef, PathBuf)>> {
    let mut out = vec![];
    if !base.exists() {
        return Ok(out);
    }
    walk_models(base, base, &mut out).await?;
    Ok(out)
}

async fn ensure_mod_chain(root: &Path, rel_path: &Path) -> Result<()> {
    // SeaORM exports use a standard nested Rust module tree.
    let path_without_ext = rel_path.with_extension("");
    let path_stripped = if let Some(stem) = path_without_ext.file_stem().and_then(|s| s.to_str()) {
        let stripped_stem = stem.strip_suffix(".vespertide").unwrap_or(stem);
        if let Some(parent) = path_without_ext.parent() {
            parent.join(stripped_stem)
        } else {
            PathBuf::from(stripped_stem)
        }
    } else {
        path_without_ext
    };
    let mut comps: Vec<String> = path_stripped
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(seaorm_module_name))
        .collect();
    if comps.is_empty() {
        return Ok(());
    }

    while let Some(child) = comps.pop() {
        let dir = root.join(comps.join(std::path::MAIN_SEPARATOR_STR));
        let mod_path = dir.join("mod.rs");
        if let Some(parent) = mod_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent).await?;
        }

        let mut content = if mod_path.exists() {
            fs::read_to_string(&mod_path).await?
        } else {
            String::new()
        };

        let decl = format!("pub mod {child};");
        if !content.lines().any(|line| line.trim() == decl) {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&decl);
            content.push('\n');
            fs::write(mod_path, content).await?;
        }
    }

    Ok(())
}

async fn cmd_export_prisma(
    normalized_models: Vec<(TableDef, PathBuf)>,
    target_root: PathBuf,
) -> Result<()> {
    let all_tables: Vec<TableDef> = normalized_models.iter().map(|(t, _)| t.clone()).collect();
    let content = prisma::render_schema(&all_tables);

    clean_export_dir(&target_root, Orm::Prisma).await?;

    if !target_root.exists() {
        fs::create_dir_all(&target_root)
            .await
            .with_context(|| format!("create export dir {}", target_root.display()))?;
    }

    // Not `schema.prisma`: that name belongs to the user's own file holding the
    // datasource/generator blocks.
    let out_path = target_root.join("models.prisma");
    fs::write(&out_path, &content)
        .await
        .with_context(|| format!("write {}", out_path.display()))?;

    println!(
        "Exported {} model(s) -> {}",
        normalized_models.len(),
        out_path.display()
    );

    Ok(())
}

#[async_recursion::async_recursion]
async fn walk_models(
    root: &Path,
    current: &Path,
    acc: &mut Vec<(TableDef, PathBuf)>,
) -> Result<()> {
    let mut entries = fs::read_dir(current)
        .await
        .with_context(|| format!("read {}", current.display()))?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            walk_models(root, &path, acc).await?;
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str());
        if !matches!(ext, Some("json" | "yaml" | "yml")) {
            continue;
        }
        let content = fs::read_to_string(&path)
            .await
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
mod tests;
