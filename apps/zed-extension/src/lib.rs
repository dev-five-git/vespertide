use std::fs;
use zed_extension_api::{self as zed, LanguageServerId, Result};

struct VespertideExtension {
    cached_binary_path: Option<String>,
}

impl zed::Extension for VespertideExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // 1. PATH lookup (for `cargo install` users or local dev).
        // On Windows, `worktree.which()` does NOT auto-append `.exe`, so try
        // platform-appropriate names in order.
        let (os, _arch) = zed::current_platform();
        let candidates: &[&str] = match os {
            zed::Os::Windows => &["vespertide-lsp.exe", "vespertide-lsp"],
            _ => &["vespertide-lsp"],
        };
        for name in candidates {
            if let Some(path) = worktree.which(name) {
                return Ok(zed::Command {
                    command: path,
                    args: vec![],
                    env: Default::default(),
                });
            }
        }

        // 2. Cached binary from a previous download.
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|m| m.is_file()) {
                return Ok(zed::Command {
                    command: path.clone(),
                    args: vec![],
                    env: Default::default(),
                });
            }
        }

        // 3. Download from GitHub Releases.
        let binary_path = self.install_language_server(language_server_id)?;
        self.cached_binary_path = Some(binary_path.clone());

        Ok(zed::Command {
            command: binary_path,
            args: vec![],
            env: Default::default(),
        })
    }
}

impl VespertideExtension {
    fn install_language_server(&self, id: &LanguageServerId) -> Result<String> {
        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            "dev-five-git/vespertide",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (os, arch) = zed::current_platform();
        let asset_name = format!(
            "vespertide-lsp-{os}-{arch}.tar.gz",
            os = match os {
                zed::Os::Mac => "darwin",
                zed::Os::Linux => "linux",
                zed::Os::Windows => "windows",
            },
            arch = match arch {
                zed::Architecture::Aarch64 => "aarch64",
                zed::Architecture::X8664 => "x86_64",
                zed::Architecture::X86 => "x86",
            }
        );

        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| format!("No asset found for platform: {asset_name}"))?;

        let version_dir = format!("vespertide-lsp-{}", release.version);
        let binary_path = format!(
            "{version_dir}/vespertide-lsp{ext}",
            ext = if matches!(os, zed::Os::Windows) {
                ".exe"
            } else {
                ""
            }
        );

        if !fs::metadata(&binary_path).is_ok_and(|m| m.is_file()) {
            zed::set_language_server_installation_status(
                id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &version_dir,
                zed::DownloadedFileType::GzipTar,
            )?;

            zed::make_file_executable(&binary_path)?;

            // Clean up old version directories.
            if let Ok(entries) = fs::read_dir(".") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("vespertide-lsp-") && name_str != version_dir {
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
        }

        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::None,
        );

        Ok(binary_path)
    }
}

zed::register_extension!(VespertideExtension);

#[cfg(test)]
mod tests {
    #[test]
    fn asset_name_format_matches_release_convention() {
        // Smoke test the format string; actual matching happens at runtime.
        let asset_name = format!("vespertide-lsp-{}-{}.tar.gz", "linux", "x86_64");
        assert_eq!(asset_name, "vespertide-lsp-linux-x86_64.tar.gz");
    }

    #[test]
    fn asset_name_format_windows_arm() {
        let asset_name = format!("vespertide-lsp-{}-{}.tar.gz", "windows", "aarch64");
        assert_eq!(asset_name, "vespertide-lsp-windows-aarch64.tar.gz");
    }
}
