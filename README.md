# Comment Annotations

Finds annotations inside code comments (`NOTE:`, `TODO(user):`, `=NOTE=`)
and inline emphasis (`**bold**`, `*italic*`, `` `code` ``) and publishes
each one as **semantic tokens** (styling), **diagnostics** (project list),
and **workspace symbols** (jump-to) via one LSP server, plus a CLI for CI
and coding agents.

The server attaches to languages Zed already has (`C`, `C++`, `Rust`,
`Python`, `Go`, `JavaScript`, `TypeScript`, `TSX`) — it never needs its own
grammar.

## Build

```sh
cargo build --release
```

Binary: `target/release/canc`

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

If you customised your server list, keep this server **last** — Zed gives
later servers precedence on overlapping tokens:

```json
{
  "languages": {
    "C++": { "language_servers": ["clangd", "canc", "..."] }
  }
}
```

`initializationOptions` (optional): `configPath`, `globalConfigPath`,
`checkWhileTyping`, `logLevel`.

## Config (`anchors.toml`)

```toml
version = 1

[syntax]
conventional = true          # NOTE: text   TODO(user): text
delimited    = true          # =NOTE= text  =NOTE=: text
inline       = "all"         # "all" | "tagged" | "off"
emphasis     = "commonmark"  # or "star-bold"
unknown_tags = "ignore"      # or "highlight"

[[tags]]
name = "NOTE"
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
