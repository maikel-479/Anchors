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
        // Download fallback: use the pinned release asset. Adjust owner/repo
        // and version when publishing. The (os, arch) mapping follows
        // codebook-zed / zed-todo-highlight (musl for Linux).
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
            let release = zed::github_release_by_tag_name("example/comment-annotations", &format!("v{VERSION}"))
                .map_err(|e| {
                    format!(
                        "canc binary not found on PATH and no release v{VERSION} is published yet.\
                         ~/.local/bin/canc is not on Zed's PATH.\
                         Either put it on PATH or set \"lsp\": {{ \"canc\": {{ \"binary\": {{ \"path\": \"/home/maikel/.local/bin/canc\" }} }} }} ({e})"
                    )
                })?;
            let asset = release
                .assets
                .iter()
                .find(|a| a.name == asset_name)
                .ok_or_else(|| format!("Release asset {asset_name} not found"))?;
            zed::download_file(&asset.download_url, &version_dir, file_type)
                .map_err(|e| format!("Download failed: {e}"))?;
        }
        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

zed::register_extension!(CommentAnnotationsExtension);
