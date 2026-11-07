# Editor snippets

The `canc` binary speaks LSP over stdio (`serve`) and also
provides a CLI (`anchors`). All editors share the same `scan()` in core.

- Neovim: start the server with `canc serve` (see `editors/neovim/canc.lua`).
- Helix: see `editors/helix/languages.toml` snippet.
- VS Code: see `editors/vscode/settings.json` snippet.

Colors are *not* set here. For Zed, run `canc init-zed` and
paste the output into `global_lsp_settings.semantic_token_rules`.
