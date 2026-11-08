# Comment Annotations

Finds annotations inside code comments (`NOTE:`, `TODO(user):`, `=NOTE=`)
and inline emphasis (`**bold**`, `*italic*`, `` `code` ``) and publishes
each one as **semantic tokens** (styling), **diagnostics** (project list),
and **workspace symbols** (jump-to) via one LSP server, plus a CLI for CI
and coding agents.

The server attaches to languages Zed already has (`C`, `C++`, `Rust`,
`Python`, `Go`, `JavaScript`, `TypeScript`, `TSX` via tree-sitter; `HTML`,
`CSS`, `JSON` via the delimiter fallback) — it never needs its own grammar.

## Build

```sh
cargo build --release
```

Binary: `target/release/canc`. `cargo run -p xtask -- build` builds both
the binary and the Zed extension WASM into `target/dist/`.

## Install & run

### CLI / LSP server (cargo)

```sh
cargo install canc   # installs `canc` into ~/.cargo/bin
canc serve                              # LSP over stdio (what Zed launches)
canc anchors path/to/file.rs            # one-shot CLI scan
```

From source, the same binary is at `target/release/canc`.

### Zed

Point Zed at the binary with:

```json
// ~/.config/zed/settings.json
{
  "lsp": {
    "canc": {
      "binary": { "path": "/home/you/.cargo/bin/canc" }
    }
  },
  "languages": {
    "Rust": { "language_servers": ["rust-analyzer", "canc", "..."] }
  }
}
```

Then add the per-class colour rules from `canc init-zed` under
`"global_lsp_settings": { "semantic_token_rules": […] }`.

The bundled Zed extension (`extension/`, package `canc-zed`, its own Cargo
workspace) resolves the binary in this order: user `binary.path` override →
`canc` on `PATH` → `cargo install canc` (if cargo is
available) → GitHub release download fallback. To install the extension in
Zed as a dev extension, use `extension/` as the source directory; build its
WASM with `cargo build --target wasm32-wasip2 --release` inside `extension/`.

## CLI

```text
canc anchors [PATH...] [--format text|json|ndjson] [--tag NOTE,FIXME] [--deny FIXME,BUG] [--spans] [--no-ignore]
canc serve          # LSP over stdio
canc init-zed       # print semantic_token_rules for Zed settings
canc config check [PATH]
canc config print [PATH]
```

Exit codes for `anchors`: `0` clean, `1` findings under `--deny`,
`2` unreadable input or invalid config.

```text
$ canc anchors main.cpp
main.cpp:3:4  NOTE  This is how you start a c++ file
```

`--format json` emits (columns are 1-based, `end_column` is the 0-based
exclusive end — matching the worked example in the design doc):

```json
{"file":"main.cpp","line":3,"column":4,"end_column":10,"tag":"NOTE","class":"note","form":"delimited","meta":null,"text":"This is how you start a c++ file"}
```

`--spans` additionally includes inline spans — attached to their anchor when
they share a line, and as `SPAN` records (`tag: "SPAN"`, `class: "emphasis"`
etc.) when they sit on an otherwise tag-free line.

## Zed setup

```json
{
  "semantic_tokens": "combined",
  "global_lsp_settings": {
    "semantic_token_rules": [ ... from `canc init-zed` ... ]
  }
}
```

`init-zed` generates those rules from `anchors.toml`, keeping
the two files in sync. Tags are visible even with no rules (the `keyword`
default); rules only refine color/weight per class.

> [!NOTE]
> The bundled `comment` language (`extension/languages/comment/` + a small
> `tree-sitter-comment` grammar) is a **bonus only**: Zed extensions can't
> define injections into built-in languages, so it fires only where a host
> language already injects a language named `comment`. Your primary
> highlighter is the LSP semantic-token path below, which needs no grammar.
> You do **not** need the separate *Comments Highlighter*
> (`thedadams/zed-comment`) extension.

If you customised your server list, keep this server **last** — Zed gives
later servers precedence on overlapping tokens:

```json
{
  "languages": {
    "C++": { "language_servers": ["clangd", "canc", "..."] }
  }
}
```

`initializationOptions` (optional): `configPath` (explicit config, used
when a document has no on-disk project config),
`globalConfigPath` (overrides the standard `~/.config/canc/anchors.toml`
lookup), `logLevel`. Project `anchors.toml` files are always discovered by
walking up from each file and layered on top of the global config; changing
one triggers a refresh on the next `didChangeConfiguration`.

### Outline panel

The server implements `textDocument/documentSymbol` (one symbol per anchor),
so you can show anchors in the outline/breadcrumb panel. That panel defaults
to tree-sitter `outline.scm` (`"document_symbols": "off"`); opt in per
language to use the server instead (merged with clangd/rust-analyzer symbols):

```json
{
  "languages": {
    "C++": { "document_symbols": "on" },
    "Rust": { "document_symbols": "on" }
  }
}
```

Without this, anchors are still available in `workspace/symbol` search
(`Cmd+Shift+O`) and via the CLI.

## Config (`anchors.toml`)

```toml
version = 1

[syntax]
conventional = true          # NOTE: text   TODO(user): text
delimited    = true          # =NOTE= text  =NOTE=: text
inline       = "all"         # "all" | "tagged" | "off"
emphasis     = "commonmark"  # or "star-bold"
unknown_tags = "ignore"      # or "highlight"

[[tags]]                     # additions/overrides, merged by name with
name = "NOTE"                # the built-in NOTE/TODO/FIXME/BUG/HACK set
class = "note"               # semantic-token modifier
severity = "off"             # off | hint | information | warning | error
symbol = true

[[overrides]]
paths = ["vendor/**", "third_party/**"]
enabled = false
```

Global path: `~/.config/canc/anchors.toml`.
Project config is found by walking up from each file.

## Tests

```sh
cargo test
canc anchors tests/golden --format json
```
