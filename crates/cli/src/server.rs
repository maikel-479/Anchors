use canc_core::{scan, Config, Lang, Severity, SpanKind};
use lsp_server::{Connection, Message, Request};
use lsp_types::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct Server {
    /// Explicit config passed via initialization options `configPath`
    /// (used when a document has no file path or no on-disk config).
    config: Config,
    /// Optional global config path from initialization options
    /// `globalConfigPath`; otherwise `Config::load_for_file` resolves the
    /// standard user config location.
    global_config_path: Option<PathBuf>,
    /// Resolved project configs, cached per directory so we don't re-walk
    /// the directory tree on every keystroke.
    config_cache: HashMap<PathBuf, Config>,
    /// Semantic-token modifier legend. Advertised once in the initialize
    /// result and must not change mid-session, so it's built from the union
    /// of configured tag classes (initial config + workspace-root configs)
    /// plus the inline-span modifiers.
    token_modifiers: Vec<String>,
    docs: HashMap<Url, String>,
    doc_paths: HashMap<Url, PathBuf>,
    workspace_roots: Vec<PathBuf>,
    /// Counter for `workspace/semanticTokens/refresh` request ids this
    /// server sends to the client. Formatted as `canc-refresh-<n>` so
    /// these ids never collide with numeric ids the client uses for its
    /// own requests on the same connection.
    next_refresh_id: u32,
}

/// Build the modifier legend: every configured tag class in config order
/// (deduped, across the given configs), then `strong`/`emphasis`/`code`.
fn legend_modifiers(configs: &[Config]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for cfg in configs {
        for tag in &cfg.tags {
            if !out.contains(&tag.class) {
                out.push(tag.class.clone());
            }
        }
    }
    for modifier in ["strong", "emphasis", "code"] {
        if !out.iter().any(|c| c == modifier) {
            out.push(modifier.into());
        }
    }
    out
}

/// Resolve the effective config for a document path, cacheable per directory.
fn resolved_config(server: &mut Server, path: Option<&Path>) -> Config {
    let Some(path) = path else {
        return server.config.clone();
    };
    let dir = path
        .parent()
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| path.to_path_buf());
    if let Some(cfg) = server.config_cache.get(&dir) {
        return cfg.clone();
    }
    let cfg = Config::load_for_file(path, server.global_config_path.as_deref())
        .ok()
        .unwrap_or_else(Config::default);
    server.config_cache.insert(dir, cfg.clone());
    cfg
}

/// Effective config for an open document (per project, not one global value).
fn config_for_doc(server: &mut Server, uri: &Url) -> Config {
    let path = server
        .doc_paths
        .get(uri)
        .cloned()
        .or_else(|| uri.to_file_path().ok());
    resolved_config(server, path.as_deref())
}

pub fn run() {
    eprintln!("canc: starting LSP server");
    let (connection, io_threads) = Connection::stdio();
    // io_threads is never joined: every exit path below terminates the
    // process directly, because `io_threads.join()` dead-locks when the
    // client half-closes the pipe before/without an `exit` notification.

    // A client may disconnect before `initialize` (e.g. `canc serve
    // </dev/null`, or Zed tearing a stale session down). On stdin EOF the
    // channel closes and `initialize_start` fails: exit cleanly instead of
    // panicking on the Err.
    let (id, init_params_value) = match connection.initialize_start() {
        Ok(init) => init,
        Err(err) => {
            eprintln!("canc: client disconnected before initialize: {err}");
            std::process::exit(1);
        }
    };
    let init_params: InitializeParams =
        serde_json::from_value(init_params_value).unwrap_or(InitializeParams {
            root_uri: None,
            ..Default::default()
        });

    let mut config = Config::default();
    let mut global_config_path: Option<PathBuf> = None;
    if let Some(opts) = &init_params.initialization_options {
        if let Some(obj) = opts.as_object() {
            if let Some(p) = obj.get("configPath").and_then(|v| v.as_str()) {
                if let Ok(c) = Config::from_file(Path::new(p)) {
                    config = c;
                }
            }
            if let Some(p) = obj.get("globalConfigPath").and_then(|v| v.as_str()) {
                global_config_path = Some(PathBuf::from(p));
            }
            if let Some(log) = obj.get("logLevel").and_then(|v| v.as_str()) {
                eprintln!("canc: logLevel={log}");
            }
        }
    }

    // Workspace roots: prefer explicit `workspace_folders` and fall back to
    // `root_uri`, deduped by path. The old code pushed both, so when they
    // matched every workspace/symbol result was duplicated.
    let mut workspace_roots: Vec<PathBuf> = Vec::new();
    let mut push_root = |p: PathBuf| {
        if !workspace_roots.contains(&p) {
            workspace_roots.push(p);
        }
    };
    if let Some(folders) = &init_params.workspace_folders {
        for folder in folders {
            if let Ok(p) = folder.uri.to_file_path() {
                push_root(p);
            }
        }
    }
    if let Some(uri) = &init_params.root_uri {
        if let Ok(p) = uri.to_file_path() {
            push_root(p);
        }
    }
    if workspace_roots.is_empty() {
        workspace_roots.push(PathBuf::from("."));
    }

    // The legend is fixed for the whole session (advertised in the
    // initialize result), so build it up front from the union of classes
    // across the initial config and every workspace root. Classes found
    // later (per-directory configs) that aren't in the legend still emit
    // tokens, but only with modifier bitset 0.
    let mut initial_configs = vec![config.clone()];
    for root in &workspace_roots {
        if let Ok(c) = Config::load_for_file(root, global_config_path.as_deref()) {
            initial_configs.push(c);
        }
    }
    let token_modifiers = legend_modifiers(&initial_configs);

    let server_caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: SemanticTokensLegend {
                    token_types: vec![
                        SemanticTokenType::new("keyword"),
                        SemanticTokenType::new("comment"),
                    ],
                    token_modifiers: token_modifiers
                        .iter()
                        .map(|m| SemanticTokenModifier::from(m.clone()))
                        .collect(),
                },
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    let caps = InitializeResult {
        capabilities: server_caps,
        server_info: Some(ServerInfo {
            name: "canc".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    if let Err(err) = connection.initialize_finish(id, serde_json::to_value(caps).unwrap()) {
        eprintln!("canc: initialize_finish failed: {err}");
        std::process::exit(1);
    }

    let mut server = Server {
        config,
        global_config_path,
        config_cache: HashMap::new(),
        token_modifiers,
        docs: HashMap::new(),
        doc_paths: HashMap::new(),
        workspace_roots,
        next_refresh_id: 0,
    };

    for msg in connection.receiver.iter() {
        match msg {
            Message::Request(req) => {
                match connection.handle_shutdown(&req) {
                    // handle_shutdown returns Ok(true) only after the
                    // shutdown request AND its exit notification, and then
                    // terminates the loop channel — stop here.
                    Ok(true) => std::process::exit(0),
                    Ok(false) => handle_request(&connection, &mut server, req),
                    // Timeout/disconnect while waiting for exit after
                    // shutdown: don't dead-lock on io_threads.
                    Err(err) => {
                        eprintln!("canc: shutdown handling failed: {err}");
                        std::process::exit(1);
                    }
                }
            }
            Message::Notification(notif) => {
                // Spec: `exit` with a prior `shutdown` → code 0; `exit`
                // without `shutdown` → code 1. Exit explicitly: the message
                // channel ends here anyway, and io_threads.join() can
                // dead-lock when the client half-closes first (leaving a
                // zombie `canc` per dead Zed session).
                if notif.method == "exit" {
                    eprintln!("canc: exit notification without shutdown");
                    std::process::exit(1);
                }
                handle_notification(&connection, &mut server, notif);
            }
            Message::Response(_) => {}
        }
    }
    // EOF without an `exit` notification (client vanished): terminate the
    // process rather than dead-locking on the join.
    eprintln!("canc: client went away without exit notification");
    std::process::exit(1);
}

fn handle_request(conn: &Connection, server: &mut Server, req: Request) {
    if req.method == "textDocument/semanticTokens/full" {
        let params: SemanticTokensParams =
            serde_json::from_value(req.params).unwrap_or(SemanticTokensParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///tmp").unwrap(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            });
        let tokens = semantic_tokens_for(server, &params.text_document.uri);
        let result = SemanticTokensResult::Tokens(SemanticTokens {
            data: tokens,
            result_id: None,
        });
        let resp = lsp_server::Response::new_ok(req.id, result);
        let _ = conn.sender.send(Message::Response(resp));
        return;
    }
    if req.method == "workspace/symbol" {
        let params: WorkspaceSymbolParams =
            serde_json::from_value(req.params).unwrap_or(WorkspaceSymbolParams {
                query: String::new(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            });
        let symbols = workspace_symbols(server, &params.query);
        let result = WorkspaceSymbolResponse::Flat(symbols);
        let resp = lsp_server::Response::new_ok(req.id, result);
        let _ = conn.sender.send(Message::Response(resp));
        return;
    }
    if req.method == "textDocument/documentSymbol" {
        let params: DocumentSymbolParams =
            serde_json::from_value(req.params).unwrap_or(DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///tmp").unwrap(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            });
        let symbols = document_symbols_for(server, &params.text_document.uri);
        let result = DocumentSymbolResponse::Nested(symbols);
        let resp = lsp_server::Response::new_ok(req.id, result);
        let _ = conn.sender.send(Message::Response(resp));
        return;
    }
    // Unknown method: per JSON-RPC, respond with MethodNotFound instead of a
    // null `result` (which is reserved for succeeded-but-empty results).
    let resp = lsp_server::Response::new_err(
        req.id,
        lsp_server::ErrorCode::MethodNotFound as i32,
        format!("method not found: {}", req.method),
    );
    let _ = conn.sender.send(Message::Response(resp));
}

fn handle_notification(conn: &Connection, server: &mut Server, notif: lsp_server::Notification) {
    if notif.method == "textDocument/didOpen" {
        let params: DidOpenTextDocumentParams =
            serde_json::from_value(notif.params).unwrap_or(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: Url::parse("file:///tmp").unwrap(),
                    language_id: String::new(),
                    version: 0,
                    text: String::new(),
                },
            });
        let uri = params.text_document.uri.clone();
        if let Ok(p) = uri.to_file_path() {
            server.doc_paths.insert(uri.clone(), p);
        }
        server.docs.insert(uri.clone(), params.text_document.text);
        publish_diagnostics(conn, server, &uri);
        send_semantic_refresh(conn, server);
        return;
    }
    if notif.method == "textDocument/didChange" {
        let params: DidChangeTextDocumentParams =
            serde_json::from_value(notif.params).unwrap_or(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: Url::parse("file:///tmp").unwrap(),
                    version: 0,
                },
                content_changes: vec![],
            });
        let uri = params.text_document.uri.clone();
        // Support both FULL (no range) and incremental (range) changes.
        // Zed may send incremental edits even when FULL is advertised
        // after reindents; blindly replacing the doc with last.text
        // corrupts all subsequent offsets ("affected by indents").
        let mut updated: Option<String> = server.docs.get(&uri).cloned();
        if let Some(mut text) = updated.clone() {
            for change in &params.content_changes {
                if let Some(range) = change.range {
                    let start = lsp_pos_to_byte(&text, range.start.line, range.start.character);
                    let end = lsp_pos_to_byte(&text, range.end.line, range.end.character);
                    let start = start.min(text.len());
                    let end = end.min(text.len());
                    if start <= end {
                        text = format!("{}{}{}", &text[..start], change.text, &text[end..]);
                    } else {
                        text = change.text.clone();
                    }
                } else {
                    text = change.text.clone();
                }
            }
            updated = Some(text);
        } else if let Some(last) = params.content_changes.last() {
            updated = Some(last.text.clone());
        }
        if let Some(text) = updated {
            server.docs.insert(uri.clone(), text);
            publish_diagnostics(conn, server, &uri);
            send_semantic_refresh(conn, server);
        }
        return;
    }
    if notif.method == "textDocument/didClose" {
        let params: DidCloseTextDocumentParams =
            serde_json::from_value(notif.params).unwrap_or(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///tmp").unwrap(),
                },
            });
        server.docs.remove(&params.text_document.uri);
        publish_diagnostics_empty(conn, &params.text_document.uri);
        return;
    }
    if notif.method == "workspace/didChangeConfiguration" {
        // Configs may have changed on disk and/or per-root settings; drop the
        // per-directory cache and re-resolve on the next use.
        server.config_cache.clear();
        let uris: Vec<Url> = server.docs.keys().cloned().collect();
        for uri in uris {
            publish_diagnostics(conn, server, &uri);
        }
        return;
    }
}

/// Send a `workspace/semanticTokens/refresh` request to the client so it
/// re-requests semantic tokens instead of leaving highlights anchored to
/// pre-edit positions. Per the LSP spec a server MAY send this at any
/// time; it is sent once per didOpen/didChange notification that changed
/// text (not once per content-change chunk). Some clients may reply with
/// a MethodNotFound error — that Response is tolerated silently by the
/// main loop's `Message::Response(_) => {}` arm.
/// Request ids use a string namespace so they cannot collide with the
/// numeric ids clients use for their own requests on the same pipe.
fn send_semantic_refresh(conn: &Connection, server: &mut Server) {
    server.next_refresh_id = server.next_refresh_id.wrapping_add(1);
    let id = lsp_server::RequestId::from(format!("canc-refresh-{}", server.next_refresh_id));
    let req = lsp_server::Request {
        id,
        method: "workspace/semanticTokens/refresh".into(),
        params: serde_json::Value::Null,
    };
    let _ = conn.sender.send(Message::Request(req));
}

fn lang_for_doc(server: &Server, uri: &Url) -> Lang {
    if let Some(p) = server.doc_paths.get(uri) {
        Lang::from_path(p)
    } else if let Ok(p) = uri.to_file_path() {
        Lang::from_path(&p)
    } else {
        Lang::Unknown
    }
}

fn publish_diagnostics_empty(conn: &Connection, uri: &Url) {
    let notif = lsp_server::Notification {
        method: "textDocument/publishDiagnostics".into(),
        params: serde_json::to_value(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![],
            version: None,
        })
        .unwrap(),
    };
    let _ = conn.sender.send(Message::Notification(notif));
}

fn publish_diagnostics(conn: &Connection, server: &mut Server, uri: &Url) {
    let Some(src) = server.docs.get(uri).cloned() else {
        return;
    };
    let lang = lang_for_doc(server, uri);
    if lang == Lang::Unknown {
        publish_diagnostics_empty(conn, uri);
        return;
    }
    let config = config_for_doc(server, uri);
    let result = scan(lang, &src, &config);
    let mut diags = Vec::new();
    for anchor in &result.anchors {
        let (_, sev, _) =
            config
                .lookup_tag(&anchor.tag)
                .unwrap_or(("".into(), Severity::Off, false));
        if sev == Severity::Off {
            continue;
        }
        let (line_s, char_s) =
            canc_core::scan::byte_to_lsp(&src, anchor.range.start);
        let (line_e, char_e) = canc_core::scan::byte_to_lsp(&src, anchor.range.end);
        let severity = match sev {
            Severity::Hint => DiagnosticSeverity::HINT,
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Off => continue,
        };
        let msg = format!("{}: {}", anchor.tag, anchor.text);
        diags.push(Diagnostic {
            range: Range {
                start: Position {
                    line: line_s,
                    character: char_s,
                },
                end: Position {
                    line: line_e,
                    character: char_e,
                },
            },
            severity: Some(severity),
            code: Some(NumberOrString::String(anchor.tag.clone())),
            source: Some("canc".into()),
            message: msg,
            ..Default::default()
        });
    }
    let notif = lsp_server::Notification {
        method: "textDocument/publishDiagnostics".into(),
        params: serde_json::to_value(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: diags,
            version: None,
        })
        .unwrap(),
    };
    let _ = conn.sender.send(Message::Notification(notif));
}

fn lsp_pos_to_byte(text: &str, line: u32, character: u32) -> usize {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if current_line == line {
            let line_text = &text[line_start..];
            let mut utf16_col = 0u32;
            for (j, ch) in line_text.char_indices() {
                if utf16_col >= character {
                    return line_start + j;
                }
                utf16_col += ch.len_utf16() as u32;
            }
            return line_start + line_text.len();
        }
        if b == b'\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    text.len()
}

fn semantic_tokens_for(server: &mut Server, uri: &Url) -> Vec<SemanticToken> {
    let Some(src) = server.docs.get(uri).cloned() else {
        return vec![];
    };
    let lang = lang_for_doc(server, uri);
    if lang == Lang::Unknown {
        return vec![];
    }
    let config = config_for_doc(server, uri);
    let result = scan(lang, &src, &config);
    #[derive(Clone)]
    struct Raw {
        line: u32,
        col: u32,
        len: u32,
        t: u32,
        mods: u32,
    }
    let mut raw: Vec<Raw> = Vec::new();
    for anchor in &result.anchors {
        let (line, col) = canc_core::scan::byte_to_lsp(&src, anchor.range.start);
        let (_, end_col) = canc_core::scan::byte_to_lsp(&src, anchor.range.end);
        let len = end_col.saturating_sub(col);
        if len == 0 {
            continue;
        }
        let mod_idx = server
            .token_modifiers
            .iter()
            .position(|m| *m == anchor.class)
            .unwrap_or(usize::MAX);
        let mods = if mod_idx != usize::MAX {
            1u32 << mod_idx
        } else {
            0
        };
        raw.push(Raw {
            line,
            col,
            len,
            t: 0,
            mods,
        });
    }
    for span in &result.spans {
        let (line, col) = canc_core::scan::byte_to_lsp(&src, span.range.start);
        let (_, end_col) = canc_core::scan::byte_to_lsp(&src, span.range.end);
        let len = end_col.saturating_sub(col);
        if len == 0 {
            continue;
        }
        let kind_name = match span.kind {
            SpanKind::Strong => "strong",
            SpanKind::Emphasis => "emphasis",
            SpanKind::Code => "code",
        };
        let mod_idx = server
            .token_modifiers
            .iter()
            .position(|m| *m == kind_name)
            .unwrap_or(usize::MAX);
        let mods = if mod_idx != usize::MAX {
            1u32 << mod_idx
        } else {
            0
        };
        raw.push(Raw {
            line,
            col,
            len,
            t: 1,
            mods,
        });
    }
    raw.sort_by_key(|r| (r.line, r.col));
    let mut tokens = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;
    for r in raw {
        let dline = r.line - prev_line;
        let dcol = if dline == 0 { r.col - prev_col } else { r.col };
        tokens.push(SemanticToken {
            delta_line: dline,
            delta_start: dcol,
            length: r.len,
            token_type: r.t,
            token_modifiers_bitset: r.mods,
        });
        prev_line = r.line;
        prev_col = r.col;
    }
    tokens
}

fn document_symbols_for(server: &mut Server, uri: &Url) -> Vec<DocumentSymbol> {
    let Some(src) = server.docs.get(uri).cloned() else {
        return vec![];
    };
    let lang = lang_for_doc(server, uri);
    if lang == Lang::Unknown {
        return vec![];
    }
    let config = config_for_doc(server, uri);
    let result = scan(lang, &src, &config);
    let mut out = Vec::new();
    for anchor in result.anchors {
        let (_, _, sym) =
            config
                .lookup_tag(&anchor.tag)
                .unwrap_or((String::new(), Severity::Off, false));
        if !sym {
            continue;
        }
        let (line_s, char_s) =
            canc_core::scan::byte_to_lsp(&src, anchor.range.start);
        let (line_e, char_e) = canc_core::scan::byte_to_lsp(&src, anchor.range.end);
        let range = Range {
            start: Position {
                line: line_s,
                character: char_s,
            },
            end: Position {
                line: line_e,
                character: char_e,
            },
        };
        // Name is the bare tag (with user) so Zed's `highlights_from_buffer`
        // finds it verbatim in the buffer and applies combined
        // (tree-sitter + semantic token) highlights in the outline.
        // A "TAG: text" name never matches verbatim for delimited tags
        // (=NOTE=: vs NOTE:), so rows rendered grey (C++ case).
        let name = match &anchor.meta {
            Some(m) => format!("{}({})", anchor.tag, m),
            None => anchor.tag.clone(),
        };
        out.push(DocumentSymbol {
            name,
            detail: Some(anchor.text.clone()),
            kind: SymbolKind::KEY,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }
    // Include inline spans as emphasis symbols? Keep to anchors only so
    // outline stays a tag list; users can still jump to *main* via tokens.
    out
}

fn workspace_symbols(server: &mut Server, query: &str) -> Vec<SymbolInformation> {
    let q = query.to_ascii_lowercase();
    let mut out = Vec::new();
    for root in server.workspace_roots.clone() {
        let mut builder = ignore::WalkBuilder::new(root);
        // Keep hidden files excluded (the default); explicitly walking dot
        // directories like .config/.cargo made workspace/symbol painfully
        // slow and noisy.
        builder
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true);
        builder.filter_entry(|e| {
            if let Some(name) = e.file_name().to_str() {
                if name == ".git" {
                    return false;
                }
                // Exclude *directory* entries named `target` (Cargo build
                // dirs) only — a legit `target.rs` / `target/` source file
                // elsewhere stays scannable via the file-type check below.
                if name == "target" && e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    return false;
                }
            }
            true
        });
        for entry in builder.build() {
            let Ok(e) = entry else { continue };
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = e.path().to_path_buf();
            let lang = Lang::from_path(&path);
            if lang == Lang::Unknown {
                continue;
            }
            let cfg = resolved_config(server, Some(&path));
            if !cfg.enabled_for_path(&path) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            let result = scan(lang, &src, &cfg);
            for anchor in result.anchors {
                let (_, _, sym) =
                    cfg.lookup_tag(&anchor.tag)
                        .unwrap_or((String::new(), Severity::Off, false));
                if !sym {
                    continue;
                }
                let name = format!("{}: {}", anchor.tag, anchor.text);
                if !q.is_empty()
                    && !name.to_ascii_lowercase().contains(&q)
                    && !anchor.tag.to_ascii_lowercase().contains(&q)
                {
                    continue;
                }
                let Ok(uri) = Url::from_file_path(&path) else {
                    continue;
                };
                let (line_s, char_s) =
                    canc_core::scan::byte_to_lsp(&src, anchor.range.start);
                let (line_e, char_e) =
                    canc_core::scan::byte_to_lsp(&src, anchor.range.end);
                let range = Range {
                    start: Position {
                        line: line_s,
                        character: char_s,
                    },
                    end: Position {
                        line: line_e,
                        character: char_e,
                    },
                };
                out.push(SymbolInformation {
                    name,
                    kind: SymbolKind::KEY,
                    location: Location { uri, range },
                    container_name: None,
                    tags: None,
                    deprecated: None,
                });
            }
        }
    }
    out
}
