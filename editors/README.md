# Editor snippets

The `comment-annotations` binary speaks LSP over stdio (`serve`) and also
provides a CLI (`anchors`). All editors share the same `scan()` in core.

- Neovim: start the server with `comment-annotations serve` (see `editors/neovim/comment-annotations.lua`).
- Helix: see `editors/helix/languages.toml` snippet.
- VS Code: see `editors/vscode/settings.json` snippet.

Colors are *not* set here. For Zed, run `comment-annotations init-zed` and
paste the output into `global_lsp_settings.semantic_token_rules`.
