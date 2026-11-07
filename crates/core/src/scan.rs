use crate::config::{Config, InlineMode, Severity};
use crate::extract::extract_pieces;
use crate::lang::Lang;
use crate::tag::{scan_inline, Span, TagForm};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}
impl ByteRange {
    pub fn new(start: usize, end: usize) -> Self { Self { start, end } }
    pub fn range(&self) -> Range<usize> { self.start..self.end }
}

#[derive(Debug, Clone)]
pub struct Anchor {
    pub range: ByteRange,
    pub tag: String,
    pub class: String,
    pub form: TagForm,
    pub meta: Option<String>,
    pub text: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct Scan {
    pub anchors: Vec<Anchor>,
    pub spans: Vec<Span>,
}

pub fn scan(lang: Lang, src: &str, cfg: &Config) -> Scan {
    let pieces = extract_pieces(lang, src);
    let mut anchors: Vec<Anchor> = Vec::new();
    let mut spans: Vec<Span> = Vec::new();

    // Precompute line starts for line calc (byte -> 0-based line)
    let line_starts: Vec<usize> = {
        let mut v = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' { v.push(i + 1); }
        }
        v
    };
    let byte_to_line = |byte_off: usize| -> u32 {
        (line_starts.partition_point(|&s| s <= byte_off).saturating_sub(1)) as u32
    };

    for piece in &pieces {
        let body = &piece.body;
        let body_start = piece.body_start;

        // Tag detection
        let tag_match = crate::tag::match_tag(body, cfg);

        if let Some(tm) = tag_match {
            let (class, _severity, _symbol) = cfg.lookup_tag(&tm.tag)
                .unwrap_or((tm.tag.to_lowercase(), Severity::Off, false));
            let anchor_start = body_start + tm.match_range_in_body.start;
            let anchor_end = body_start + tm.match_range_in_body.end;
            let text = body[tm.text_start_in_body..].trim_end().to_string();
            anchors.push(Anchor {
                range: ByteRange::new(anchor_start, anchor_end),
                tag: tm.tag.clone(),
                class,
                form: tm.form,
                meta: tm.meta,
                text,
                line: byte_to_line(anchor_start),
            });

            // Inline scan: rest of the body after the tag (and whole body if inline==all? plan: inline tagged vs all)
            // For "tagged": only scan inside anchored comments (the rest after tag)
            // For "all": scan whole body minus tag region to avoid matching inside tag
            match cfg.syntax.inline {
                InlineMode::All | InlineMode::Tagged => {
                    let region_start_in_body = tm.text_start_in_body;
                    let region = &body[region_start_in_body..];
                    let region_abs = body_start + region_start_in_body;
                    let mut found = scan_inline(region, region_abs, cfg);
                    spans.append(&mut found);
                }
                InlineMode::Off => {}
            }
        } else {
            // No tag
            if matches!(cfg.syntax.inline, InlineMode::All) {
                let mut found = scan_inline(body, body_start, cfg);
                spans.append(&mut found);
            }
        }
    }

    spans.sort_by_key(|s| s.range.start);
    anchors.sort_by_key(|a| a.range.start);
    Scan { anchors, spans }
}

// Re-export SpanKind convenience
pub use crate::tag::SpanKind as CoreSpanKind;

/// Convert byte offset to (line, utf16_col) per LSP.
pub fn byte_to_lsp(src: &str, byte_off: usize) -> (u32, u32) {
    // line: count \n before byte_off
    let upto = &src[..byte_off.min(src.len())];
    let line = upto.bytes().filter(|&b| b == b'\n').count() as u32;
    // find line start byte
    let line_start = match upto.rfind('\n') {
        Some(idx) => idx + 1,
        None => 0,
    };
    let col_str = &src[line_start..byte_off.min(src.len())];
    (line, col_str.encode_utf16().count() as u32)
}

pub fn byte_len_to_utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}
