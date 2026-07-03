//! Helpers for `workspace/didChangeWatchedFiles`. The notification
//! payload is just a list of `(uri, kind)` changes — the only thing
//! this module decides is whether a given path falls under our
//! `models/` or `migrations/` directories so we can ignore unrelated
//! filesystem noise (build artefacts, dotfiles, sibling projects).
//!
//! Decision logic ([`should_refresh_for`]) is unit-testable in
//! isolation; the LSP `did_change_watched_files` handler lives on
//! [`Backend`](crate::Backend) and delegates here.

use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::{
    DidChangeWatchedFilesRegistrationOptions, FileSystemWatcher, GlobPattern, Registration,
    WatchKind,
};

/// Build the LSP dynamic registration payload used in `initialized()`
/// to ask the client to watch every model / migration file under the
/// workspace. The id is stable so the server can `unregisterCapability`
/// later if needed (not currently used).
#[must_use]
pub fn build_registration() -> Registration {
    let opts = DidChangeWatchedFilesRegistrationOptions {
        watchers: vec![FileSystemWatcher {
            glob_pattern: GlobPattern::String(
                "**/{models,migrations}/**/*.{json,yaml,yml}".to_string(),
            ),
            kind: Some(WatchKind::all()),
        }],
    };
    let value = serde_json::to_value(opts)
        .expect("DidChangeWatchedFilesRegistrationOptions is always serialisable");
    Registration {
        id: "vespertide/watched-files".to_string(),
        method: "workspace/didChangeWatchedFiles".to_string(),
        register_options: Some(value),
    }
}

/// Returns the canonical (lowercase on Windows) path of `path` relative
/// to `root`. Falls back to a slash-normalised lossy form.
fn canonical(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let lossy = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        PathBuf::from(lossy.to_lowercase())
    } else {
        PathBuf::from(lossy)
    }
}

/// True when `changed_path` lives somewhere under `root/models_dir` or
/// `root/migrations_dir`. The match is **prefix-based on normalised
/// paths**, which is robust against Windows drive-letter casing and
/// trailing slashes — the same normalisation
/// `Backend::collect_workspace_tables` uses to dedup workspace entries.
#[must_use]
pub fn should_refresh_for(
    root: &Path,
    models_dir: &Path,
    migrations_dir: &Path,
    changed_path: &Path,
) -> bool {
    let _ = root; // included for API symmetry / future use
    let changed = canonical(changed_path);
    let models_norm = canonical(models_dir);
    let migrations_norm = canonical(migrations_dir);

    changed.starts_with(&models_norm) || changed.starts_with(&migrations_norm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn changes_under_models_trigger_refresh() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let models = root.join("models");
        let migrations = root.join("migrations");
        fs::create_dir_all(&models).unwrap();
        fs::create_dir_all(&migrations).unwrap();

        let changed = models.join("user.json");
        fs::write(&changed, "{}").unwrap();
        assert!(should_refresh_for(root, &models, &migrations, &changed));
    }

    #[test]
    fn changes_under_migrations_trigger_refresh() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let models = root.join("models");
        let migrations = root.join("migrations");
        fs::create_dir_all(&models).unwrap();
        fs::create_dir_all(&migrations).unwrap();

        let changed = migrations.join("0001_init.json");
        fs::write(&changed, "{}").unwrap();
        assert!(should_refresh_for(root, &models, &migrations, &changed));
    }

    #[test]
    fn changes_outside_tracked_dirs_are_ignored() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let models = root.join("models");
        let migrations = root.join("migrations");
        fs::create_dir_all(&models).unwrap();
        fs::create_dir_all(&migrations).unwrap();

        let unrelated = root.join("README.md");
        fs::write(&unrelated, "irrelevant").unwrap();
        assert!(!should_refresh_for(root, &models, &migrations, &unrelated));

        let sibling = root.parent().unwrap_or(root).join("other_file.txt");
        // Don't actually create — assertion only needs the path check.
        assert!(!should_refresh_for(root, &models, &migrations, &sibling));
    }

    #[test]
    fn nested_subdirectories_are_also_tracked() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let models = root.join("models");
        let migrations = root.join("migrations");
        fs::create_dir_all(models.join("nested")).unwrap();
        fs::create_dir_all(&migrations).unwrap();

        let changed = models.join("nested").join("nested.json");
        fs::write(&changed, "{}").unwrap();
        assert!(should_refresh_for(root, &models, &migrations, &changed));
    }

    #[test]
    fn build_registration_has_expected_shape() {
        let reg = build_registration();

        assert_eq!(reg.id, "vespertide/watched-files");
        assert_eq!(reg.method, "workspace/didChangeWatchedFiles");
        let opts = reg.register_options.expect("register_options present");
        let opts_str = opts.to_string();
        assert!(
            opts_str.contains("models") && opts_str.contains("migrations"),
            "expected glob to include both dirs, got: {opts_str}"
        );
    }

    #[test]
    fn should_refresh_handles_missing_tracked_dirs() {
        let tmp = tempdir().unwrap();
        let unrelated = tmp.path().join("other.txt");

        assert!(!should_refresh_for(
            tmp.path(),
            &tmp.path().join("models"),
            &tmp.path().join("migrations"),
            &unrelated
        ));
    }
}
