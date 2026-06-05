//! Markdown + wikilink parsing for the knowledge index.
//!
//! Walks standard Markdown with `pulldown-cmark` to find code spans/blocks and
//! headings, then scans the raw text for `[[wikilinks]]` and `#tags`, **skipping
//! anything inside code** (fenced / indented / inline). All positions follow the
//! D14 coordinate contract: code-point offsets + 1-based line / 0-based
//! code-point column, computed relative to the *original full file* (frontmatter
//! and original CRLF included) via [`PosMap`].

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use unicode_normalization::UnicodeNormalization;

use super::types::LinkType;

/// A position in the file under the D14 coordinate contract.
#[derive(Debug, Clone, Copy)]
pub struct Pos {
    /// Unicode code-point offset from start of file.
    pub offset: u32,
    /// 1-based line number (split on `\n`).
    pub line: u32,
    /// 0-based code-point column within the line (tab counts as one).
    pub col: u32,
}

/// Byte-offset → (code-point offset, line, col) mapper over the full file.
pub struct PosMap<'a> {
    text: &'a str,
    /// Byte offset of each line start (line 0 starts at byte 0).
    line_starts: Vec<usize>,
    /// Code-point count at each line start.
    cp_at_line_start: Vec<usize>,
}

impl<'a> PosMap<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        let mut cp_at_line_start = vec![0usize];
        let mut cp = 0usize;
        for (i, ch) in text.char_indices() {
            cp += 1;
            if ch == '\n' {
                line_starts.push(i + ch.len_utf8());
                cp_at_line_start.push(cp);
            }
        }
        Self {
            text,
            line_starts,
            cp_at_line_start,
        }
    }

    /// Map a byte offset (must be a char boundary) to a [`Pos`].
    pub fn pos(&self, byte_off: usize) -> Pos {
        let byte_off = byte_off.min(self.text.len());
        let line_idx = match self.line_starts.binary_search(&byte_off) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        // `\r` only appears at a line's end (before `\n`), so counting code
        // points from the line start to a mid-line offset never includes it.
        let col = self.text[line_start..byte_off].chars().count();
        Pos {
            offset: (self.cp_at_line_start[line_idx] + col) as u32,
            line: (line_idx + 1) as u32,
            col: col as u32,
        }
    }
}

/// A heading occurrence with its level and start position.
#[derive(Debug, Clone)]
pub struct Heading {
    pub level: u32,
    pub title: String,
    pub byte_start: usize,
}

/// A wikilink occurrence with parsed components and source position.
#[derive(Debug, Clone)]
pub struct ParsedLink {
    pub target_ref: String,
    pub anchor: Option<String>,
    pub alias: Option<String>,
    pub link_type: LinkType,
    pub raw_text: String,
    pub start: Pos,
    pub end: Pos,
    pub heading_path: Option<String>,
}

/// Result of parsing a note file.
pub struct ParsedDoc {
    /// frontmatter `title` > first H1 > (filled by caller from file stem).
    pub title: Option<String>,
    /// frontmatter parsed to JSON (opaque), `None` if absent / unparseable.
    pub frontmatter_json: Option<String>,
    /// Byte offset where the body begins (after a frontmatter block, else 0).
    pub body_start_byte: usize,
    pub headings: Vec<Heading>,
    pub links: Vec<ParsedLink>,
    /// Normalized (NFC + lowercase), de-duplicated tags.
    pub tags: Vec<String>,
}

/// Parse a note file's full raw content.
pub fn parse_document(full: &str) -> ParsedDoc {
    let posmap = PosMap::new(full);

    // 1. Frontmatter envelope: leading `---\n ... \n---`.
    let (frontmatter_raw, body_start_byte) = split_frontmatter(full);
    let frontmatter = frontmatter_raw.map(parse_frontmatter_to_json);
    let frontmatter_json = frontmatter
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());

    let body = &full[body_start_byte..];

    // 2. Walk markdown structure (over the body slice, so YAML frontmatter never
    //    confuses pulldown-cmark). All ranges are body-relative → add
    //    `body_start_byte` to map back to full-file coordinates.
    let mut code_ranges: Vec<Range<usize>> = Vec::new();
    let mut headings: Vec<Heading> = Vec::new();
    let mut first_h1: Option<String> = None;

    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let mut iter = Parser::new_ext(body, opts).into_offset_iter();

    let mut cur_heading: Option<(u32, usize, String)> = None; // (level, byte_start, accum text)
    let mut code_block_start: Option<usize> = None;

    while let Some((ev, range)) = iter.next() {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                cur_heading = Some((
                    heading_level(level),
                    body_start_byte + range.start,
                    String::new(),
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, byte_start, title)) = cur_heading.take() {
                    let title = title.trim().to_string();
                    if level == 1 && first_h1.is_none() && !title.is_empty() {
                        first_h1 = Some(title.clone());
                    }
                    headings.push(Heading {
                        level,
                        title,
                        byte_start,
                    });
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
            | Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => {
                code_block_start = Some(body_start_byte + range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = code_block_start.take() {
                    code_ranges.push(start..(body_start_byte + range.end));
                }
            }
            Event::Code(_) => {
                code_ranges.push((body_start_byte + range.start)..(body_start_byte + range.end));
            }
            Event::Text(t) => {
                if let Some((_, _, ref mut accum)) = cur_heading {
                    accum.push_str(&t);
                }
            }
            _ => {}
        }
    }

    // 3. Scan the full file for wikilinks + tags, skipping code ranges. We scan
    //    over `full` (not `body`) but ignore anything before `body_start_byte`
    //    via the frontmatter cut, since links/tags in frontmatter aren't body
    //    references. Coordinates stay full-file.
    let links = scan_wikilinks(full, body_start_byte, &code_ranges, &headings, &posmap);
    let mut tags = scan_tags(full, body_start_byte, &code_ranges);

    // Merge frontmatter tags.
    if let Some(fm) = &frontmatter {
        collect_frontmatter_tags(fm, &mut tags);
    }
    tags.sort();
    tags.dedup();

    let title = frontmatter
        .as_ref()
        .and_then(|v| v.get("title"))
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or(first_h1);

    ParsedDoc {
        title,
        frontmatter_json,
        body_start_byte,
        headings,
        links,
        tags,
    }
}

fn heading_level(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Build the `a > b > c` heading path active at a byte offset, by walking the
/// headings before it and keeping the level hierarchy.
pub fn heading_path_at(byte_off: usize, headings: &[Heading]) -> Option<String> {
    let mut stack: Vec<&Heading> = Vec::new();
    for h in headings {
        if h.byte_start > byte_off {
            break;
        }
        while let Some(last) = stack.last() {
            if last.level >= h.level {
                stack.pop();
            } else {
                break;
            }
        }
        stack.push(h);
    }
    if stack.is_empty() {
        None
    } else {
        Some(
            stack
                .iter()
                .map(|h| h.title.as_str())
                .collect::<Vec<_>>()
                .join(" > "),
        )
    }
}

/// Detect a leading YAML frontmatter block. Returns `(Some(yaml), body_start)`
/// or `(None, 0)`. Mirrors the hand-rolled detection in `skills/frontmatter.rs`.
fn split_frontmatter(full: &str) -> (Option<&str>, usize) {
    // Allow an optional UTF-8 BOM.
    let start = full.strip_prefix('\u{feff}').map(|_| 3).unwrap_or(0);
    let rest = &full[start..];
    if !(rest.starts_with("---\n") || rest.starts_with("---\r\n")) {
        return (None, 0);
    }
    let after_open = if rest.starts_with("---\r\n") {
        start + 5
    } else {
        start + 4
    };
    // Closing fence: a line that is exactly `---` (with optional CR).
    let hay = &full[after_open..];
    let mut idx = 0usize;
    for line in hay.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            let yaml = &full[after_open..after_open + idx];
            let body_start = after_open + idx + line.len();
            return (Some(yaml), body_start);
        }
        idx += line.len();
    }
    (None, 0)
}

/// Minimal YAML→JSON for frontmatter. Handles `key: scalar`, `key: [a, b]`, and
/// block lists (`key:` then `- item` lines). Sufficient for title + tags; not a
/// full YAML implementation (project convention: no YAML crate).
fn parse_frontmatter_to_json(yaml: &str) -> serde_json::Value {
    use serde_json::{Map, Value};
    let mut map = Map::new();
    let lines: Vec<&str> = yaml.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        // Only handle top-level (non-indented) keys.
        if line.starts_with(char::is_whitespace) {
            i += 1;
            continue;
        }
        if let Some(colon) = trimmed.find(':') {
            let key = trimmed[..colon].trim().to_string();
            let val = trimmed[colon + 1..].trim();
            if val.is_empty() {
                // Possible block list following.
                let mut items: Vec<Value> = Vec::new();
                let mut j = i + 1;
                while j < lines.len() {
                    let l = lines[j];
                    let lt = l.trim();
                    if l.starts_with(char::is_whitespace) && lt.starts_with("- ") {
                        items.push(Value::String(unquote(lt[2..].trim()).to_string()));
                        j += 1;
                    } else if lt.is_empty() {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if items.is_empty() {
                    map.insert(key, Value::Null);
                } else {
                    map.insert(key, Value::Array(items));
                    i = j;
                    continue;
                }
            } else if val.starts_with('[') && val.ends_with(']') {
                let inner = &val[1..val.len() - 1];
                let items: Vec<Value> = inner
                    .split(',')
                    .map(|s| Value::String(unquote(s.trim()).to_string()))
                    .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
                    .collect();
                map.insert(key, Value::Array(items));
            } else {
                map.insert(key, Value::String(unquote(val).to_string()));
            }
        }
        i += 1;
    }
    Value::Object(map)
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn collect_frontmatter_tags(fm: &serde_json::Value, out: &mut Vec<String>) {
    let Some(tags) = fm.get("tags").or_else(|| fm.get("tag")) else {
        return;
    };
    match tags {
        serde_json::Value::Array(arr) => {
            for t in arr {
                if let Some(s) = t.as_str() {
                    push_tag(s, out);
                }
            }
        }
        serde_json::Value::String(s) => {
            for piece in s.split([',', ' ']) {
                push_tag(piece, out);
            }
        }
        _ => {}
    }
}

fn push_tag(raw: &str, out: &mut Vec<String>) {
    let t = raw.trim().trim_start_matches('#').trim();
    if t.is_empty() {
        return;
    }
    out.push(normalize_tag(t));
}

/// NFC + lowercase, the canonical tag form used by `note_tag` and resolve.
pub fn normalize_tag(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase()
}

/// NFC normalize (case preserved) — used for path/title resolve keys with a
/// separate case-fold compare.
pub fn nfc(s: &str) -> String {
    s.nfc().collect::<String>()
}

fn in_code(ranges: &[Range<usize>], pos: usize) -> bool {
    ranges.iter().any(|r| pos >= r.start && pos < r.end)
}

/// Scan `[[wikilink]]` / `![[embed]]` occurrences (skipping code).
fn scan_wikilinks(
    full: &str,
    body_start_byte: usize,
    code_ranges: &[Range<usize>],
    headings: &[Heading],
    posmap: &PosMap,
) -> Vec<ParsedLink> {
    let mut out = Vec::new();
    let bytes = full.as_bytes();
    let mut i = body_start_byte;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Find closing `]]` on the same content (no embedded newline).
            if let Some(close_rel) = find_close(&full[i + 2..]) {
                let inner_start = i + 2;
                let inner_end = inner_start + close_rel;
                let inner = &full[inner_start..inner_end];
                // Embeds carry a leading `!`.
                let is_embed = i > 0 && bytes[i - 1] == b'!';
                let link_start = if is_embed { i - 1 } else { i };
                let link_end = inner_end + 2; // include closing ]]

                if !in_code(code_ranges, i) {
                    if let Some(link) = parse_wikilink_inner(
                        inner, link_start, link_end, full, headings, posmap, is_embed,
                    ) {
                        out.push(link);
                    }
                }
                i = link_end;
                continue;
            }
        }
        // advance by one char boundary
        i += utf8_len(bytes[i]);
    }
    out
}

/// Find the byte index of the closing `]]` within `s`, refusing to cross a
/// newline (wikilinks are single-line).
fn find_close(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'\n' => return None,
            b']' if bytes[i + 1] == b']' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_wikilink_inner(
    inner: &str,
    link_start: usize,
    link_end: usize,
    full: &str,
    headings: &[Heading],
    posmap: &PosMap,
    is_embed: bool,
) -> Option<ParsedLink> {
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    // alias: split on first `|`.
    let (target_and_anchor, alias) = match inner.split_once('|') {
        Some((t, a)) => (t.trim(), Some(a.trim().to_string())),
        None => (inner, None),
    };
    // anchor: split on first `#`.
    let (target_ref, anchor) = match target_and_anchor.split_once('#') {
        Some((t, a)) => (t.trim().to_string(), Some(a.trim().to_string())),
        None => (target_and_anchor.to_string(), None),
    };
    if target_ref.is_empty() && anchor.is_none() {
        return None;
    }
    let raw_text = &full[link_start..link_end];
    Some(ParsedLink {
        target_ref,
        anchor: anchor.filter(|s| !s.is_empty()),
        alias: alias.filter(|s| !s.is_empty()),
        link_type: if is_embed {
            LinkType::Embed
        } else {
            LinkType::Wiki
        },
        raw_text: raw_text.to_string(),
        start: posmap.pos(link_start),
        end: posmap.pos(link_end),
        heading_path: heading_path_at(link_start, headings),
    })
}

/// Scan `#tag` occurrences (skipping code + heading markers).
fn scan_tags(full: &str, body_start_byte: usize, code_ranges: &[Range<usize>]) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = full.as_bytes();
    let mut i = body_start_byte;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            // Preceding char must be start-of-line or whitespace (so `a#b` and
            // URL fragments don't match).
            let prev_ws = i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'\r');
            // Next char must be a tag char (so `# Heading` — `#` then space —
            // is not a tag).
            let next_is_tag = full[i + 1..]
                .chars()
                .next()
                .map(is_tag_char)
                .unwrap_or(false);
            if prev_ws && next_is_tag && !in_code(code_ranges, i) {
                let tag: String = full[i + 1..]
                    .chars()
                    .take_while(|c| is_tag_char(*c))
                    .collect();
                if !tag.is_empty() && tag.chars().any(|c| c.is_alphanumeric()) {
                    out.push(normalize_tag(&tag));
                }
                i += 1 + tag.len();
                continue;
            }
        }
        i += utf8_len(bytes[i]);
    }
    out
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '/'
}

fn utf8_len(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posmap_lines_cols_codepoints() {
        let text = "ab\ncдe\n"; // line2 has a 2-byte cyrillic char
        let pm = PosMap::new(text);
        // byte 0 = 'a'
        let p = pm.pos(0);
        assert_eq!((p.line, p.col, p.offset), (1, 0, 0));
        // start of line 2 (byte 3, after \n)
        let p = pm.pos(3);
        assert_eq!((p.line, p.col), (2, 0));
        // the 'e' after the cyrillic char: bytes c(1) д(2) => byte 6
        let p = pm.pos(6);
        assert_eq!((p.line, p.col), (2, 2)); // col counts code points, not bytes
    }

    #[test]
    fn wikilinks_parsed_with_alias_and_anchor() {
        let doc = parse_document("# Title\n\nSee [[folder/note#Heading|Alias]] here.\n");
        assert_eq!(doc.title.as_deref(), Some("Title"));
        assert_eq!(doc.links.len(), 1);
        let l = &doc.links[0];
        assert_eq!(l.target_ref, "folder/note");
        assert_eq!(l.anchor.as_deref(), Some("Heading"));
        assert_eq!(l.alias.as_deref(), Some("Alias"));
        assert_eq!(l.link_type, LinkType::Wiki);
    }

    #[test]
    fn wikilinks_in_code_are_skipped() {
        let doc = parse_document("Real [[a]]\n\n```\n[[notalink]]\n```\n\n`[[alsonot]]`\n");
        let targets: Vec<&str> = doc.links.iter().map(|l| l.target_ref.as_str()).collect();
        assert_eq!(targets, vec!["a"]);
    }

    #[test]
    fn tags_collected_inline_and_frontmatter() {
        let doc = parse_document(
            "---\ntitle: T\ntags: [alpha, Beta]\n---\n\nbody #gamma and `#nocode`\n",
        );
        assert!(doc.tags.contains(&"alpha".to_string()));
        assert!(doc.tags.contains(&"beta".to_string()));
        assert!(doc.tags.contains(&"gamma".to_string()));
        assert!(!doc.tags.contains(&"nocode".to_string()));
    }

    #[test]
    fn heading_marker_is_not_a_tag() {
        let doc = parse_document("# Heading\n\ntext\n");
        assert!(doc.tags.is_empty());
    }

    #[test]
    fn frontmatter_offsets_keep_full_file_coords() {
        let full = "---\ntitle: T\n---\n\n[[x]]\n";
        let doc = parse_document(full);
        assert!(doc.body_start_byte > 0);
        // The link is on line 5 of the full file.
        assert_eq!(doc.links[0].start.line, 5);
    }
}
