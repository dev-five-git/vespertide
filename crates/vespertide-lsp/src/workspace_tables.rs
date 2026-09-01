//! Disk-discovered workspace tables.
//!
//! The LSP keeps open documents in [`DocumentStore`](crate::DocumentStore), but
//! cross-file features also need models that have not been opened by the editor.
//! This cache is populated from `vespertide.json` + `vespertide-loader` on
//! initialize and refreshed after document changes. [`BTreeMap`] keeps
//! iteration deterministic across the workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tree_sitter::Tree;

use crate::parser::{DocumentFormat, ParserPool};
use vespertide_core::TableDef;

#[derive(Debug, Default)]
pub struct WorkspaceTables {
    inner: RwLock<Inner>,
    /// Monotonic counter bumped every time `inner`'s table set is replaced by
    /// `refresh()`. Consumers (the diagnostics workspace-table cache) read this
    /// as part of their cache key to detect disk-side changes in O(1) without
    /// hashing the whole table set. See `generation()`.
    generation: AtomicU64,
}

#[derive(Debug, Default)]
struct Inner {
    root: Option<PathBuf>,
    by_name: BTreeMap<String, TableDef>,
    /// `table_name → absolute file path` recorded during refresh. Needed
    /// because filenames don't always match the declared `name`
    /// (`media.vespertide.json` declares `name: media`, but
    /// `models/my_table.json` is also valid and declares `name: user`).
    path_by_name: BTreeMap<String, PathBuf>,
    /// Per-path cached `(text, tree)` from `cached_parse`. Keyed on
    /// canonical absolute path; entries are invalidated on `refresh()` OR
    /// individually on stale mtime in `cached_parse`. Trees are
    /// `Arc`-shared so consumers can hold them without copying.
    tree_cache: BTreeMap<PathBuf, CachedTree>,
}

#[derive(Debug, Clone)]
struct CachedTree {
    mtime: SystemTime,
    text: Arc<String>,
    tree: Arc<Tree>,
    format: DocumentFormat,
}

const LOCK_POISONED_MSG: &str =
    "workspace_tables lock poisoned — invariant: no panic while holding lock";

impl WorkspaceTables {
    pub fn new() -> Self {
        Self::default()
    }

    fn read_inner(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner.read().expect(LOCK_POISONED_MSG)
    }

    fn write_inner(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner.write().expect(LOCK_POISONED_MSG)
    }

    /// Walk up from `start` looking for `vespertide.json`, then load all models.
    ///
    /// Returns `true` only when a config was found and at least one table loaded.
    pub fn refresh(&self, start: &Path) -> bool {
        if let Some(root) = find_workspace_root(start) {
            if let Ok(config) =
                vespertide_loader::load_config_from_path(root.join("vespertide.json"))
            {
                let models_dir = root.join(config.models_dir());

                // Walk the models directory ourselves so we capture
                // (table_name, file_path) — vespertide-loader's public API only
                // returns the parsed tables and drops the filename.
                let mut by_name: BTreeMap<String, TableDef> = BTreeMap::new();
                let mut path_by_name: BTreeMap<String, PathBuf> = BTreeMap::new();
                collect_models(&models_dir, &mut by_name, &mut path_by_name);
                let count = by_name.len();

                *self.write_inner() = Inner {
                    root: Some(root),
                    by_name,
                    path_by_name,
                    tree_cache: BTreeMap::new(),
                };
                // Bump AFTER the write completes so a concurrent reader that
                // observes the new generation is guaranteed (via the lock) to
                // see the new table set.
                self.generation.fetch_add(1, Ordering::Relaxed);

                count > 0
            } else {
                false
            }
        } else {
            *self.write_inner() = Inner::default();
            self.generation.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Monotonic generation counter, incremented whenever `refresh()` replaces
    /// the in-memory table set. Cheap (`Relaxed` atomic load) cache-key input
    /// for consumers that must invalidate when disk-discovered tables change.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    pub fn get(&self, name: &str) -> Option<TableDef> {
        self.read_inner().by_name.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.read_inner().by_name.keys().cloned().collect()
    }

    pub fn all(&self) -> Vec<(String, TableDef)> {
        self.read_inner()
            .by_name
            .iter()
            .map(|(name, table)| (name.clone(), table.clone()))
            .collect()
    }

    pub fn root(&self) -> Option<PathBuf> {
        self.read_inner().root.clone()
    }

    /// Look up the on-disk file path that declared `table_name`. Cached at
    /// `refresh()` time so the lookup is filename-agnostic — works for
    /// `media.json`, `media.vespertide.json`, or `models/whatever.json`
    /// regardless of the filename convention.
    pub fn model_path(&self, table_name: &str) -> Option<PathBuf> {
        self.read_inner().path_by_name.get(table_name).cloned()
    }

    /// Lookup or parse on-demand.
    ///
    /// On hit (path exists in cache + mtime unchanged), returns
    /// `(Arc<String>, Arc<Tree>)` without touching disk.
    ///
    /// On miss or stale mtime: reads the file, parses with `pool`, stores in
    /// cache, returns the new shared (text, tree) tuple.
    ///
    /// Returns `None` if the file is unreadable or the parser fails.
    ///
    /// # Format detection
    /// Uses the file extension (`.yaml` / `.yml` → YAML, else → JSON).
    pub fn cached_parse(&self, path: &Path, pool: &ParserPool) -> Option<(Arc<String>, Arc<Tree>)> {
        let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
        let format = format_for_path(path);

        {
            let inner = self.read_inner();
            if let Some(entry) = inner.tree_cache.get(path)
                && entry.mtime == mtime
                && entry.format == format
            {
                return Some((Arc::clone(&entry.text), Arc::clone(&entry.tree)));
            }
        }

        let text = std::fs::read_to_string(path).ok()?;
        let tree = pool.parse(&text, format)?;

        let text_arc = Arc::new(text);
        let tree_arc = Arc::new(tree);

        {
            let mut inner = self.write_inner();
            inner.tree_cache.insert(
                path.to_path_buf(),
                CachedTree {
                    mtime,
                    text: Arc::clone(&text_arc),
                    tree: Arc::clone(&tree_arc),
                    format,
                },
            );
        }

        Some((text_arc, tree_arc))
    }
}

fn format_for_path(path: &Path) -> DocumentFormat {
    match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => DocumentFormat::Yaml,
        _ => DocumentFormat::Json,
    }
}

/// Recursively walk `dir` collecting every `.json` / `.yaml` / `.yml`
/// model file. For each file we parse + normalize the `TableDef` and
/// record `(name → table)` alongside `(name → path)`.
///
/// On parse / normalize failure we silently skip the file: the diagnostics
/// engine will surface a parse error for any opened model, and silently
/// skipping disk-only invalid files keeps the workspace cache from
/// blocking on a single corrupted model.
fn collect_models(
    dir: &Path,
    by_name: &mut BTreeMap<String, TableDef>,
    path_by_name: &mut BTreeMap<String, PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_models(&path, by_name, path_by_name);
        } else if is_model_file(&path)
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Some(table) = parse_table(&path, &content)
            && let Ok(normalized) = table.normalize()
        {
            let name = normalized.name.clone();
            by_name.insert(name.to_string(), normalized);
            path_by_name.insert(name.to_string(), path);
        }
    }
}

fn is_model_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("json" | "yaml" | "yml")
    )
}

fn parse_table(path: &Path, content: &str) -> Option<TableDef> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => serde_json::from_str(content).ok(),
        Some("yaml" | "yml") => serde_yaml::from_str(content).ok(),
        _ => None,
    }
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()
    } else {
        Some(start)
    };

    while let Some(dir) = current {
        if dir.join("vespertide.json").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }

    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn cached_parse_returns_same_arc_when_mtime_unchanged() {
        use crate::parser::ParserPool;

        let tmp = tempdir().unwrap();
        let path = tmp.path().join("user.json");
        fs::write(&path, r#"{"name":"user","columns":[]}"#).unwrap();
        let tables = WorkspaceTables::new();
        let pool = ParserPool::new();

        let (t1, tree1) = tables.cached_parse(&path, &pool).expect("first");
        let (t2, tree2) = tables.cached_parse(&path, &pool).expect("second");

        assert!(
            Arc::ptr_eq(&t1, &t2),
            "text Arc identity preserved on cache hit"
        );
        assert!(
            Arc::ptr_eq(&tree1, &tree2),
            "tree Arc identity preserved on cache hit"
        );
    }

    #[test]
    fn cached_parse_reparses_when_mtime_advances() {
        use crate::parser::ParserPool;

        let tmp = tempdir().unwrap();
        let path = tmp.path().join("user.json");
        fs::write(&path, r#"{"name":"user","columns":[]}"#).unwrap();
        let tables = WorkspaceTables::new();
        let pool = ParserPool::new();

        let (_t1, tree1) = tables.cached_parse(&path, &pool).expect("first");

        // Sleep to ensure mtime advances (Windows NTFS has 100ns resolution
        // but actual updates can lag; 50ms is generous enough).
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(
            &path,
            r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#,
        )
        .unwrap();

        let (_t2, tree2) = tables.cached_parse(&path, &pool).expect("second");
        assert!(
            !Arc::ptr_eq(&tree1, &tree2),
            "tree should be re-parsed after mtime advances"
        );
    }

    #[test]
    fn refresh_clears_tree_cache() {
        use crate::parser::ParserPool;

        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("models")).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake"}"#).unwrap();
        fs::write(
            tmp.path().join("models/user.json"),
            r#"{"name":"user","columns":[]}"#,
        )
        .unwrap();

        let tables = WorkspaceTables::new();
        let pool = ParserPool::new();
        let path = tmp.path().join("models/user.json");

        let (_t1, tree1) = tables.cached_parse(&path, &pool).expect("populate cache");
        tables.refresh(tmp.path());
        let (_t2, tree2) = tables.cached_parse(&path, &pool).expect("post-refresh");

        assert!(
            !Arc::ptr_eq(&tree1, &tree2),
            "refresh() must invalidate tree cache"
        );
    }

    #[test]
    fn cached_parse_returns_none_for_missing_file() {
        use crate::parser::ParserPool;

        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("does_not_exist.json");
        let tables = WorkspaceTables::new();
        let pool = ParserPool::new();

        assert!(tables.cached_parse(&missing, &pool).is_none());
    }

    #[test]
    fn cached_parse_detects_yaml_by_extension() {
        use crate::parser::ParserPool;

        let tmp = tempdir().unwrap();
        let path = tmp.path().join("user.yaml");
        fs::write(&path, "name: user\ncolumns: []\n").unwrap();
        let tables = WorkspaceTables::new();
        let pool = ParserPool::new();

        let (_text, _tree) = tables.cached_parse(&path, &pool).expect("YAML parse");
    }

    #[test]
    fn no_config_refresh_returns_false() {
        let tmp = tempdir().unwrap();
        let tables = WorkspaceTables::new();

        assert!(!tables.refresh(tmp.path()));
        assert!(tables.names().is_empty());
    }

    #[test]
    fn loads_models_when_config_present() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let tables = WorkspaceTables::new();

        assert!(tables.refresh(tmp.path()));
        assert!(tables.names().contains(&"user".to_string()));
        assert!(tables.get("user").is_some());
        assert_eq!(tables.root().as_deref(), Some(tmp.path()));
        assert_eq!(
            tables.model_path("user"),
            Some(models_dir.join("user.json"))
        );
    }

    #[test]
    fn refresh_uses_configured_models_dir() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("schema_models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"schema_models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("account.json"), r#"{"name":"account","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let tables = WorkspaceTables::new();

        assert!(tables.refresh(tmp.path()));
        assert_eq!(
            tables.model_path("account"),
            Some(models_dir.join("account.json"))
        );
    }

    /// Regression — `media.vespertide.json` declares `name: media`. The
    /// old `model_path` only tried `media.json`, missed the double
    /// extension, and made `collect_workspace_tables` drop the model. The
    /// planner then reported `foreign key references non-existent table`
    /// even though hover (which uses `get(name)` directly) showed
    /// `Target table: media (on disk)`.
    #[test]
    fn model_path_resolves_double_extension_filenames() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("media.vespertide.json"), r#"{"name":"media","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let tables = WorkspaceTables::new();
        assert!(tables.refresh(tmp.path()));
        assert!(tables.names().contains(&"media".to_string()));
        assert_eq!(
            tables.model_path("media"),
            Some(models_dir.join("media.vespertide.json")),
            "double-extension files must be discoverable by their `name`"
        );
    }

    #[test]
    fn model_path_resolves_when_filename_disagrees_with_name() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        // Filename `something_weird.json` but the declared name is `user`.
        fs::write(models_dir.join("something_weird.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let tables = WorkspaceTables::new();
        assert!(tables.refresh(tmp.path()));
        assert_eq!(
            tables.model_path("user"),
            Some(models_dir.join("something_weird.json")),
            "model_path must follow the declared `name`, not the filename"
        );
    }

    #[test]
    fn collect_models_skips_missing_dir_non_models_bad_utf8_bad_parse_and_bad_normalize() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let mut by_name = BTreeMap::new();
        let mut path_by_name = BTreeMap::new();
        collect_models(&missing, &mut by_name, &mut path_by_name);
        assert!(by_name.is_empty());

        let models = tmp.path().join("models");
        let nested = models.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(models.join("notes.txt"), "not a model").unwrap();
        fs::write(models.join("bad_utf8.json"), [0xff, 0xfe]).unwrap();
        fs::write(models.join("bad_parse.json"), "not json").unwrap();
        fs::write(models.join("bad_normalize.json"), r#"{"name":"bad","columns":[{"name":"id","type":"integer","index":["ix_bad","ix_bad"]}]}"#).unwrap();
        fs::write(nested.join("user.yaml"), "name: user\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n").unwrap();

        collect_models(&models, &mut by_name, &mut path_by_name);

        assert!(
            by_name.contains_key("user"),
            "valid nested YAML model should load"
        );
        assert!(!by_name.contains_key("bad"));
    }

    #[test]
    fn parse_table_rejects_unknown_extension() {
        assert!(parse_table(std::path::Path::new("model.toml"), "name = 'user'").is_none());
    }

    #[test]
    fn refresh_with_unreadable_config_returns_false() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("vespertide.json")).unwrap();
        let tables = WorkspaceTables::new();

        assert!(!tables.refresh(tmp.path()));
    }

    #[test]
    fn get_returns_none_for_unknown_table() {
        let tables = WorkspaceTables::new();

        assert!(tables.get("unknown").is_none());
    }

    #[test]
    fn all_returns_loaded_name_table_pairs() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();
        let tables = WorkspaceTables::new();

        assert!(tables.refresh(tmp.path()));
        assert!(tables.all().iter().any(|(name, _)| name == "user"));
    }

    #[test]
    fn refresh_skips_models_that_fail_normalize() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("bad.json"), r#"{"name":"bad","columns":[{"name":"id","type":"integer"},{"name":"id","type":"text"}]}"#).unwrap();
        let tables = WorkspaceTables::new();

        assert!(!tables.refresh(tmp.path()));
        assert!(tables.get("bad").is_none());
    }

    #[test]
    fn collect_models_skips_table_when_inline_fk_normalize_fails() {
        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(models_dir.join("bad_fk.json"), r#"{"name":"bad_fk","columns":[{"name":"user_id","type":"integer","foreign_key":"users"}]}"#).unwrap();
        let mut by_name = BTreeMap::new();
        let mut path_by_name = BTreeMap::new();

        collect_models(&models_dir, &mut by_name, &mut path_by_name);

        assert!(by_name.is_empty());
        assert!(path_by_name.is_empty());
    }

    #[test]
    fn default_constructs_workspace_tables() {
        let _ = WorkspaceTables::default();
    }
}
