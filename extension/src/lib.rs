use zed_extension_api::{self as zed, LanguageServerId, Worktree};

struct CommentAnnotationsExtension {
    cached_binary_path: Option<String>,
}

impl zed::Extension for CommentAnnotationsExtension {
    fn new() -> Self {
        Self { cached_binary_path: None }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> zed::Result<zed::Command> {
        Ok(zed::Command {
            command: self.resolve_binary(language_server_id, worktree)?,
            args: vec!["serve".into()],
            env: vec![],
        })
    }

    fn language_server_initialization_options(
        &mut self,
        server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        let settings = zed::settings::LspSettings::for_worktree(server_id.as_ref(), worktree)?;
        Ok(settings.initialization_options)
    }
}

fn home_bin_dirs() -> Vec<std::path::PathBuf> {
    let mut buf = vec![];
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            for sub in [".local/bin", ".cargo/bin", ".opencode/bin", ".mimocode/bin"] {
                buf.push(std::path::PathBuf::from(&home).join(sub));
            }
            // Nix user profile (NixOS / home-manager): binary dirs that are
            // often not in Zed's captured login-shell PATH.
            buf.push(std::path::PathBuf::from(&home).join(".nix-profile/bin"));
            buf.push(std::path::PathBuf::from(&home).join("nix-profile/bin"));
            buf.push(std::path::PathBuf::from(&home).join(".local/state/nix/profile/bin"));
        }
    }
    for sub in ["/home/maikel/.local/bin", "/home/maikel/.cargo/bin"] {
        buf.push(std::path::PathBuf::from(sub));
    }
    buf
}

fn bin_dir_contains(dir: &std::path::PathBuf, name: &str) -> Option<String> {
    let candidate = dir.join(name);
    if candidate.exists() {
        Some(candidate.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Where `cargo install` puts a newly installed binary, derived strictly
/// from the environment — not from `PATH`. This is the deterministic path
/// we return after a successful install, so we never need to find it again
/// via `which()` (which shares the same broken desktop PATH).
fn cargo_home_bin() -> Option<String> {
    let exe_name = if std::env::consts::OS == "windows" {
        "canc.exe"
    } else {
        "canc"
    };
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        if !cargo_home.is_empty() {
            let p = std::path::PathBuf::from(&cargo_home)
                .join("bin")
                .join(exe_name);
            return Some(p.to_string_lossy().to_string());
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            let p = std::path::PathBuf::from(&home)
                .join(".cargo")
                .join("bin")
                .join(exe_name);
            return Some(p.to_string_lossy().to_string());
        }
    }
    // Best-effort fallback for machines where the sandbox hides $HOME.
    match std::env::consts::OS {
        "windows" => Some(format!(
            "C:\\Users\\{}\\AppData\\Local\\.cargo\\bin\\{}",
            std::env::var("USERNAME").unwrap_or_else(|_| "user".into()),
            exe_name
        )),
        _ => Some(format!("/home/maikel/.cargo/bin/{}", exe_name)),
    }
}

impl CommentAnnotationsExtension {
    fn resolve_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> zed::Result<String> {
        // Step 0: honour explicit user override from Zed LSP settings:
        //   { "lsp": { "canc": { "binary": { "path": "/path/to/binary" } } } }
        if let Ok(lsp_settings) =
            zed::settings::LspSettings::for_worktree(language_server_id.as_ref(), worktree)
        {
            if let Some(binary) = lsp_settings.binary {
                if let Some(path) = binary.path {
                    eprintln!("canc: using user-configured binary: {path}");
                    return Ok(path);
                }
            }
        }

        if let Some(path) = &self.cached_binary_path {
            if std::path::Path::new(path).exists() {
                return Ok(path.clone());
            }
            self.cached_binary_path = None;
        }
        if let Some(path) = worktree.which("canc") {
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        // Direct filesystem lookup in per-user bin dirs. Zed's `which`
        // only searches the PATH captured from the desktop login shell,
        // which on NixOS/home-manager often lacks ~/.local/bin and
        // ~/.cargo/bin even though interactive shells have them.
        for dir in home_bin_dirs() {
            if let Some(path) = bin_dir_contains(&dir, "canc") {
                eprintln!("canc: found at {path}");
                self.cached_binary_path = Some(path.clone());
                return Ok(path);
            }
        }

        // Cargo-install fallback: if `cargo` is on PATH, try installing the
        // server binary. Prefer crates.io once committed, then git main.
        // This makes local use work without a GitHub release — typically
        // `cargo install canc`.
        if worktree.which("cargo").is_some() {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            let install = zed::process::Command::new("cargo")
                .arg("install")
                .arg("canc")
                .output();
            match install {
                Ok(output) if output.status == Some(0) => {
                    // Deterministic: cargo put it in its CARGO_HOME/bin
                    // regardless of which() and Zed's PATH.
                    if let Some(deterministic) = cargo_home_bin() {
                        eprintln!("canc: cargo install -> {deterministic}");
                        self.cached_binary_path = Some(deterministic.clone());
                        return Ok(deterministic);
                    }
                    if let Some(path) = worktree.which("canc") {
                        self.cached_binary_path = Some(path.clone());
                        return Ok(path);
                    }
                    for dir in home_bin_dirs() {
                        if let Some(path) = bin_dir_contains(&dir, "canc") {
                            let path = path.clone();
                            self.cached_binary_path = Some(path.clone());
                            return Ok(path);
                        }
                    }
                }
                Ok(output) => {
                    eprintln!(
                        "canc: `cargo install canc` exited with {:?}; stderr: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                Err(err) => {
                    eprintln!("canc: cargo install failed to spawn: {err}");
                }
            }
            // Fall through to the GitHub-release path below if the install
            // failed or the binary still isn't visible on PATH.
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
        }

        // Download fallback: use the pinned release asset. Adjust REPO and
        // VERSION when publishing. The (os, arch) mapping follows
        // codebook-zed / zed-todo-highlight (musl for Linux).
        const REPO: &str = "maikel-479/Anchors";
        const VERSION: &str = "0.1.0";
        let (os, arch) = zed::current_platform();
        let (target, exe_suffix) = match (os, arch) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => ("aarch64-apple-darwin", ""),
            (zed::Os::Mac, zed::Architecture::X8664) => ("x86_64-apple-darwin", ""),
            (zed::Os::Linux, zed::Architecture::Aarch64) => ("aarch64-unknown-linux-musl", ""),
            (zed::Os::Linux, zed::Architecture::X8664) => ("x86_64-unknown-linux-musl", ""),
            (zed::Os::Windows, zed::Architecture::X8664) => ("x86_64-pc-windows-msvc", ".exe"),
            (zed::Os::Windows, zed::Architecture::Aarch64) => ("aarch64-pc-windows-msvc", ".exe"),
            _ => {
                return Err(format!(
                    "canc has no pre-built binary for {os:?}/{arch:?}. \
                     Build from source and put `canc` on PATH."
                ))
            }
        };
        let (archive_ext, file_type) = if exe_suffix.is_empty() {
            (".tar.gz", zed::DownloadedFileType::GzipTar)
        } else {
            (".zip", zed::DownloadedFileType::Zip)
        };
        let asset_name = format!("canc-{target}{archive_ext}");
        let version_dir = format!("bin/canc-{target}-v{VERSION}");
        let binary_path = format!("{version_dir}/canc{exe_suffix}");
        if !std::path::Path::new(&binary_path).exists() {
            let release = zed::github_release_by_tag_name(REPO, &format!("v{VERSION}"))
                .map_err(|e| {
                    format!(
                        "canc binary not found: checked PATH via worktree.which({checks}), ~/.local/bin, ~/.cargo/bin and other per-user bin dirs; GitHub release download failed ({e}). \
                         Run `cargo install canc` or put `canc` in ~/.local/bin. \
                         Or set \"lsp\": {{ \"canc\": {{ \"binary\": {{ \"path\": \"/home/maikel/.local/bin/canc\" }} }} }}",
                        checks = {
                            let dirs = home_bin_dirs();
                            let checked: Vec<String> = dirs
                                .iter()
                                .map(|d| {
                                    let name = match std::env::consts::OS {
                                        "windows" => "canc.exe",
                                        _ => "canc",
                                    };
                                    let path = d.join(name);
                                    format!("<{}>", path.display())
                                })
                                .collect();
                            checked.join(", ")
                        },
                    )
                })?;
            let asset = release
                .assets
                .iter()
                .find(|a| a.name == asset_name)
                .ok_or_else(|| format!("Release asset {asset_name} not found"))?;
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&asset.download_url, &version_dir, file_type)
                .map_err(|e| format!("Download failed: {e}"))?;
            zed::make_file_executable(&binary_path)
                .map_err(|e| format!("Failed to make {binary_path} executable: {e}"))?;
            zed::set_language_server_installation_status(language_server_id, &zed::LanguageServerInstallationStatus::None);
        }
        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

zed::register_extension!(CommentAnnotationsExtension);
