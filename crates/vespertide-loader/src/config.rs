use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vespertide_config::VespertideConfig;

/// Load vespertide.json config from current directory.
pub fn load_config() -> Result<VespertideConfig> {
    let path = PathBuf::from("vespertide.json");
    if !path.exists() {
        anyhow::bail!("vespertide.json not found. Run 'vespertide init' first.");
    }

    let content = fs::read_to_string(&path).context("read vespertide.json")?;
    let config: VespertideConfig =
        serde_json::from_str(&content).context("parse vespertide.json")?;
    Ok(config)
}

/// Load config from a specific path.
pub fn load_config_from_path(path: PathBuf) -> Result<VespertideConfig> {
    if !path.exists() {
        anyhow::bail!("vespertide.json not found at: {}", path.display());
    }

    let content = fs::read_to_string(&path).context("read vespertide.json")?;
    let config: VespertideConfig =
        serde_json::from_str(&content).context("parse vespertide.json")?;
    Ok(config)
}

/// Load config from project root, with fallback to defaults.
pub fn load_config_or_default(project_root: Option<PathBuf>) -> Result<VespertideConfig> {
    let config_path = if let Some(root) = project_root {
        root.join("vespertide.json")
    } else {
        PathBuf::from("vespertide.json")
    };

    if config_path.exists() {
        load_config_from_path(config_path)
    } else {
        Ok(VespertideConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

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

    fn write_config(path: &PathBuf) {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    #[serial]
    fn test_load_config_from_path_success() {
        let tmp = tempdir().unwrap();
        let config_path = tmp.path().join("vespertide.json");
        write_config(&config_path);

        let result = load_config_from_path(config_path);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.models_dir, PathBuf::from("models"));
    }

    #[test]
    #[serial]
    fn test_load_config_from_path_not_found() {
        let tmp = tempdir().unwrap();
        let config_path = tmp.path().join("nonexistent.json");

        let result = load_config_from_path(config_path.clone());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("vespertide.json not found at:"));
        assert!(err_msg.contains(&config_path.display().to_string()));
    }

    #[test]
    #[serial]
    fn test_load_config_or_default_with_root() {
        let tmp = tempdir().unwrap();
        let config_path = tmp.path().join("vespertide.json");
        write_config(&config_path);

        let result = load_config_or_default(Some(tmp.path().to_path_buf()));
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.models_dir, PathBuf::from("models"));
    }

    #[test]
    #[serial]
    fn test_load_config_or_default_without_root() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let config_path = PathBuf::from("vespertide.json");
        write_config(&config_path);

        let result = load_config_or_default(None);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.models_dir, PathBuf::from("models"));
    }

    #[test]
    #[serial]
    fn test_load_config_or_default_fallback_to_default() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let result = load_config_or_default(None);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.models_dir, PathBuf::from("models"));
    }

    #[test]
    #[serial]
    fn test_load_config_success() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let config_path = PathBuf::from("vespertide.json");
        write_config(&config_path);

        let result = load_config();
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.models_dir, PathBuf::from("models"));
    }

    #[test]
    #[serial]
    fn test_load_config_not_found() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let result = load_config();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("vespertide.json not found"));
    }
}
