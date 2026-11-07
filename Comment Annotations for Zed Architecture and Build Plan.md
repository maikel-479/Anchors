# Comment Annotations for Zed: Architecture and Build Plan

*Working title `<name>` · design draft, 6 Nov 2026 · status: pre-spike, nothing built yet*

## 1. Thesis

Build one Rust core that finds annotations inside code comments (`NOTE:`, `=FIXME=`, inline `**bold**`) and ship it as a language server, a CLI, and a thin Zed extension. The server attaches to languages Zed already has, so it never needs to own a grammar. It publishes each annotation three ways: semantic tokens for styling, diagnostics for a project-wide list, and workspace symbols for jump-to navigation.

Highlighting is the presentation layer. The substance is anchors as structured data that the editor, a CLI, CI, and coding agents can all read.

This replaces the earlier idea of injecting Markdown into comments. Injections live inside the host language's definition, and an extension cannot add them to languages it does not own. A language server has no such restriction.

**Alternatives considered**

- **Tree-sitter injection per host language.** Only works for languages whose repo you can PR into (Zed core for built-ins). No per-user styling, and nothing for a CLI.
- **Fork or reuse a `comment` grammar.** Tags get theme captures, so colors come from the theme, not from user config. No diagnostics or anchor list.
- **Upstream feature request.** A useful complement later, not a substitute: it cannot deliver the config and CLI story.

## 2. Scope

**In v1**

- Comment tags in two forms: conventional (`NOTE:`, `TODO(user):`) and delimited (`=NOTE=`, `=NOTE=:`).
- Inline emphasis inside any comment: strong, emphasis, code span.
- Per-tag class, diagnostic severity, and symbol visibility, set in TOML.
- Semantic tokens, diagnostics, and `workspace/symbol` from the server.
- CLI: `anchors`, `serve`, `init-zed`, `config check`.
- Languages first: C, C++, Rust, Python, Go, JavaScript, TypeScript.

**Out of scope**

- Real Markdown rendering (headings, links, fenced code, images). Semantic tokens only carry color, weight, style, underline, and background.
- Replacing or wrapping clangd, rust-analyzer, or any other server.
- A custom tree-sitter grammar or per-language `injections.scm`.
- Multi-line tag bodies in v1.

## 3. Evidence ledger

Status key: **E** empirical (read in docs or source), **I** inferred from evidence, **U** unverified.

| # | Claim | Status | Basis |
| --- | --- | --- | --- |
| 1 | Zed ships LSP semantic tokens (added in the 0.224 series, Feb 2026) | E | Zed release notes 0.224.4 |
| 2 | Off by default. `combined` overlays tokens on tree-sitter, `full` replaces it | E | Zed docs: semantic tokens |
| 3 | User rules support `foreground_color`, `background_color`, `underline`, `strikethrough`, `font_weight`, `font_style`, matched on token type and modifiers | E | Zed docs: language extensions |
| 4 | Custom modifiers match in rules (docs use `mutable`, which is not a standard LSP modifier) | I, \~85% | Zed docs examples |
| 5 | An extension can attach a server to built-in languages by name with no `languages/` directory | E | `codebook-zed/extension.toml` lists C, C++, Rust, Python and more |
| 6 | Extension-shipped default token rules live in a language directory the extension defines, so this extension cannot ship defaults for built-in languages | I, \~75% | Zed docs wording; confirm in spike |
| 7 | Tokens from several servers are aggregated and shown once all have replied | E for the original PR, U on current main | zed PR #39539 |
| 8 | Zed project symbol search consumes server workspace symbols | I, \~85% | zed-imba README |
| 9 | Using a server for the outline replaces `outline.scm` and is a user opt-in | I, \~80% | zed-imba README |
| 10 | A token type with no matching rule is not styled | E | Zed docs: semantic tokens |
| 11 | Hierarchical TOML config plus LSP `initializationOptions` is a working pattern | E | Codebook README |
| 12 | A comment-tag highlighter extension for Zed already exists | U | recall only; check the registry before building |
| 13 | Publishing is a PR to `zed-industries/extensions` adding a submodule and an `extensions.toml` entry | E | zed-imba README, Codebook release script |

## 4. Architecture

### 4.1 Components

| Part | Role | Depends on |
| --- | --- | --- |
| `core` crate | Comment extraction (tree-sitter), tag and inline scanner, config loader, one `scan()` entry point | tree-sitter grammars, `toml`, `regex`, `ignore` |
| server (`<name> serve`) | LSP over stdio: tokens, diagnostics, workspace symbols | `core`, `lsp-server`, `lsp-types` |
| CLI (same binary) | `anchors`, `init-zed`, `config check` | `core` |
| Zed extension (separate repo, Rust to wasm) | `extension.toml`, binary resolution, nothing else | `zed_extension_api` |

This follows Codebook's split: server and CLI ship in one binary (`codebook-lsp serve` and `codebook-lsp lint`), and the Zed extension is its own repo that fetches that binary.

### 4.2 Repo layout

```text
<name>/                        # main repo
├── Cargo.toml                 # workspace
├── crates/
│   ├── core/                  # scan(), config, languages/
│   │   └── src/languages/     #   one module + comments.scm per language
│   └── cli/                   # binary: serve | anchors | init-zed | config
├── editors/                   # snippets for Neovim, Helix, VS Code
├── tests/golden/              # input files + expected anchors.json
└── <name>.toml                # dogfooding config
<name>-zed/                    # extension repo
├── extension.toml
├── Cargo.toml                 # cdylib, zed_extension_api
└── src/lib.rs                 # language_server_command()
```

### 4.3 Pipeline for one file

1. Detect the language: `languageId` from the client in the server, file extension in the CLI.
2. Parse with tree-sitter and run that language's comment query.
3. Strip each comment's delimiter (`//`, `///`, `/*`, a leading `*`, `#`) and keep an offset map back to the source.
4. Match a tag at the start of the comment body, then scan the rest for inline spans.
5. Produce `Anchor` and `Span` values with UTF-8 byte offsets; convert to line and column per consumer.
6. The server renders them as tokens, diagnostics, and symbols. The CLI prints them.

### 4.4 Component diagram

&#91;embedded content: component diagram · 4 parts around one core\]

The server and the CLI both call the same `scan()`. The extension only launches the server, and colors stay in Zed's settings.

### 4.5 Data model

```rust
pub struct Anchor {
    pub range: ByteRange,       // the tag token itself
    pub tag: String,            // "NOTE"
    pub class: String,          // from config: note | todo | fixme | bug | ...
    pub form: TagForm,          // Conventional | Delimited
    pub meta: Option<String>,   // "alice" from TODO(alice):
    pub text: String,           // rest of the comment line
    pub line: u32,              // 0-based; CLI prints 1-based
}

pub struct Span {
    pub range: ByteRange,
    pub kind: SpanKind,         // Strong | Emphasis | Code
}

pub struct Scan {
    pub anchors: Vec<Anchor>,
    pub spans: Vec<Span>,
}

pub fn scan(lang: Lang, src: &str, cfg: &Config) -> Scan;
```

### 4.6 Position encoding

LSP columns are UTF-16 code units unless the client negotiates otherwise. Semantic tokens are delta-encoded and cannot span lines, so a multi-line block comment needs one token per line (an existing Zed-oriented server does the same). Non-ASCII comment text will break naive byte-to-column maths. Keep byte offsets internally, convert at the edge, and add non-ASCII comments to the golden tests from day one.

## 5. Comment syntax

| Form | Example | Rule |
| --- | --- | --- |
| Conventional tag | `// NOTE: text`, `// TODO(user): text` | Uppercase tag at the start of the comment body, colon required |
| Delimited tag | `// =NOTE= text`, `// =NOTE=: text` | `=TAG=` at the start of the body, colon optional |
| Strong | `**word**` | CommonMark |
| Emphasis | `*word*` | CommonMark |
| Code span | `` `word` `` | Suppresses inline parsing inside it |

Matched against the comment body after the delimiter and whitespace:

```text
conventional: ^(?P<tag>[A-Z][A-Z0-9_]+)(?:[(](?P<meta>[^)]*)[)])?:
delimited:    ^=(?P<tag>[A-Z][A-Z0-9_]+)=:?
```

A match only counts if the tag is in the configured set, unless `unknown_tags = "highlight"`.

**Inline rules** (simplified CommonMark flanking): an opener is not followed by whitespace, a closer is not preceded by whitespace, spans do not cross a line end in v1, and code spans are parsed first. The block-comment gutter (`*`) and uses of `*` as multiplication or a pointer (`a * b`, `int *p, *q`) fail the flanking test, so they are left alone.

**Decision: `*word*` is italic by default.** The first sketch used `*main*` for bold. In CommonMark `*x*` is italic and `**x**` is bold, and a private dialect makes files look wrong in every other tool. Default is `emphasis = "commonmark"`. A `"star-bold"` dialect is opt-in for anyone who wants the sketch's behavior, so the deviation is explicit.

**Decision: `=NOTE=` is optional.** Conventional forms are on by default so existing code highlights with no edits. The delimited form buys fewer false positives from prose that happens to contain a tag word, and costs portability: it is noise in any tool that does not know the syntax.

**Worked example** (the sketch from the original idea):

```cpp
#include <iostream>

// =NOTE=: This is how you start a c++ file
int main(){
// This is called the *main* function.
}
```

Expected: one anchor on line 3, columns 4 to 10 (exclusive end), tag `NOTE`, form `delimited`, text `This is how you start a c++ file`. One emphasis span on line 5, columns 23 to 29 (strong under `star-bold`).

## 6. Configuration

There are two surfaces because Zed owns colors: tag semantics live in `<name>.toml`, styles live in Zed's settings.

Precedence mirrors Codebook: project config overrides global config, `[[overrides]]` blocks apply per path glob, and LSP `initializationOptions` can point at config files. Global path: `~/.config/<name>/<name>.toml`. Project path: `<name>.toml` or `.<name>.toml`, found by walking up from the file.

**`<name>.toml`**

```toml
version = 1

[syntax]
conventional = true          # NOTE: text   TODO(user): text
delimited    = true          # =NOTE= text  =NOTE=: text
inline       = "all"         # "all" | "tagged" | "off"
emphasis     = "commonmark"  # or "star-bold"
unknown_tags = "ignore"      # or "highlight"

[[tags]]
name     = "NOTE"
class    = "note"            # becomes a semantic-token modifier
severity = "off"             # off | hint | information | warning | error
symbol   = true              # listed in workspace/symbol and `anchors`

[[tags]]
name     = "FIXME"
aliases  = ["XXX"]
class    = "fixme"
severity = "information"
symbol   = true

[[overrides]]
paths   = ["vendor/**", "third_party/**"]
enabled = false
```

**Zed `settings.json`** (colors and weights)

```json
{
  "semantic_tokens": "combined",
  "global_lsp_settings": {
    "semantic_token_rules": [
      { "token_type": "keyword", "token_modifiers": ["note"],  "foreground_color": "#7aa2f7", "font_weight": "bold" },
      { "token_type": "keyword", "token_modifiers": ["fixme"], "foreground_color": "#f7768e", "font_weight": "bold" },
      { "token_type": "comment", "token_modifiers": ["strong"],   "font_weight": "bold" },
      { "token_type": "comment", "token_modifiers": ["emphasis"], "font_style": "italic" },
      { "token_type": "comment", "token_modifiers": ["code"],     "background_color": "#292e42" }
    ]
  }
}
```

`<name> init-zed` prints the `semantic_token_rules` array generated from `<name>.toml`, so the two files stay in sync. Server options passed as `initializationOptions`: `configPath`, `globalConfigPath`, `checkWhileTyping`, `logLevel`.

## 7. Server design

| Capability | v1 | Notes |
| --- | --- | --- |
| `textDocument/semanticTokens/full` | yes | `range` and `delta` in M4 |
| `publishDiagnostics` | yes | Per-tag severity. `source` = server name, `code` = tag. Default off for NOTE |
| `workspace/symbol` | yes | Background scan honoring `.gitignore` (`ignore` crate), refreshed on save and on file-watch events |
| `textDocument/documentSymbol` | no | Only matters if a user moves the outline source away from `outline.scm`; skip |
| hover, code actions | later | Hover could show tag metadata; a code action could open an issue |

**Token legend**

- Types: `keyword` for tags, `comment` for inline spans.
- Modifiers: one per tag class from config (`note`, `todo`, `fixme`, `bug`, and so on), plus `strong`, `emphasis`, `code`.

**Why standard types.** Zed's docs let a language extension ship default rules in a `semantic_token_rules.json` inside a language directory it defines. This extension defines none, because it attaches to languages other people own, and a token type with no matching rule is not styled. Tags therefore use `keyword` so they are visible out of the box, and user rules keyed on the class modifier refine them. Inline spans use `comment` so the base color stays and only weight, style, or background changes. Those need user rules, which is the onboarding cost in R2.

**Coexistence.** With several servers attached, Zed aggregates their tokens and, per the original PR, waits for all of them before showing any. Display latency then tracks the slowest server, not this one. Keep a scan to a few milliseconds and cache results per document version.

**Sync and crates.** Start with full-document sync and a full reparse; add incremental tree-sitter edits only if profiling asks for them. Recommendation: `lsp-server` plus `lsp-types`, a minimal synchronous loop that suits this workload. Check the maintenance state of async alternatives before committing to either.

**Comment extraction per language.** Each language gets a small `comments.scm` capturing its comment nodes. Node names differ across grammars (`comment`, `line_comment`, `block_comment`, doc-comment variants), so pin grammar revisions and keep golden tests per language. For unsupported languages, a delimiter-table fallback (line prefixes and block pairs) gives best-effort results and is labelled as such.

## 8. CLI design

`<name> anchors [PATH...]`

| Flag | Meaning |
| --- | --- |
| `--format text\|json\|ndjson` | Output format, default `text` |
| `--tag NOTE,FIXME` | Filter by tag |
| `--deny FIXME,BUG` | Exit 1 if any of these exist (CI gate) |
| `--spans` | Include inline spans in JSON |
| `--no-ignore` | Do not honor `.gitignore` |

Exit codes: 0 clean, 1 findings under `--deny`, 2 unreadable input or invalid config (same convention as Codebook's `lint`).

```text
$ <name> anchors main.cpp
main.cpp:3:4  NOTE  This is how you start a c++ file
```

```json
{"file":"main.cpp","line":3,"column":4,"end_column":10,"tag":"NOTE","class":"note","form":"delimited","meta":null,"text":"This is how you start a c++ file"}
```

**Subcommand, not flag.** The original sketch used `<command> --anchors main.cpp`. A subcommand matches Codebook's `lint` and leaves `serve`, `init-zed`, and `config` as siblings. Other commands: `serve` (stdio LSP), `init-zed`, `config check|print`.

## 9. Zed extension

```toml
id = "<name>"
name = "<Display Name>"
version = "0.0.1"
schema_version = 1
authors = ["<you>"]
description = "Comment tags and inline emphasis, styled by you."
license = "MIT"
repository = "https://github.com/<you>/<name>-zed"

[language_servers.<name>]
name = "<Display Name>"
languages = ["C", "C++", "Rust", "Python", "Go", "JavaScript", "TypeScript", "TSX"]

[language_servers.<name>.language_ids]
"C" = "c"
"C++" = "cpp"
"Rust" = "rust"
"Python" = "python"
"Go" = "go"
"JavaScript" = "javascript"
"TypeScript" = "typescript"
"TSX" = "typescriptreact"
```

(Placeholders in the table names are for illustration; use the real server id.)

**Binary resolution order.** (1) `<name>` on `PATH`, which also sidesteps failed downloads on remote hosts and WSL, as Codebook's extension does; (2) a cached download of the GitHub release asset for the pinned version and the current platform; (3) a clear error telling the user how to install.

**Running beside the primary server.** If a language's server list is customised, include the new server or the `"..."` wildcard, for example `"C++": { "language_servers": ["clangd", "<name>", "..."] }`.

**Notes**

- Zed's multi-language example writes `[language-servers.…]` with a hyphen, while the same page's other examples and Codebook's real `extension.toml` use `language_servers`. Use the underscore form and confirm in the spike.
- `opt_in_languages` lets the server start only for languages a user names explicitly, which is a conservative rollout option.
- Release flow: tag the extension repo, then open a PR to `zed-industries/extensions` adding the repo as a submodule and an entry in `extensions.toml`. Codebook sets `license` in its `extension.toml`; as far as I know Zed requires an open-source license for registry extensions, so verify before publishing.

## 10. Build plan

| Milestone | Deliverable | Exit criteria |
| --- | --- | --- |
| M0 Spike | Single-file toy server that emits one semantic token for `NOTE:` in C++ comments, loaded as a dev extension next to clangd | Token visible with `combined`. A user rule on a custom modifier changes its color. clangd highlighting is unaffected. `language_ids` and the underscore table name verified. Check whether a `languages/C++/semantic_token_rules.json` collides with the built-in language |
| M1 Core | `scan()`, config loader, C, C++, Rust, Python, golden tests, `anchors` with text and JSON output | Golden tests green. The worked example in section 5 matches exactly, including non-ASCII cases |
| M2 Server | Tokens, diagnostics, workspace symbols, config reload | One week of dogfooding on your own repos with no missed or stuck tokens |
| M3 Extension | Binary resolution and download, `extension.toml`, README with the settings snippet, `init-zed`, registry PR | Fresh install works on a clean machine and on a remote host |
| M4 Hardening | More languages, `range` and `delta` tokens, hover, Neovim and Helix docs | Profile shows scan time well below the primary server's latency |

**Fallback.** If the spike shows tokens from this server are lost or overridden (R1), ship diagnostics, workspace symbols, and the CLI, which stand on their own, and treat highlighting as an upstream conversation.

## 11. Risks

| # | Risk | Likelihood / impact | Test or mitigation |
| --- | --- | --- | --- |
| R1 | Tokens lost or overridden when clangd or rust-analyzer also emit tokens over comments | Medium / fatal for highlighting | M0 spike; fallback above |
| R2 | Semantic tokens are off by default and style rules live in user settings | High / medium | README snippet, `init-zed`, visible default via the `keyword` type |
| R3 | `=TAG=` dialect leaks into shared code and reads as noise elsewhere | Medium / low | Conventional forms default-on, delimited optional |
| R4 | Per-language comment query upkeep as grammars change | High / medium | Golden tests per language, pinned grammar revisions, delimiter-table fallback |
| R5 | Diagnostics noise | Medium / medium | Severity off by default for NOTE, per-path overrides |
| R6 | Zed API churn: the feature is under a year old and the docs already differ on command names | Medium / low | State the tested Zed version in the README, keep `core` editor-agnostic |
| R7 | Overlap with an existing extension | Unknown | Search the registry before M3; differentiate on per-user styling, anchors, and the CLI |
| R8 | Emphasis false positives (pointers, math, globs, list markers in comments) | Medium / low | Flanking rules, `inline = "tagged"` mode, golden tests |

## 12. Open decisions

- Project name (kept as `<name>` here).
- Default tag set and default severities. Suggested start: NOTE off, TODO hint, FIXME information, BUG warning, HACK information.
- Whether `inline` defaults to `all` (the sketch's behavior) or `tagged` (fewer false positives).
- Whether to add a per-tag `color` field in `<name>.toml` for terminal output from the CLI, accepting that Zed colors stay in Zed settings.

## 13. Sources

- Zed docs, Language Extensions: https://zed.dev/docs/extensions/languages
- Zed docs, Semantic Tokens: https://zed.dev/docs/semantic-tokens
- Zed release notes 0.224.4: https://zed.dev/releases/stable/0.224.4
- Zed PR #39539, semantic tokens and multi-server aggregation: https://github.com/zed-industries/zed/pull/39539
- Codebook (core, LSP, CLI, config): https://github.com/blopker/codebook
- Codebook Zed extension: https://github.com/blopker/codebook-zed
- zed-imba (workspace symbols, outline opt-in, registry flow): https://github.com/imba/zed-imba
- zed-hyperlinks-server (custom token legend, per-line tokens): https://github.com/krawitzzZ/zed-hyperlinks-server
