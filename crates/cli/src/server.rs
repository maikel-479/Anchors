use comment_annotations_core::{scan, Config, Lang, Severity, SpanKind};
use lsp_server::{Connection, Message, Request};
use lsp_types::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const TOKEN_MODIFIERS: &[&str] = &[
    "note", "todo", "fixme", "bug", "hack", "strong", "emphasis", "code",
];

struct Server {
    config: Config,
    docs: HashMap<Url, String>,
    doc_paths: HashMap<Url, PathBuf>,
    workspace_roots: Vec<PathBuf>,
}

pub fn run() {
    eprintln!("comment-annotations: starting LSP server");
    let (connection, io_threads) = Connection::stdio();
    let (id, init_params_value) = connection.initialize_start().unwrap();
    let init_params: InitializeParams = serde_json::from_value(init_params_value).unwrap_or(InitializeParams {
        root_uri: None,
        ..Default::default()
    });

    let mut config = Config::default();
    if let Some(opts) = &init_params.initialization_options {
        if let Some(obj) = opts.as_object() {
            if let Some(p) = obj.get("configPath").and_then(|v| v.as_str()) {
                if let Ok(c) = Config::from_file(Path::new(p)) {
                    config = c;
                }
            }
            if let Some(log) = obj.get("logLevel").and_then(|v| v.as_str()) {
                eprintln!("comment-annotations: logLevel={log}");
            }
        }
    }

    let mut workspace_roots = Vec::new();
    if let Some(uri) = &init_params.root_uri {
        if let Ok(p) = uri.to_file_path() {
            workspace_roots.push(p);
        }
    }
    if let Some(folders) = &init_params.workspace_folders {
        for f in folders {
            if let Ok(p) = f.uri.to_file_path() {
                workspace_roots.push(p);
            }
        }
    }
    if workspace_roots.is_empty() {
        workspace_roots.push(PathBuf::from("."));
    }

    let server_caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: SemanticTokensLegend {
                    token_types: vec![SemanticTokenType::new("keyword"), SemanticTokenType::new("comment")],
                    token_modifiers: TOKEN_MODIFIERS.iter().map(|s| SemanticTokenModifier::new(s)).collect(),
                },
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    let caps = InitializeResult {
        capabilities: server_caps,
        server_info: Some(ServerInfo {
            name: "comment-annotations".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    connection.initialize_finish(id, serde_json::to_value(caps).unwrap()).unwrap();

    let mut server = Server { config, docs: HashMap::new(), doc_paths: HashMap::new(), workspace_roots };

    for msg in connection.receiver.iter() {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req) == Ok(true) {
                    return;
                }
                handle_request(&connection, &mut server, req);
            }
            Message::Notification(notif) => {
                handle_notification(&connection, &mut server, notif);
            }
            Message::Response(_) => {}
        }
    }
    io_threads.join().unwrap();
}

fn handle_request(conn: &Connection, server: &mut Server, req: Request) {
    if req.method == "textDocument/semanticTokens/full" {
        let params: SemanticTokensParams = serde_json::from_value(req.params).unwrap_or(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: Url::parse("file:///tmp").unwrap() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
        let tokens = semantic_tokens_for(server, &params.text_document.uri);
        let result = SemanticTokensResult::Tokens(SemanticTokens { data: tokens, result_id: None });
        let resp = lsp_server::Response::new_ok(req.id, result);
        let _ = conn.sender.send(Message::Response(resp));
        return;
    }
    if req.method == "workspace/symbol" {
        let params: WorkspaceSymbolParams = serde_json::from_value(req.params).unwrap_or(WorkspaceSymbolParams {
            query: String::new(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
        let symbols = workspace_symbols(&server.workspace_roots, &params.query, &server.config);
        let result = WorkspaceSymbolResponse::Flat(symbols);
        let resp = lsp_server::Response::new_ok(req.id, result);
        let _ = conn.sender.send(Message::Response(resp));
        return;
    }
    let resp = lsp_server::Response::new_ok(req.id, serde_json::Value::Null);
    let _ = conn.sender.send(Message::Response(resp));
}

fn handle_notification(conn: &Connection, server: &mut Server, notif: lsp_server::Notification) {
    if notif.method == "textDocument/didOpen" {
        let params: DidOpenTextDocumentParams = serde_json::from_value(notif.params).unwrap_or(DidOpenTextDocumentParams {
            text_document: TextDocumentItem { uri: Url::parse("file:///tmp").unwrap(), language_id: String::new(), version: 0, text: String::new() },
        });
        let uri = params.text_document.uri.clone();
        if let Ok(p) = uri.to_file_path() {
            server.doc_paths.insert(uri.clone(), p);
        }
        server.docs.insert(uri.clone(), params.text_document.text);
        publish_diagnostics(conn, server, &uri);
        return;
    }
    if notif.method == "textDocument/didChange" {
        let params: DidChangeTextDocumentParams = serde_json::from_value(notif.params).unwrap_or(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri: Url::parse("file:///tmp").unwrap(), version: 0 },
            content_changes: vec![],
        });
        if let Some(last) = params.content_changes.last() {
            server.docs.insert(params.text_document.uri.clone(), last.text.clone());
            publish_diagnostics(conn, server, &params.text_document.uri);
        }
        return;
    }
    if notif.method == "textDocument/didClose" {
        let params: DidCloseTextDocumentParams = serde_json::from_value(notif.params).unwrap_or(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: Url::parse("file:///tmp").unwrap() },
        });
        server.docs.remove(&params.text_document.uri);
        publish_diagnostics_empty(conn, &params.text_document.uri);
        return;
    }
    if notif.method == "workspace/didChangeConfiguration" {
        let local = Path::new("anchors.toml");
        if local.exists() {
            if let Ok(c) = Config::from_file(local) {
                server.config = c;
            }
        }
        let uris: Vec<Url> = server.docs.keys().cloned().collect();
        for uri in uris {
            publish_diagnostics(conn, server, &uri);
        }
        return;
    }
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
        }).unwrap(),
    };
    let _ = conn.sender.send(Message::Notification(notif));
}

fn publish_diagnostics(conn: &Connection, server: &Server, uri: &Url) {
    let Some(src) = server.docs.get(uri) else { return };
    let lang = lang_for_doc(server, uri);
    if lang == Lang::Unknown {
        publish_diagnostics_empty(conn, uri);
        return;
    }
    let result = scan(lang, src, &server.config);
    let mut diags = Vec::new();
    for anchor in &result.anchors {
        let (_, sev, _) = server.config.lookup_tag(&anchor.tag).unwrap_or(("".into(), Severity::Off, false));
        if sev == Severity::Off {
            continue;
        }
        let (line_s, char_s) = comment_annotations_core::scan::byte_to_lsp(src, anchor.range.start);
        let (line_e, char_e) = comment_annotations_core::scan::byte_to_lsp(src, anchor.range.end);
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
                start: Position { line: line_s, character: char_s },
                end: Position { line: line_e, character: char_e },
            },
            severity: Some(severity),
            code: Some(NumberOrString::String(anchor.tag.clone())),
            source: Some("comment-annotations".into()),
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
        }).unwrap(),
    };
    let _ = conn.sender.send(Message::Notification(notif));
}

fn semantic_tokens_for(server: &Server, uri: &Url) -> Vec<SemanticToken> {
    let Some(src) = server.docs.get(uri) else { return vec![] };
    let lang = lang_for_doc(server, uri);
    if lang == Lang::Unknown {
        return vec![];
    }
    let result = scan(lang, src, &server.config);
    #[derive(Clone)]
    struct Raw { line: u32, col: u32, len: u32, t: u32, mods: u32 }
    let mut raw: Vec<Raw> = Vec::new();
    for anchor in &result.anchors {
        let (line, col) = comment_annotations_core::scan::byte_to_lsp(src, anchor.range.start);
        let (_, end_col) = comment_annotations_core::scan::byte_to_lsp(src, anchor.range.end);
        let len = end_col.saturating_sub(col);
        if len == 0 { continue; }
        let mod_idx = TOKEN_MODIFIERS.iter().position(|m| *m == anchor.class).unwrap_or(usize::MAX);
        let mods = if mod_idx != usize::MAX { 1u32 << mod_idx } else { 0 };
        raw.push(Raw { line, col, len, t: 0, mods });
    }
    for span in &result.spans {
        let (line, col) = comment_annotations_core::scan::byte_to_lsp(src, span.range.start);
        let (_, end_col) = comment_annotations_core::scan::byte_to_lsp(src, span.range.end);
        let len = end_col.saturating_sub(col);
        if len == 0 { continue; }
        let kind_name = match span.kind {
            SpanKind::Strong => "strong",
            SpanKind::Emphasis => "emphasis",
            SpanKind::Code => "code",
        };
        let mod_idx = TOKEN_MODIFIERS.iter().position(|m| *m == kind_name).unwrap_or(usize::MAX);
        let mods = if mod_idx != usize::MAX { 1u32 << mod_idx } else { 0 };
        raw.push(Raw { line, col, len, t: 1, mods });
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

fn workspace_symbols(roots: &[PathBuf], query: &str, cfg: &Config) -> Vec<SymbolInformation> {
    let q = query.to_ascii_lowercase();
    let mut out = Vec::new();
    for root in roots {
        let mut builder = ignore::WalkBuilder::new(root);
        builder.hidden(false).git_ignore(true).git_global(true).git_exclude(true).ignore(true);
        builder.filter_entry(|e| {
            if let Some(name) = e.file_name().to_str() {
                if name == ".git" || name == "target" { return false; }
            }
            true
        });
        for entry in builder.build() {
            let Ok(e) = entry else { continue };
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
            let path = e.path();
            let lang = Lang::from_path(path);
            if lang == Lang::Unknown { continue; }
            if !cfg.enabled_for_path(path) { continue; }
            let Ok(src) = std::fs::read_to_string(path) else { continue };
            let result = scan(lang, &src, cfg);
            for anchor in result.anchors {
                let (_, _, sym) = cfg.lookup_tag(&anchor.tag).unwrap_or((String::new(), Severity::Off, false));
                if !sym { continue; }
                let name = format!("{}: {}", anchor.tag, anchor.text);
                if !q.is_empty() && !name.to_ascii_lowercase().contains(&q) && !anchor.tag.to_ascii_lowercase().contains(&q) {
                    continue;
                }
                let Ok(uri) = Url::from_file_path(path) else { continue };
                let (line_s, char_s) = comment_annotations_core::scan::byte_to_lsp(&src, anchor.range.start);
                let (line_e, char_e) = comment_annotations_core::scan::byte_to_lsp(&src, anchor.range.end);
                let range = Range {
                    start: Position { line: line_s, character: char_s },
                    end: Position { line: line_e, character: char_e },
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
