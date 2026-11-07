use clap::ValueEnum;
use comment_annotations_core::{scan, Config, Lang};
use ignore::WalkBuilder;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, ValueEnum, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
    Ndjson,
}

#[derive(Debug, Serialize)]
struct AnchorRecord {
    file: String,
    line: u32,
    column: u32,
    end_column: u32,
    tag: String,
    class: String,
    form: String,
    meta: Option<String>,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    spans: Option<Vec<SpanRecord>>,
}

#[derive(Debug, Serialize)]
struct SpanRecord {
    line: u32,
    column: u32,
    end_column: u32,
    kind: String,
}

pub fn run(paths: Vec<String>, format: OutputFormat, tag_filter: Option<String>, deny: Option<String>, include_spans: bool, no_ignore: bool, config_path: Option<String>) -> i32 {
    // Load config: explicit path, else search from first path or cwd
    let cfg = load_config(config_path.as_deref(), &paths);
    let cfg = match cfg {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: invalid config: {e}");
            return 2;
        }
    };

    let search_roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };

    let tag_filter_set: Option<Vec<String>> = tag_filter.map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
    let deny_set: Vec<String> = deny.map(|s| s.split(',').map(|x| x.trim().to_string()).collect()).unwrap_or_default();

    let mut records: Vec<AnchorRecord> = Vec::new();
    let mut had_unreadable = false;
    let mut any_denied = false;

    for root in &search_roots {
        if !root.exists() {
            eprintln!("error: not found: {}", root.display());
            had_unreadable = true;
            continue;
        }
        // Collect files
        let mut files = collect_files(root, no_ignore);
        files.sort();
        for file in files {
            if !cfg.enabled_for_path(&file) {
                continue;
            }
            let lang = Lang::from_path(&file);
            if lang == Lang::Unknown {
                continue;
            }
            let src = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read {}: {e}", file.display());
                    had_unreadable = true;
                    continue;
                }
            };
            let result = scan(lang, &src, &cfg);
            let anchor_lines_for_spans: std::collections::HashSet<u32> =
                result.anchors.iter().map(|a| a.line).collect();
            for anchor in &result.anchors {
                if let Some(filters) = &tag_filter_set {
                    if !filters.iter().any(|f| f == &anchor.tag) {
                        continue;
                    }
                }
                if deny_set.iter().any(|d| d == &anchor.tag) {
                    any_denied = true;
                }
                let (line0, col0) = comment_annotations_core::scan::byte_to_lsp(&src, anchor.range.start);
                let (_line1, col1) = comment_annotations_core::scan::byte_to_lsp(&src, anchor.range.end);
                let spans_rec = if include_spans {
                    let mut v = Vec::new();
                    for s in &result.spans {
                        // Only spans on same line as anchor? Include all spans in file? For per-anchor, include spans on that line
                        let (sline, scol) = comment_annotations_core::scan::byte_to_lsp(&src, s.range.start);
                        if sline == anchor.line {
                            let (_, ecol) = comment_annotations_core::scan::byte_to_lsp(&src, s.range.end);
                            let kind = match s.kind {
                                comment_annotations_core::SpanKind::Strong => "strong",
                                comment_annotations_core::SpanKind::Emphasis => "emphasis",
                                comment_annotations_core::SpanKind::Code => "code",
                            };
                            v.push(SpanRecord { line: sline + 1, column: scol + 1, end_column: ecol, kind: kind.to_string() });
                        }
                    }
                    if v.is_empty() { None } else { Some(v) }
                } else { None };

                records.push(AnchorRecord {
                    file: file.to_string_lossy().to_string(),
                    line: line0 + 1,
                    column: col0 + 1,
                    end_column: col1,
                    tag: anchor.tag.clone(),
                    class: anchor.class.clone(),
                    form: match anchor.form {
                        comment_annotations_core::TagForm::Conventional => "conventional",
                        comment_annotations_core::TagForm::Delimited => "delimited",
                    }.to_string(),
                    meta: anchor.meta.clone(),
                    text: anchor.text.clone(),
                    spans: spans_rec,
                });
            }
            // With --spans, also emit spans that sit on lines without an anchor
            // (e.g. emphasis in a plain comment) so they are not dropped.
            if include_spans {
                for s in &result.spans {
                    let (sline, scol) = comment_annotations_core::scan::byte_to_lsp(&src, s.range.start);
                    if anchor_lines_for_spans.contains(&sline) {
                        continue;
                    }
                    let (_, ecol) = comment_annotations_core::scan::byte_to_lsp(&src, s.range.end);
                    let kind = match s.kind {
                        comment_annotations_core::SpanKind::Strong => "strong",
                        comment_annotations_core::SpanKind::Emphasis => "emphasis",
                        comment_annotations_core::SpanKind::Code => "code",
                    };
                    let span_text = src[s.range.clone()].to_string();
                    records.push(AnchorRecord {
                        file: file.to_string_lossy().to_string(),
                        line: sline + 1,
                        column: scol + 1,
                        end_column: ecol,
                        tag: "SPAN".to_string(),
                        class: kind.to_string(),
                        form: "inline".to_string(),
                        meta: None,
                        text: span_text,
                        spans: None,
                    });
                }
                records.sort_by_key(|r| (r.file.clone(), r.line, r.column));
            }
        }
    }

    match format {
        OutputFormat::Text => {
            for r in &records {
                println!("{}:{}:{}  {}  {}", r.file, r.line, r.column, r.tag, r.text);
            }
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&records).unwrap());
        }
        OutputFormat::Ndjson => {
            for r in &records {
                println!("{}", serde_json::to_string(r).unwrap());
            }
        }
    }

    if had_unreadable {
        2
    } else if any_denied {
        1
    } else {
        0
    }
}

fn load_config(explicit: Option<&str>, paths: &[String]) -> Result<Config, String> {
    if let Some(p) = explicit {
        return Config::from_file(Path::new(p)).map_err(|e| e.to_string());
    }
    let start: PathBuf = paths.first().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    Config::load_for_file(&start, None).map_err(|e| e.to_string())
}

fn collect_files(root: &Path, no_ignore: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
        return out;
    }
    let mut builder = WalkBuilder::new(root);
    builder.hidden(false);
    if !no_ignore {
        builder.git_ignore(true).git_global(true).git_exclude(true).ignore(true);
    } else {
        builder.git_ignore(false).git_global(false).git_exclude(false).ignore(false);
    }
    builder.filter_entry(|e| {
        // Skip target/ dirs and .git
        if let Some(name) = e.file_name().to_str() {
            if name == ".git" || name == "target" {
                return false;
            }
        }
        true
    });
    for entry in builder.build() {
        match entry {
            Ok(e) => {
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let path = e.path().to_path_buf();
                    if Lang::from_path(&path) != Lang::Unknown {
                        out.push(path);
                    }
                }
            }
            Err(_) => {}
        }
    }
    out
}
