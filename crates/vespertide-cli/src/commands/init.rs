use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use vespertide_config::VespertideConfig;

pub fn cmd_init() -> Result<()> {
    let path = PathBuf::from("vespertide.json");
    if path.exists() {
        bail!("vespertide.json already exists");
    }

    let config = VespertideConfig::default();
    let json = serde_json::to_string_pretty(&config).context("serialize default config")?;
    fs::write(&path, json).context("write vespertide.json")?;
    println!("created {:?}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &PathBuf) -> Self {
            let original = env::current_dir().unwrap();
            env::set_current_dir(dir).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    #[test]
    #[serial_test::serial]
    fn cmd_init_creates_config() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        cmd_init().unwrap();
        assert!(PathBuf::from("vespertide.json").exists());
    }

    #[test]
    #[serial_test::serial]
    fn cmd_init_fails_when_exists() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        cmd_init().unwrap();
        let err = cmd_init().unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }
}
