use crate::lang::Lang;
use streaming_iterator::StreamingIterator;

/// One logical comment body (already stripped of its delimiter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPiece {
    /// Byte offset of `body` in the original source.
    pub body_start: usize,
    /// Body text (slice of source text).
    pub body: String,
}

pub fn extract_pieces(lang: Lang, src: &str) -> Vec<CommentPiece> {
    if let Some(pieces) = extract_tree_sitter(lang, src) {
        return pieces;
    }
    extract_fallback(lang, src)
}

fn extract_tree_sitter(lang: Lang, src: &str) -> Option<Vec<CommentPiece>> {
    let language = lang.ts_language()?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(src, None)?;
    let query_src = lang.query_source();
    let query = tree_sitter::Query::new(&language, query_src).ok()?;
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), src.as_bytes());
    let mut pieces = Vec::new();
    loop {
        matches.advance();
        let Some(m) = matches.get() else { break };
        for capture in m.captures {
            let node = capture.node;
            let start = node.start_byte();
            let end = node.end_byte();
            if start >= end { continue; }
            let node_text = &src[start..end];
            pieces.extend(strip_delimiter(node_text, start));
        }
    }
    pieces.sort_by_key(|p| p.body_start);
    pieces.dedup();
    Some(pieces)
}

/// Strip comment delimiters, producing one piece per logical line.
/// For block comments, splits into per-line pieces (semantic tokens cannot
/// span lines; multi-line tag bodies are out of scope in v1).
pub fn strip_delimiter(node_text: &str, node_start: usize) -> Vec<CommentPiece> {
    // Identify kind
    if node_text.starts_with("//") {
        let delim_len = if node_text.starts_with("///") || node_text.starts_with("//!") { 3 } else { 2 };
        let body_off = node_start + delim_len;
        let body = &node_text[delim_len..];
        // Return single piece (line comments have no embedded newline; but be safe: split by \n)
        return split_lines(body, body_off);
    }
    if node_text.starts_with("/*") {
        let mut pieces = Vec::new();
        // Walk lines within the block comment
        let mut offset = node_start;
        for (idx, line) in node_text.split('\n').enumerate() {
            // Each `line` is a slice; its byte offset in node_text is accumulated
            // We recompute by tracking offset.
            // First line: strip "/*" (+ one extra '*' or '!' for doc comments)
            // Last line: strip trailing "*/"
            // Middle lines: strip leading whitespace + optional '*' gutter
            let mut piece = line.to_string();
            let mut piece_start = offset;
            if idx == 0 {
                let mut strip = 2;
                if line.len() > 2 && (line.as_bytes()[2] == b'*' || line.as_bytes()[2] == b'!') {
                    strip += 1;
                }
                if strip <= piece.len() {
                    // trim_start spaces after delimiter
                    let rest = &piece[strip..];
                    let trimmed = rest.trim_start_matches(|c: char| c == ' ' || c == '\t');
                    let skipped = rest.len() - trimmed.len();
                    piece = trimmed.to_string();
                    piece_start = offset + strip + skipped;
                } else {
                    piece = String::new();
                    piece_start = offset + piece.len();
                }
            } else {
                // middle or last line: strip leading ws + '*' gutter
                let t = piece.trim_start_matches(|c: char| c == ' ' || c == '\t');
                let skipped = piece.len() - t.len();
                let mut rest = t;
                let mut rest_off = skipped;
                if rest.starts_with('*') {
                    // Strip gutter '*' unless it's the closing '*/' already handled below
                    rest = &rest[1..];
                    rest_off += 1;
                    let tt = rest.trim_start_matches(|c: char| c == ' ' || c == '\t');
                    rest_off += rest.len() - tt.len();
                    rest = tt;
                }
                piece = rest.to_string();
                piece_start = offset + rest_off;
            }
            if idx == node_text.split('\n').count() - 1 {
                // Strip trailing "*/" (allowing trailing whitespace before it)
                let trimmed_end = piece.trim_end_matches(|c: char| c == ' ' || c == '\t' || c == '\r');
                // Check for "*/" at end
                let without_ws = trimmed_end;
                if without_ws.ends_with("*/") {
                    let cut = without_ws.len() - 2;
                    piece = without_ws[..cut].to_string();
                } else {
                    piece = trimmed_end.to_string();
                }
            }
            if !piece.is_empty() {
                pieces.push(CommentPiece { body_start: piece_start, body: piece });
            }
            offset += line.len() + 1; // +1 for '\n'
        }
        return pieces;
    }
    if node_text.starts_with('#') {
        // Python (also shell-style). Strip one '#', then subsequent '#'s/spaces handled by tag trim.
        let delim_len = 1;
        let body_off = node_start + delim_len;
        let body = &node_text[delim_len..];
        return split_lines(body, body_off);
    }
    // Unknown kind: treat whole node as body
    vec![CommentPiece { body_start: node_start, body: node_text.to_string() }]
}

fn split_lines(body: &str, body_off: usize) -> Vec<CommentPiece> {
    // `body` is a &str slice; need byte offsets per line
    let mut out = Vec::new();
    let mut off = body_off;
    let mut start_idx = 0usize;
    for (i, b) in body.bytes().enumerate() {
        if b == b'\n' {
            let line_body = &body[start_idx..i];
            // trim trailing \r
            let line_body = line_body.strip_suffix('\r').unwrap_or(line_body);
            if !line_body.is_empty() {
                out.push(CommentPiece { body_start: off, body: line_body.to_string() });
            }
            off += (i - start_idx) + 1;
            start_idx = i + 1;
        }
    }
    let last = &body[start_idx..];
    let last = last.strip_suffix('\r').unwrap_or(last);
    if !last.is_empty() {
        out.push(CommentPiece { body_start: off, body: last.to_string() });
    }
    out
}

/// Best-effort fallback using a delimiter table (strings may produce false positives).
pub fn extract_fallback(lang: Lang, src: &str) -> Vec<CommentPiece> {
    let table = lang.delimiter_table();
    let mut pieces = Vec::new();
    // Block comments first: find /* ... */
    if !table.block.is_empty() {
        let (open, close) = table.block[0];
        let mut pos = 0usize;
        while let Some(rel) = src[pos..].find(open) {
            let start = pos + rel;
            if let Some(end_rel) = src[start + open.len()..].find(close) {
                let end = start + open.len() + end_rel + close.len();
                let node_text = &src[start..end];
                pieces.extend(strip_delimiter(node_text, start));
                pos = end;
                continue;
            } else {
                break;
            }
        }
    }
    // Line comments, line by line
    let mut line_start = 0usize;
    for line in src.split_inclusive('\n') {
        let line_content = line.strip_suffix('\n').unwrap_or(line);
        for delim in table.line {
            if let Some(rel) = line_content.find(delim) {
                // Heuristic: delimiter must be at start (after ws) or preceded by ws
                let before = &line_content[..rel];
                let ok = before.trim().is_empty() || before.ends_with(' ') || before.ends_with('\t');
                // Also avoid matching 'http://' style: if preceded by ':' skip
                if ok && !before.ends_with(':') {
                    let comment_start = line_start + rel;
                    let node_text = &line_content[rel..];
                    // Strip trailing \r
                    let node_text = node_text.strip_suffix('\r').unwrap_or(node_text);
                    pieces.extend(strip_delimiter(node_text, comment_start));
                    break;
                }
            }
        }
        line_start += line.len();
    }
    pieces.sort_by_key(|p| p.body_start);
    pieces.dedup();
    pieces
}
