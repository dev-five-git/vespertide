//! Workspace-wide drift computation entry points and path-resolution
//! helpers.
//!
//! `compute_with_cache` is the cached hot-path used by the LSP backend on
//! every `did_change` debounce. `compute` is the cache-free convenience
//! that owns a process-static `DriftCache` for ad-hoc callers (CLI, tests).

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use tower_lsp_server::ls_types::Uri;
use vespertide_config::VespertideConfig;

use crate::parser::ParserPool;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;

use super::actions::action_to_drift;
use super::cache::{self, DriftCache, LoadedState};
use super::sources::source_and_tree;
use super::types::DomainDrift;

/// Same as `compute` but reuses a per-instance `DriftCache` to skip loading
/// models / migrations when no input file has changed since the last call.
/// The backend should hold one `Arc<DriftCache>` for the server lifetime and
/// pass it here on every `did_change`-triggered drift refresh.
///
/// Returns an empty vector when `vespertide.json` is not found or any loader /
/// planner step fails. Drift diagnostics are best-effort and must never block
/// normal LSP feedback.
#[must_use]
pub fn compute_with_cache(
    workspace_root: &Path,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    cache: &DriftCache,
) -> Vec<DomainDrift> {
    static SHARED_POOL: OnceLock<ParserPool> = OnceLock::new();

    let Some((project_root, config_mtime)) = find_config_and_mtime(workspace_root) else {
        return Vec::new();
    };

    let models_dir_path = project_root.join("models");
    let migrations_dir_path = project_root.join("migrations");
    let max_model_mtime = cache::max_mtime_in_dir(&models_dir_path);
    let max_migration_mtime = cache::max_mtime_in_dir(&migrations_dir_path);
    let fingerprint = cache::docstore_fingerprint(docs);

    if let Some(cached_drifts) = cache.get_drifts(
        &project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        fingerprint,
    ) {
        return (*cached_drifts).clone();
    }

    let Some(loaded) = loaded_state_with_cache(
        &project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        cache,
    ) else {
        return Vec::new();
    };

    debug_assert_eq!(
        loaded.models_dir,
        resolve_models_dir(&project_root, &loaded.config)
    );

    let Ok(plan) = vespertide_planner::diff_schemas(&loaded.baseline, &loaded.current_models)
    else {
        return Vec::new();
    };

    if plan.actions.is_empty() {
        let drifts_arc = Arc::new(Vec::new());
        cache.store_drifts(
            project_root,
            config_mtime,
            max_model_mtime,
            max_migration_mtime,
            fingerprint,
            Arc::clone(&drifts_arc),
        );
        return (*drifts_arc).clone();
    }

    let parser_pool = SHARED_POOL.get_or_init(ParserPool::new);
    let mut drifts = Vec::new();

    for action in &plan.actions {
        if let Some(table_name) = action.table_name()
            && let Some(uri) = index
                .lookup(table_name)
                .map(|loc| loc.uri)
                .or_else(|| guess_uri(&loaded.models_dir, table_name))
            && let Some((source, tree)) = source_and_tree(&uri, docs, parser_pool)
            && let Some((kind, byte_range, message)) =
                action_to_drift(action, &loaded.baseline, &source, tree.as_ref())
        {
            drifts.push(DomainDrift {
                uri,
                kind,
                byte_range,
                message,
            });
        }
    }

    let drifts_arc = Arc::new(drifts);
    cache.store_drifts(
        project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        fingerprint,
        Arc::clone(&drifts_arc),
    );

    (*drifts_arc).clone()
}

fn loaded_state_with_cache(
    project_root: &Path,
    config_mtime: std::time::SystemTime,
    max_model_mtime: std::time::SystemTime,
    max_migration_mtime: std::time::SystemTime,
    cache: &DriftCache,
) -> Option<Arc<LoadedState>> {
    if let Some(hit) = cache.get(
        project_root,
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
    ) {
        return Some(hit);
    }

    let config =
        vespertide_loader::load_config_from_path(project_root.join("vespertide.json")).ok()?;
    let current_models =
        vespertide_loader::load_models_from_dir(Some(project_root.to_path_buf())).ok()?;
    let applied_plans =
        vespertide_loader::load_migrations_from_dir(Some(project_root.to_path_buf())).ok()?;
    let baseline = vespertide_planner::schema_from_plans(&applied_plans).ok()?;
    let models_dir = resolve_models_dir(project_root, &config);
    let loaded = Arc::new(LoadedState {
        config,
        current_models,
        baseline,
        models_dir,
    });
    cache.store(
        project_root.to_path_buf(),
        config_mtime,
        max_model_mtime,
        max_migration_mtime,
        Arc::clone(&loaded),
    );
    Some(loaded)
}

/// Compute drift across the entire workspace.
///
/// Returns an empty vector when `vespertide.json` is not found or any loader /
/// planner step fails. Drift diagnostics are best-effort and must never block
/// normal LSP feedback.
#[must_use]
pub fn compute(
    workspace_root: &Path,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
) -> Vec<DomainDrift> {
    static SHARED_CACHE: OnceLock<DriftCache> = OnceLock::new();
    compute_with_cache(
        workspace_root,
        index,
        docs,
        SHARED_CACHE.get_or_init(DriftCache::new),
    )
}

fn find_config_and_mtime(start: &Path) -> Option<(PathBuf, std::time::SystemTime)> {
    let mut current = if start.is_file() {
        start.parent()
    } else {
        Some(start)
    };

    while let Some(dir) = current {
        let candidate = dir.join("vespertide.json");
        if let Ok(meta) = std::fs::metadata(&candidate)
            && meta.is_file()
        {
            return Some((dir.to_path_buf(), meta.modified().ok()?));
        }
        current = dir.parent();
    }

    None
}

fn resolve_models_dir(root: &Path, config: &VespertideConfig) -> PathBuf {
    root.join(config.models_dir())
}

fn guess_uri(models_dir: &Path, table_name: &str) -> Option<Uri> {
    let mut path = models_dir.join(table_name);
    for ext in ["json", "yaml", "yml"] {
        path.set_extension(ext);
        if path.exists() {
            return path_to_uri(&path);
        }
    }
    None
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let mut path_text = path.to_string_lossy().replace('\\', "/");
    if !path_text.starts_with('/') {
        path_text = format!("/{path_text}");
    }
    Uri::from_str(&format!("file://{path_text}")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

    fn write_config(root: &std::path::Path) {
        fs::create_dir_all(root.join("models")).unwrap();
        fs::create_dir_all(root.join("migrations")).unwrap();
        fs::write(root.join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
    }

    fn user_model() -> &'static str {
        r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#
    }

    #[test]
    fn compute_returns_empty_when_loaded_state_fails() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("vespertide.json"), "not json").unwrap();

        let out = compute_with_cache(
            tmp.path(),
            &WorkspaceIndex::new(),
            &DocumentStore::new(),
            &DriftCache::new(),
        );

        assert!(out.is_empty());
    }

    #[test]
    fn compute_returns_empty_when_models_directory_load_fails() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("migrations")).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();

        let out = compute_with_cache(
            tmp.path(),
            &WorkspaceIndex::new(),
            &DocumentStore::new(),
            &DriftCache::new(),
        );

        assert!(out.is_empty());
    }

    #[test]
    fn compute_returns_empty_when_diff_validation_fails() {
        let tmp = tempdir().unwrap();
        write_config(tmp.path());
        fs::write(tmp.path().join("models/user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"id","type":"integer"}]}"#).unwrap();

        let out = compute_with_cache(
            tmp.path(),
            &WorkspaceIndex::new(),
            &DocumentStore::new(),
            &DriftCache::new(),
        );

        assert!(out.is_empty());
    }

    #[test]
    fn compute_returns_empty_when_diff_schemas_rejects_cyclic_creates() {
        let tmp = tempdir().unwrap();
        write_config(tmp.path());
        fs::write(tmp.path().join("models/a.json"), r#"{"name":"a","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"b_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"b","ref_columns":["id"]}}]}"#).unwrap();
        fs::write(tmp.path().join("models/b.json"), r#"{"name":"b","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"a_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"a","ref_columns":["id"]}}]}"#).unwrap();
        let models =
            vespertide_loader::load_models_from_dir(Some(tmp.path().to_path_buf())).unwrap();
        assert!(
            vespertide_planner::diff_schemas(&[], &models).is_err(),
            "fixture must reach the diff_schemas error path"
        );

        let out = compute_with_cache(
            tmp.path(),
            &WorkspaceIndex::new(),
            &DocumentStore::new(),
            &DriftCache::new(),
        );

        assert!(out.is_empty());
    }

    #[test]
    fn compute_skips_actions_when_index_uri_has_no_source() {
        let tmp = tempdir().unwrap();
        write_config(tmp.path());
        fs::write(tmp.path().join("models/user.json"), user_model()).unwrap();
        let pool = ParserPool::new();
        let tree = pool
            .parse(user_model(), crate::parser::DocumentFormat::Json)
            .unwrap();
        let index = WorkspaceIndex::new();
        let missing_uri = path_to_uri(&tmp.path().join("models/missing.json")).unwrap();
        index.upsert(&missing_uri, user_model(), &tree);

        let out = compute_with_cache(
            tmp.path(),
            &index,
            &DocumentStore::new(),
            &DriftCache::new(),
        );

        assert!(out.is_empty());
    }

    #[test]
    fn compute_skips_remap_enum_values_action_with_no_drift_mapping() {
        let tmp = tempdir().unwrap();
        write_config(tmp.path());
        fs::write(tmp.path().join("migrations/0001_init.vespertide.json"), r#"{"comment":null,"created_at":null,"version":1,"actions":[{"type":"create_table","table":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"priority","type":{"kind":"enum","name":"priority_level","values":[{"name":"low","value":0},{"name":"high","value":1}]},"nullable":false}],"constraints":[]}]}"#).unwrap();
        fs::write(tmp.path().join("models/user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"priority","type":{"kind":"enum","name":"priority_level","values":[{"name":"low","value":0},{"name":"high","value":2}]},"nullable":false}]}"#).unwrap();

        let out = compute_with_cache(
            tmp.path(),
            &WorkspaceIndex::new(),
            &DocumentStore::new(),
            &DriftCache::new(),
        );

        assert!(out.is_empty());
    }

    #[test]
    fn guess_uri_checks_yaml_yml_and_missing_extensions() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path()).unwrap();
        fs::write(tmp.path().join("user.yaml"), "name: user\ncolumns: []\n").unwrap();

        assert!(guess_uri(tmp.path(), "user").is_some());
        assert!(guess_uri(tmp.path(), "missing").is_none());
    }

    #[test]
    fn path_to_uri_prepends_leading_slash_for_relative_path() {
        // A relative path never starts with `/` on any platform, so this
        // exercises the leading-slash normalization branch deterministically
        // (absolute POSIX paths on CI would otherwise skip it).
        let uri = path_to_uri(std::path::Path::new("models/user.json")).expect("uri");
        assert!(
            uri.as_str().starts_with("file:///"),
            "got: {}",
            uri.as_str()
        );
        assert!(
            uri.as_str().ends_with("models/user.json"),
            "got: {}",
            uri.as_str()
        );
    }
}
