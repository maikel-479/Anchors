use crate::config::{Config, InlineMode, UnknownTags};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagForm {
    Conventional,
    Delimited,
}

#[derive(Debug, Clone)]
pub struct TagMatch {
    pub tag: String,
    pub meta: Option<String>,
    pub form: TagForm,
    /// Byte range of the match within the (trimmed) body.
    pub match_range_in_body: std::ops::Range<usize>,
    /// Byte offset within body where the remaining text starts (after tag + whitespace).
    pub text_start_in_body: usize,
}

fn conventional_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^(?P<tag>[A-Z][A-Z0-9_]+)(?:\((?P<meta>[^)]*)\))?:"#).unwrap())
}
fn delimited_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^=(?P<tag>[A-Z][A-Z0-9_]+)=:?"#).unwrap())
}

/// Match a tag at the start of `body` (leading whitespace is trimmed by caller
/// but we also tolerate it here by trimming and reporting offsets relative to
/// the trimmed start's caller-adjusted offset).
pub fn match_tag(body: &str, cfg: &Config) -> Option<TagMatch> {
    // Trim leading spaces/tabs
    let trimmed = body.trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '\r');
    let lead_ws = body.len() - trimmed.len();
    if trimmed.is_empty() {
        return None;
    }

    // Try delimited first (more specific)
    if cfg.syntax.delimited {
        if let Some(cap) = delimited_re().captures(trimmed) {
            if let Some(tag_m) = cap.name("tag") {
                let tag = tag_m.as_str().to_string();
                if tag_known(&tag, cfg) {
                    let m = cap.get(0).unwrap();
                    // text after match + one space
                    let mut text_start = m.end();
                    if trimmed[text_start..].starts_with(' ') {
                        text_start += 1;
                    }
                    return Some(TagMatch {
                        tag,
                        meta: None,
                        form: TagForm::Delimited,
                        match_range_in_body: (lead_ws + m.start())..(lead_ws + m.end()),
                        text_start_in_body: lead_ws + text_start,
                    });
                }
            }
        }
    }

    if cfg.syntax.conventional {
        if let Some(cap) = conventional_re().captures(trimmed) {
            if let Some(tag_m) = cap.name("tag") {
                let tag = tag_m.as_str().to_string();
                if tag_known(&tag, cfg) {
                    let meta = cap.name("meta").map(|m| m.as_str().to_string());
                    let m = cap.get(0).unwrap();
                    let mut text_start = m.end();
                    if trimmed[text_start..].starts_with(' ') {
                        text_start += 1;
                    }
                    return Some(TagMatch {
                        tag,
                        meta,
                        form: TagForm::Conventional,
                        match_range_in_body: (lead_ws + m.start())..(lead_ws + m.end()),
                        text_start_in_body: lead_ws + text_start,
                    });
                }
            }
        }
    }

    None
}

fn tag_known(tag: &str, cfg: &Config) -> bool {
    if cfg.lookup_tag(tag).is_some() {
        return true;
    }
    matches!(cfg.syntax.unknown_tags, UnknownTags::Highlight)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Strong,
    Emphasis,
    Code,
}

#[derive(Debug, Clone)]
pub struct Span {
    pub range: std::ops::Range<usize>, // absolute byte offsets in source
    pub kind: SpanKind,
}

#[derive(Debug, Clone)]
pub struct InlineResult {
    pub spans: Vec<Span>,
}

/// Scan `region` (absolute byte offsets) for inline spans.
/// `region` must not contain '\n' (we operate per line piece).
pub fn scan_inline(region: &str, region_abs_start: usize, cfg: &Config) -> Vec<Span> {
    if matches!(cfg.syntax.inline, InlineMode::Off) {
        return vec![];
    }
    let mut spans = Vec::new();
    let mut code_ranges: Vec<std::ops::Range<usize>> = Vec::new();

    // 1. Code spans: `...`
    let mut i = 0usize;
    let bytes = region.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // find closing backtick
            if let Some(close_rel) = region[i + 1..].find('`') {
                let close = i + 1 + close_rel;
                let content = &region[i + 1..close];
                if !content.is_empty() && !content.contains('\n') {
                    spans.push(Span {
                        range: (region_abs_start + i)..(region_abs_start + close + 1),
                        kind: SpanKind::Code,
                    });
                    code_ranges.push((region_abs_start + i)..(region_abs_start + close + 1));
                    i = close + 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    let in_code = |abs_start: usize, abs_end: usize| -> bool {
        code_ranges.iter().any(|r| abs_start >= r.start && abs_end <= r.end)
    };

    let star_bold = matches!(cfg.syntax.emphasis, crate::config::EmphasisDialect::StarBold);

    // 2. Emphasis / strong
    let mut j = 0usize;
    while j < region.len() {
        let abs = region_abs_start + j;
        if in_code(abs, abs + 1) {
            j += 1;
            continue;
        }
        if region[j..].starts_with("**") {
            // try **..**
            if let Some(close_rel) = region[j + 2..].find("**") {
                let close_start = j + 2 + close_rel;
                let close_end = close_start + 2;
                if !in_code(region_abs_start + close_start, region_abs_start + close_end) {
                    let content = &region[j + 2..close_start];
                    if !content.is_empty()
                        && !content.starts_with(|c: char| c.is_whitespace())
                        && !content.ends_with(|c: char| c.is_whitespace())
                        && !content.contains('\n')
                    {
                        // opener not followed by ws (already checked), closer not preceded by ws,
                        // and `a * b` style: opener preceded by space handled by content check,
                        // but also require opener not preceded by ... CommonMark: opener right-flanking rules
                        // Simplified: also reject if opener is preceded by '*' (already handled) and if
                        // the char before opener is alphanumeric and content looks like expression?
                        // We'll additionally require the char before "**" is not an alnum directly without space?
                        // For **bold**, preceded is usually space/start.
                        spans.push(Span {
                            range: (region_abs_start + j)..(region_abs_start + close_end),
                            kind: SpanKind::Strong,
                        });
                        j = close_end;
                        continue;
                    }
                }
            }
            j += 2;
            continue;
        }
        if region[j..].starts_with('*') {
            // single *
            if let Some(close_rel) = region[j + 1..].find('*') {
                let close_start = j + 1 + close_rel;
                // ensure close is single * not ** (if **, it would have been handled at close position;
                // but a single opener closing on first * of ** is wrong—skip)
                if region[close_start..].starts_with("**") {
                    j += 1;
                    continue;
                }
                let close_end = close_start + 1;
                if !in_code(region_abs_start + close_start, region_abs_start + close_end) {
                    let content = &region[j + 1..close_start];
                    if !content.is_empty()
                        && content.chars().count() >= 1
                        && !content.starts_with(|c: char| c.is_whitespace())
                        && !content.ends_with(|c: char| c.is_whitespace())
                        && !content.contains('\n')
                        && !content.contains('*')
                        && !content.contains('`')
                    {
                        // Reject `a * b` already covered (content starts with space)
                        // Reject `int *p, *q`: first opener `*p, ` -> content "p, " ends with space => reject.
                        // Reject list marker `* item`: content starts with space => reject.
                        // Reject multiplication with spaces around both sides: covered.
                        // Extra guard: reject if content contains " * " etc — already covered.
                        // Also reject if opener preceded by alnum and followed by alnum with no spaces
                        // e.g. `a*b` — CommonMark would actually render italic! We keep it (CommonMark-correct).
                        let kind = if star_bold { SpanKind::Strong } else { SpanKind::Emphasis };
                        spans.push(Span {
                            range: (region_abs_start + j)..(region_abs_start + close_end),
                            kind,
                        });
                        j = close_end;
                        continue;
                    }
                }
            }
            j += 1;
            continue;
        }
        // advance by char
        let ch_len = region[j..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        j += ch_len;
    }

    spans.sort_by_key(|s| s.range.start);
    spans
}
