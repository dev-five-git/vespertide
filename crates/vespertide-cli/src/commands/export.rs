use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use futures::future::try_join_all;
use tokio::fs;
use vespertide_config::VespertideConfig;
use vespertide_core::TableDef;
use vespertide_exporter::{Orm, render_entity_with_schema, seaorm::SeaOrmExporterWithConfig};

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

pub async fn cmd_export(orm: OrmArg, export_dir: Option<PathBuf>) -> Result<()> {
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

    // Clean the export directory before regenerating
    let orm_kind: Orm = orm.into();
    clean_export_dir(&target_root, orm_kind).await?;

    if !target_root.exists() {
        fs::create_dir_all(&target_root)
            .await
            .with_context(|| format!("create export dir {}", target_root.display()))?;
    }

    // Extract all tables for schema context (used for FK chain resolution)
    let all_tables: Vec<TableDef> = normalized_models.iter().map(|(t, _)| t.clone()).collect();

    // Create SeaORM exporter with config if needed
    let seaorm_exporter = SeaOrmExporterWithConfig::new(config.seaorm(), config.prefix());

    // Generate all entity code (CPU-bound, done synchronously)
    let entities: Vec<(String, PathBuf, String)> = normalized_models
        .iter()
        .map(|(table, rel_path)| {
            let code = match orm_kind {
                Orm::SeaOrm => seaorm_exporter
                    .render_entity_with_schema(table, &all_tables)
                    .map_err(|e| anyhow::anyhow!(e)),
                _ => render_entity_with_schema(orm_kind, table, &all_tables)
                    .map_err(|e| anyhow::anyhow!(e)),
            }?;
            let out_path = build_output_path(&target_root, rel_path, orm_kind);
            Ok((table.name.clone(), out_path, code))
        })
        .collect::<Result<Vec<_>>>()?;

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
    if matches!(orm_kind, Orm::SeaOrm) {
        for (_, rel_path) in &normalized_models {
            let out_path = build_output_path(&target_root, rel_path, orm_kind);
            ensure_mod_chain(&target_root, rel_path)
                .await
                .with_context(|| format!("ensure mod chain for {}", out_path.display()))?;
        }
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

/// Clean the export directory by removing all generated files.
/// This ensures no stale files remain from previous exports.
async fn clean_export_dir(root: &Path, orm: Orm) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let ext = match orm {
        Orm::SeaOrm => "rs",
        Orm::SqlAlchemy | Orm::SqlModel => "py",
    };

    clean_dir_recursive(root, ext).await?;
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

    let mut subdirs = Vec::new();
    let mut files_to_remove = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            files_to_remove.push(path);
        }
    }

    // Remove files in parallel
    let remove_futures: Vec<_> = files_to_remove
        .into_iter()
        .map(|path| async move {
            fs::remove_file(&path)
                .await
                .with_context(|| format!("remove file {}", path.display()))
        })
        .collect();

    try_join_all(remove_futures).await?;

    // Recursively clean subdirectories
    let subdir_futures: Vec<_> = subdirs
        .iter()
        .map(|subdir| clean_dir_recursive(subdir, ext))
        .collect();

    try_join_all(subdir_futures).await?;

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
        let ext = match orm {
            Orm::SeaOrm => "rs",
            Orm::SqlAlchemy | Orm::SqlModel => "py",
        };
        out.set_file_name(format!("{}.{}", sanitized, ext));
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
    let mut out = Vec::new();
    if !base.exists() {
        return Ok(out);
    }
    walk_models(base, base, &mut out).await?;
    Ok(out)
}

async fn ensure_mod_chain(root: &Path, rel_path: &Path) -> Result<()> {
    // Only needed for SeaORM (Rust) exports to wire modules.
    // Strip extension and ".vespertide" suffix from filename
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
        .filter_map(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| sanitize_filename(s).to_string())
        })
        .collect();
    if comps.is_empty() {
        return Ok(());
    }
    // Build from deepest file up to root: dir/mod.rs should include child module.
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
        let decl = format!("pub mod {};", child);
        if !content.lines().any(|l| l.trim() == decl) {
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
        if !matches!(ext, Some("json") | Some("yaml") | Some("yml")) {
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
mod tests {
    use super::*;
    use rstest::rstest;
    use serial_test::serial;
    use std::fs as std_fs;
    use tempfile::tempdir;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};

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
        std_fs::write("vespertide.json", text).unwrap();
    }

    fn write_model(path: &Path, table: &TableDef) {
        if let Some(parent) = path.parent() {
            std_fs::create_dir_all(parent).unwrap();
        }
        std_fs::write(path, serde_json::to_string_pretty(table).unwrap()).unwrap();
    }

    fn sample_table(name: &str) -> TableDef {
        TableDef {
            name: name.to_string(),
            description: None,
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
        }
    }

    #[tokio::test]
    #[serial]
    async fn export_writes_seaorm_files_to_default_dir() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("users");
        write_model(Path::new("models/users.json"), &model);

        cmd_export(OrmArg::Seaorm, None).await.unwrap();

        let out = PathBuf::from("src/models/users.rs");
        assert!(out.exists());
        let content = std_fs::read_to_string(out).unwrap();
        assert!(content.contains("#[sea_orm(table_name = \"users\")]"));

        // mod.rs wiring at root
        let root_mod = PathBuf::from("src/models/mod.rs");
        assert!(root_mod.exists());
        let root_mod_content = std_fs::read_to_string(root_mod).unwrap();
        assert!(root_mod_content.contains("pub mod users;"));
    }

    #[tokio::test]
    #[serial]
    async fn export_respects_custom_output_dir() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("posts");
        write_model(Path::new("models/blog/posts.json"), &model);

        let custom = PathBuf::from("out_dir");
        cmd_export(OrmArg::Seaorm, Some(custom.clone()))
            .await
            .unwrap();

        let out = custom.join("blog/posts.rs");
        assert!(out.exists());
        let content = std_fs::read_to_string(out).unwrap();
        assert!(content.contains("#[sea_orm(table_name = \"posts\")]"));

        // mod.rs wiring
        let root_mod = custom.join("mod.rs");
        let blog_mod = custom.join("blog/mod.rs");
        assert!(root_mod.exists());
        assert!(blog_mod.exists());
        let root_mod_content = std_fs::read_to_string(root_mod).unwrap();
        let blog_mod_content = std_fs::read_to_string(blog_mod).unwrap();
        assert!(root_mod_content.contains("pub mod blog;"));
        assert!(blog_mod_content.contains("pub mod posts;"));
    }

    #[tokio::test]
    #[serial]
    async fn export_with_sqlalchemy_sets_py_extension() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("items");
        write_model(Path::new("models/items.json"), &model);

        cmd_export(OrmArg::Sqlalchemy, None).await.unwrap();

        let out = PathBuf::from("src/models/items.py");
        assert!(out.exists());
        let content = std_fs::read_to_string(out).unwrap();
        assert!(content.contains("items"));
    }

    #[tokio::test]
    #[serial]
    async fn export_with_sqlmodel_sets_py_extension() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        let model = sample_table("orders");
        write_model(Path::new("models/orders.json"), &model);

        cmd_export(OrmArg::Sqlmodel, None).await.unwrap();

        let out = PathBuf::from("src/models/orders.py");
        assert!(out.exists());
        let content = std_fs::read_to_string(out).unwrap();
        assert!(content.contains("orders"));
    }

    #[tokio::test]
    #[serial]
    async fn load_models_recursive_returns_empty_when_absent() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let models = load_models_recursive(Path::new("no_models")).await.unwrap();
        assert!(models.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn load_models_recursive_ignores_non_model_files() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        std_fs::create_dir_all("models").unwrap();
        std_fs::write("models/ignore.txt", "hello").unwrap();
        write_model(Path::new("models/valid.json"), &sample_table("valid"));

        let models = load_models_recursive(Path::new("models")).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].0.name, "valid");
    }

    #[tokio::test]
    #[serial]
    async fn load_models_recursive_parses_yaml_branch() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        std_fs::create_dir_all("models").unwrap();
        let table = sample_table("yaml_table");
        let yaml = serde_yaml::to_string(&table).unwrap();
        std_fs::write("models/yaml_table.yaml", yaml).unwrap();

        let models = load_models_recursive(Path::new("models")).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].0.name, "yaml_table");
    }

    #[tokio::test]
    #[serial]
    async fn ensure_mod_chain_adds_to_existing_file_without_trailing_newline() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("src/models");
        std_fs::create_dir_all(&root).unwrap();
        std_fs::write(root.join("mod.rs"), "pub mod existing;").unwrap();

        ensure_mod_chain(&root, Path::new("blog/posts.rs"))
            .await
            .unwrap();

        let root_mod = std_fs::read_to_string(root.join("mod.rs")).unwrap();
        let blog_mod = std_fs::read_to_string(root.join("blog/mod.rs")).unwrap();
        assert!(root_mod.contains("pub mod existing;"));
        assert!(root_mod.contains("pub mod blog;"));
        assert!(blog_mod.contains("pub mod posts;"));
        // ensure newline appended if missing
        assert!(root_mod.ends_with('\n'));
    }

    #[tokio::test]
    async fn ensure_mod_chain_no_components_is_noop() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("src/models");
        std_fs::create_dir_all(&root).unwrap();
        // empty path should not error
        assert!(ensure_mod_chain(&root, Path::new("")).await.is_ok());
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

    #[rstest]
    #[case(OrmArg::Seaorm, Orm::SeaOrm)]
    #[case(OrmArg::Sqlalchemy, Orm::SqlAlchemy)]
    #[case(OrmArg::Sqlmodel, Orm::SqlModel)]
    fn orm_arg_maps_to_enum(#[case] arg: OrmArg, #[case] expected: Orm) {
        assert_eq!(Orm::from(arg), expected);
    }

    #[rstest]
    #[case("normal_name", "normal_name")]
    #[case("user copy", "user_copy")]
    #[case("user  copy", "user__copy")]
    #[case("user-copy", "user-copy")]
    #[case("user.copy", "user_copy")]
    #[case("user copy.json", "user_copy_json")]
    fn test_sanitize_filename(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(sanitize_filename(input), expected);
    }

    #[test]
    fn build_output_path_sanitizes_spaces() {
        use std::path::Path;
        let root = Path::new("src/models");
        let rel_path = Path::new("user copy.json");
        let out = build_output_path(root, rel_path, Orm::SeaOrm);
        assert_eq!(out, Path::new("src/models/user_copy.rs"));

        let rel_path2 = Path::new("blog/post name.yaml");
        let out2 = build_output_path(root, rel_path2, Orm::SeaOrm);
        assert_eq!(out2, Path::new("src/models/blog/post_name.rs"));
    }

    #[test]
    fn build_output_path_handles_file_without_extension() {
        use std::path::Path;
        let root = Path::new("src/models");
        // File without extension - covers line 88 (else branch)
        let rel_path = Path::new("users");
        let out = build_output_path(root, rel_path, Orm::SeaOrm);
        assert_eq!(out, Path::new("src/models/users.rs"));

        let out_py = build_output_path(root, rel_path, Orm::SqlAlchemy);
        assert_eq!(out_py, Path::new("src/models/users.py"));
    }

    #[test]
    fn build_output_path_handles_special_path_components() {
        use std::path::Path;
        let root = Path::new("src/models");
        // Path with CurDir component (.) - covers line 78 (non-Normal component branch)
        let rel_path = Path::new("./blog/posts.json");
        let out = build_output_path(root, rel_path, Orm::SeaOrm);
        // The . component gets pushed via the else branch
        assert!(out.to_string_lossy().contains("posts"));

        // Path with ParentDir component (..)
        let rel_path2 = Path::new("../other/items.yaml");
        let out2 = build_output_path(root, rel_path2, Orm::SeaOrm);
        assert!(out2.to_string_lossy().contains("items"));
    }

    #[test]
    fn build_output_path_strips_vespertide_suffix() {
        use std::path::Path;
        let root = Path::new("src/models");

        // .vespertide.json -> .rs (strips ".vespertide" from stem)
        let rel_path = Path::new("user.vespertide.json");
        let out = build_output_path(root, rel_path, Orm::SeaOrm);
        assert_eq!(out, Path::new("src/models/user.rs"));

        // Nested path with .vespertide.json
        let rel_path2 = Path::new("blog/post.vespertide.json");
        let out2 = build_output_path(root, rel_path2, Orm::SeaOrm);
        assert_eq!(out2, Path::new("src/models/blog/post.rs"));

        // .vespertide.yaml -> .py
        let rel_path3 = Path::new("order.vespertide.yaml");
        let out3 = build_output_path(root, rel_path3, Orm::SqlAlchemy);
        assert_eq!(out3, Path::new("src/models/order.py"));

        // Regular .json without .vespertide suffix still works
        let rel_path4 = Path::new("item.json");
        let out4 = build_output_path(root, rel_path4, Orm::SeaOrm);
        assert_eq!(out4, Path::new("src/models/item.rs"));
    }

    #[tokio::test]
    #[serial]
    async fn ensure_mod_chain_strips_vespertide_suffix() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("src/models");
        std_fs::create_dir_all(&root).unwrap();

        // File with .vespertide suffix should produce mod declaration without it
        ensure_mod_chain(&root, Path::new("user.vespertide.json"))
            .await
            .unwrap();

        let root_mod = std_fs::read_to_string(root.join("mod.rs")).unwrap();
        // Should be "pub mod user;" not "pub mod user_vespertide;"
        assert!(root_mod.contains("pub mod user;"));
        assert!(!root_mod.contains("user_vespertide"));

        // Nested path with .vespertide suffix
        ensure_mod_chain(&root, Path::new("blog/post.vespertide.json"))
            .await
            .unwrap();
        let root_mod = std_fs::read_to_string(root.join("mod.rs")).unwrap();
        let blog_mod = std_fs::read_to_string(root.join("blog/mod.rs")).unwrap();
        assert!(root_mod.contains("pub mod blog;"));
        assert!(blog_mod.contains("pub mod post;"));
        assert!(!blog_mod.contains("post_vespertide"));
    }

    #[tokio::test]
    async fn clean_export_dir_removes_rs_files_for_seaorm() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("export_dir");
        std_fs::create_dir_all(&root).unwrap();

        // Create some .rs files that should be cleaned
        std_fs::write(root.join("old_model.rs"), "// old rust file").unwrap();
        std_fs::write(root.join("another.rs"), "// another rust file").unwrap();
        // Create a non-.rs file that should NOT be cleaned
        std_fs::write(root.join("keep.txt"), "keep this").unwrap();

        clean_export_dir(&root, Orm::SeaOrm).await.unwrap();

        // .rs files should be gone
        assert!(!root.join("old_model.rs").exists());
        assert!(!root.join("another.rs").exists());
        // .txt file should remain
        assert!(root.join("keep.txt").exists());
    }

    #[tokio::test]
    async fn clean_export_dir_removes_py_files_for_sqlalchemy() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("export_dir");
        std_fs::create_dir_all(&root).unwrap();

        // Create some .py files that should be cleaned
        std_fs::write(root.join("old_model.py"), "# old python file").unwrap();
        // Create a .rs file that should NOT be cleaned
        std_fs::write(root.join("keep.rs"), "// keep this").unwrap();

        clean_export_dir(&root, Orm::SqlAlchemy).await.unwrap();

        // .py files should be gone
        assert!(!root.join("old_model.py").exists());
        // .rs file should remain
        assert!(root.join("keep.rs").exists());
    }

    #[tokio::test]
    async fn clean_export_dir_removes_py_files_for_sqlmodel() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("export_dir");
        std_fs::create_dir_all(&root).unwrap();

        std_fs::write(root.join("model.py"), "# python file").unwrap();

        clean_export_dir(&root, Orm::SqlModel).await.unwrap();

        assert!(!root.join("model.py").exists());
    }

    #[tokio::test]
    async fn clean_export_dir_handles_missing_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("nonexistent_dir");

        // Should not error on missing directory
        let result = clean_export_dir(&root, Orm::SeaOrm).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn clean_dir_recursive_cleans_subdirectories() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("export_dir");
        let subdir = root.join("nested");
        std_fs::create_dir_all(&subdir).unwrap();

        // Create files in root and subdirectory
        std_fs::write(root.join("root.rs"), "// root").unwrap();
        std_fs::write(subdir.join("nested.rs"), "// nested").unwrap();
        std_fs::write(subdir.join("keep.txt"), "keep").unwrap();

        clean_dir_recursive(&root, "rs").await.unwrap();

        // .rs files should be gone
        assert!(!root.join("root.rs").exists());
        assert!(!subdir.join("nested.rs").exists());
        // .txt file should remain
        assert!(subdir.join("keep.txt").exists());
        // subdir should still exist (has .txt file)
        assert!(subdir.exists());
    }

    #[tokio::test]
    async fn clean_dir_recursive_removes_empty_subdirectories() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("export_dir");
        let subdir = root.join("empty_after_clean");
        std_fs::create_dir_all(&subdir).unwrap();

        // Create only .rs files in subdirectory
        std_fs::write(subdir.join("only.rs"), "// only").unwrap();

        clean_dir_recursive(&root, "rs").await.unwrap();

        // .rs file should be gone
        assert!(!subdir.join("only.rs").exists());
        // Empty subdirectory should be removed
        assert!(!subdir.exists());
    }

    #[tokio::test]
    async fn clean_dir_recursive_handles_non_directory() {
        let tmp = tempdir().unwrap();
        let file_path = tmp.path().join("not_a_dir.txt");
        std_fs::write(&file_path, "content").unwrap();

        // Should not error when called on a file instead of directory
        let result = clean_dir_recursive(&file_path, "rs").await;
        assert!(result.is_ok());
    }
}
