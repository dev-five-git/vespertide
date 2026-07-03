//! Cache for the per-call `(config, current_models, baseline)` triple used
//! by `drift::compute`. Drift fires on every `did_change` debounce in a
//! typical editor session — and most of that work is wasted because the
//! input files have not changed.
//!
//! Loaded-state cache key:
//! `(project_root, config_mtime, max_model_mtime, max_migration_mtime)`.
//! Final drift cache key: the loaded-state key plus a fingerprint of all open
//! documents, because `DomainDrift::byte_range` depends on live tree positions.
//!
//! On a hit, the cached `LoadedState` is returned wrapped in `Arc` so the
//! caller does not pay the clone cost. On a miss (any mtime advances, or
//! `project_root` differs), the full load is repeated and the cache replaced.
//!
//! Cache is per-instance to keep tests isolated; the LSP backend holds
//! `Arc<DriftCache>` for the lifetime of the server.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use vespertide_config::VespertideConfig;
use vespertide_core::TableDef;

use super::DomainDrift;
pub(super) use crate::cache::docstore_fingerprint;
#[cfg(test)]
use crate::store::DocumentStore;

/// Per-instance cache for `drift::compute_with_cache`. Construct once on
/// the backend and reuse across calls.
#[derive(Debug, Default)]
pub struct DriftCache {
    state: Mutex<Option<CachedState>>,
    drifts: Mutex<Option<CachedDrifts>>,
}

#[derive(Debug)]
struct CachedState {
    project_root: PathBuf,
    config_mtime: SystemTime,
    max_model_mtime: SystemTime,
    max_migration_mtime: SystemTime,
    loaded: Arc<LoadedState>,
}

#[derive(Debug)]
struct CachedDrifts {
    project_root: PathBuf,
    config_mtime: SystemTime,
    max_model_mtime: SystemTime,
    max_migration_mtime: SystemTime,
    docstore_fingerprint: u64,
    drifts: Arc<Vec<DomainDrift>>,
}

#[derive(Debug)]
pub(super) struct LoadedState {
    pub(super) config: VespertideConfig,
    pub(super) current_models: Vec<TableDef>,
    pub(super) baseline: Vec<TableDef>,
    pub(super) models_dir: PathBuf,
}

impl DriftCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up by `project_root`. Returns the cached `Arc<LoadedState>` if
    /// every tracked mtime is unchanged; otherwise returns `None` and the
    /// caller is expected to populate via `store`.
    pub(super) fn get(
        &self,
        project_root: &Path,
        config_mtime: SystemTime,
        max_model_mtime: SystemTime,
        max_migration_mtime: SystemTime,
    ) -> Option<Arc<LoadedState>> {
        let state = self
            .state
            .lock()
            .expect("drift cache state lock poisoned — invariant: no panic while holding lock");
        let cached = state.as_ref()?;
        if cached.project_root == project_root
            && cached.config_mtime == config_mtime
            && cached.max_model_mtime == max_model_mtime
            && cached.max_migration_mtime == max_migration_mtime
        {
            return Some(Arc::clone(&cached.loaded));
        }
        None
    }

    /// Store freshly-loaded state. Atomically replaces any prior entry.
    pub(super) fn store(
        &self,
        project_root: PathBuf,
        config_mtime: SystemTime,
        max_model_mtime: SystemTime,
        max_migration_mtime: SystemTime,
        loaded: Arc<LoadedState>,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("drift cache state lock poisoned — invariant: no panic while holding lock");
        *state = Some(CachedState {
            project_root,
            config_mtime,
            max_model_mtime,
            max_migration_mtime,
            loaded,
        });
    }

    /// Look up the final drift vector by the loaded-state key plus the open
    /// document fingerprint. Returns the exact cached `Arc` on hit.
    pub(super) fn get_drifts(
        &self,
        project_root: &Path,
        config_mtime: SystemTime,
        max_model_mtime: SystemTime,
        max_migration_mtime: SystemTime,
        docstore_fingerprint: u64,
    ) -> Option<Arc<Vec<DomainDrift>>> {
        let drifts = self
            .drifts
            .lock()
            .expect("drift cache drifts lock poisoned — invariant: no panic while holding lock");
        let cached = drifts.as_ref()?;
        if cached.project_root == project_root
            && cached.config_mtime == config_mtime
            && cached.max_model_mtime == max_model_mtime
            && cached.max_migration_mtime == max_migration_mtime
            && cached.docstore_fingerprint == docstore_fingerprint
        {
            return Some(Arc::clone(&cached.drifts));
        }
        None
    }

    /// Store freshly-computed final drifts. Atomically replaces any prior
    /// final-result entry without touching the loaded-state cache.
    pub(super) fn store_drifts(
        &self,
        project_root: PathBuf,
        config_mtime: SystemTime,
        max_model_mtime: SystemTime,
        max_migration_mtime: SystemTime,
        docstore_fingerprint: u64,
        drifts: Arc<Vec<DomainDrift>>,
    ) {
        let mut slot = self
            .drifts
            .lock()
            .expect("drift cache drifts lock poisoned — invariant: no panic while holding lock");
        *slot = Some(CachedDrifts {
            project_root,
            config_mtime,
            max_model_mtime,
            max_migration_mtime,
            docstore_fingerprint,
            drifts,
        });
    }

    /// Test-only and ad-hoc invalidation. Clears the drifts cache without
    /// touching the loaded-state cache.
    pub fn invalidate_drifts(&self) {
        *self
            .drifts
            .lock()
            .expect("drift cache drifts lock poisoned — invariant: no panic while holding lock") =
            None;
    }

    /// Test-only: clear the cache.
    #[cfg(test)]
    pub(super) fn clear(&self) {
        *self
            .state
            .lock()
            .expect("drift cache state lock poisoned — invariant: no panic while holding lock") =
            None;
    }
}

/// Walk `dir` (non-recursive — drift only loads files in the top level)
/// returning the maximum file mtime found, or `SystemTime::UNIX_EPOCH` if
/// the directory does not exist or is empty.
pub(super) fn max_mtime_in_dir(dir: &Path) -> SystemTime {
    let mut max = SystemTime::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return max;
    };
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && let Ok(mtime) = meta.modified()
            && mtime > max
        {
            max = mtime;
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::super::DriftKind;
    use super::*;
    use std::fs;
    use std::str::FromStr;
    use tempfile::tempdir;
    use tower_lsp_server::ls_types::Uri;

    fn dummy_drift() -> DomainDrift {
        DomainDrift {
            uri: Uri::from_str("file:///p.json").unwrap(),
            kind: DriftKind::RawSql,
            byte_range: None,
            message: "dummy".to_string(),
        }
    }

    #[test]
    fn cache_hit_returns_arc_clone() {
        let cache = DriftCache::new();
        let now = SystemTime::now();
        let state = Arc::new(LoadedState {
            config: VespertideConfig::default(),
            current_models: vec![],
            baseline: vec![],
            models_dir: PathBuf::from("/tmp"),
        });

        cache.store(PathBuf::from("/proj"), now, now, now, Arc::clone(&state));

        let hit = cache.get(Path::new("/proj"), now, now, now).expect("hit");
        assert!(Arc::ptr_eq(&state, &hit), "should return same Arc");

        cache.clear();
        assert!(cache.get(Path::new("/proj"), now, now, now).is_none());
    }

    #[test]
    fn cache_miss_on_config_mtime_change() {
        let cache = DriftCache::new();
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + std::time::Duration::from_secs(1);
        let state = Arc::new(LoadedState {
            config: VespertideConfig::default(),
            current_models: vec![],
            baseline: vec![],
            models_dir: PathBuf::from("/tmp"),
        });
        cache.store(PathBuf::from("/proj"), t0, t0, t0, state);

        assert!(
            cache.get(Path::new("/proj"), t1, t0, t0).is_none(),
            "config mtime advance must miss"
        );
        assert!(
            cache.get(Path::new("/proj"), t0, t1, t0).is_none(),
            "model mtime advance must miss"
        );
        assert!(
            cache.get(Path::new("/proj"), t0, t0, t1).is_none(),
            "migration mtime advance must miss"
        );
    }

    #[test]
    fn cache_miss_on_different_project_root() {
        let cache = DriftCache::new();
        let now = SystemTime::now();
        let state = Arc::new(LoadedState {
            config: VespertideConfig::default(),
            current_models: vec![],
            baseline: vec![],
            models_dir: PathBuf::from("/tmp"),
        });
        cache.store(PathBuf::from("/proj_a"), now, now, now, state);

        assert!(cache.get(Path::new("/proj_b"), now, now, now).is_none());
    }

    #[test]
    fn max_mtime_in_dir_returns_epoch_for_missing() {
        let mtime = max_mtime_in_dir(Path::new("/this/does/not/exist"));
        assert_eq!(mtime, SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn max_mtime_in_dir_picks_newest() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("a.json"), "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(tmp.path().join("b.json"), "{}").unwrap();

        let max = max_mtime_in_dir(tmp.path());
        let a = std::fs::metadata(tmp.path().join("a.json"))
            .unwrap()
            .modified()
            .unwrap();
        let b = std::fs::metadata(tmp.path().join("b.json"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(max, std::cmp::max(a, b));
    }

    #[test]
    fn drifts_cache_hit_returns_arc() {
        let cache = DriftCache::new();
        let now = SystemTime::now();
        let drifts = Arc::new(vec![dummy_drift()]);
        cache.store_drifts(PathBuf::from("/p"), now, now, now, 42, Arc::clone(&drifts));

        let hit = cache
            .get_drifts(Path::new("/p"), now, now, now, 42)
            .expect("hit");

        assert!(Arc::ptr_eq(&drifts, &hit));
    }

    #[test]
    fn drifts_cache_miss_on_different_fingerprint() {
        let cache = DriftCache::new();
        let now = SystemTime::now();
        cache.store_drifts(PathBuf::from("/p"), now, now, now, 42, Arc::new(vec![]));

        assert!(
            cache
                .get_drifts(Path::new("/p"), now, now, now, 43)
                .is_none(),
            "different fingerprint must miss"
        );
    }

    #[test]
    fn drifts_cache_miss_on_mtime_change() {
        let cache = DriftCache::new();
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + std::time::Duration::from_secs(1);
        cache.store_drifts(PathBuf::from("/p"), t0, t0, t0, 42, Arc::new(vec![]));

        assert!(cache.get_drifts(Path::new("/p"), t1, t0, t0, 42).is_none());
        assert!(cache.get_drifts(Path::new("/p"), t0, t1, t0, 42).is_none());
        assert!(cache.get_drifts(Path::new("/p"), t0, t0, t1, 42).is_none());
    }

    #[test]
    fn invalidate_drifts_clears_only_drifts_cache() {
        let cache = DriftCache::new();
        let now = SystemTime::now();
        let state = Arc::new(LoadedState {
            config: VespertideConfig::default(),
            current_models: vec![],
            baseline: vec![],
            models_dir: PathBuf::from("/tmp"),
        });
        cache.store(PathBuf::from("/p"), now, now, now, Arc::clone(&state));
        cache.store_drifts(PathBuf::from("/p"), now, now, now, 42, Arc::new(vec![]));

        cache.invalidate_drifts();

        assert!(cache.get(Path::new("/p"), now, now, now).is_some());
        assert!(
            cache
                .get_drifts(Path::new("/p"), now, now, now, 42)
                .is_none()
        );
    }

    #[test]
    fn docstore_fingerprint_changes_on_text_change() {
        let docs = DocumentStore::new();
        let uri = Uri::from_str("file:///a.json").unwrap();
        docs.open(uri.clone(), "json".to_string(), 1, "{}".to_string());
        let fp1 = docstore_fingerprint(&docs);

        docs.update_full(&uri, r#"{"name":"x"}"#.to_string(), 2);
        let fp2 = docstore_fingerprint(&docs);

        assert_ne!(fp1, fp2);
    }
}
