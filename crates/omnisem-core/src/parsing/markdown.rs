//! Deterministic Markdown parser (`markdown-v1`) built on `pulldown-cmark`.
//!
//! Selected over `comrak` because the required Milestone 1 segment model needs a
//! stable event stream for headings, paragraphs, lists, quotes, fences, links,
//! and tables, without requiring a full Markdown AST library or HTML rendering.

use std::collections::HashMap;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::domain::{SegmentType, SupportedFileType};
use crate::parsing::{
    DocumentParser, ParseError, ParsedDocument, ParsedSegment, ParserWarning, SourceDocument,
};

/// Maximum characters kept in one structural segment before deterministic split.
const MAX_SEGMENT_CHARS: usize = 8_192;

/// Stable Markdown parser identity and versioned output semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkdownParser;

impl DocumentParser for MarkdownParser {
    fn parser_id(&self) -> &'static str {
        "markdown-v1"
    }

    fn parser_version(&self) -> &'static str {
        "1"
    }

    fn supports(&self, document: &crate::domain::DiscoveredDocument) -> bool {
        document.file_type == SupportedFileType::Markdown
    }

    fn parse(&self, source: &SourceDocument<'_>) -> Result<ParsedDocument, ParseError> {
        let text = std::str::from_utf8(source.bytes).map_err(|_| ParseError::InvalidUtf8)?;
        Ok(parse_markdown(text))
    }
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct MarkdownParseState {
    segments: Vec<ParsedSegment>,
    warnings: Vec<ParserWarning>,
    title: Option<String>,
    ordinal: u32,
    anchors: HashMap<String, u32>,
    heading_stack: Vec<(u8, String)>,
    text_buf: String,
    blockquote_buf: String,
    list_buf: String,
    code_buf: String,
    code_lang: String,
    table_buf: String,
    link_texts: Vec<(String, String)>,
    in_heading: bool,
    heading_level: u8,
    in_paragraph: bool,
    in_blockquote: bool,
    in_list: bool,
    in_item: bool,
    in_code: bool,
    in_table: bool,
    paragraph_count: u32,
    list_count: u32,
    quote_count: u32,
    code_count: u32,
    table_count: u32,
}

impl MarkdownParseState {
    fn push(&mut self, segment_type: SegmentType, base_anchor: String, text: String) {
        let count = self.anchors.entry(base_anchor.clone()).or_insert(0);
        *count += 1;
        let anchor = if *count == 1 {
            base_anchor
        } else {
            format!("{base_anchor}~{count}")
        };
        self.segments.push(ParsedSegment {
            segment_type,
            anchor,
            ordinal: self.ordinal,
            text,
        });
        self.ordinal += 1;
    }

    fn append_text(&mut self, value: &str) {
        if self.in_heading || self.in_paragraph {
            self.text_buf.push_str(value);
        } else if self.in_blockquote {
            self.blockquote_buf.push_str(value);
        } else if self.in_item {
            self.list_buf.push_str(value);
        } else if self.in_table {
            self.table_buf.push_str(value);
        }
    }

    fn finish_heading(&mut self) {
        self.in_heading = false;
        let heading_text = self.text_buf.trim().to_owned();
        self.text_buf.clear();
        if heading_text.is_empty() {
            return;
        }

        let slug = slugify(&heading_text);
        self.heading_stack
            .retain(|(level, _)| *level < self.heading_level);
        self.heading_stack.push((self.heading_level, slug.clone()));
        let path = self
            .heading_stack
            .iter()
            .map(|(_, part)| part.as_str())
            .collect::<Vec<_>>()
            .join("/");

        if self.title.is_none() && self.heading_level == 1 {
            self.title = Some(heading_text.clone());
            self.push(
                SegmentType::DocumentTitle,
                format!("title:{slug}"),
                heading_text.clone(),
            );
        }
        self.push(
            SegmentType::Heading,
            format!("heading:{path}"),
            heading_text,
        );
    }

    fn finish_paragraph(&mut self) {
        self.in_paragraph = false;
        let paragraph = self.text_buf.trim().to_owned();
        self.text_buf.clear();
        if paragraph.is_empty() {
            return;
        }
        self.paragraph_count += 1;
        for piece in split_oversized(&paragraph, MAX_SEGMENT_CHARS) {
            self.push(
                SegmentType::Paragraph,
                format!("paragraph:{}", self.paragraph_count),
                piece,
            );
        }
    }

    fn finish_blockquote(&mut self) {
        self.in_blockquote = false;
        let quote = self.blockquote_buf.trim().to_owned();
        self.blockquote_buf.clear();
        if quote.is_empty() {
            return;
        }
        self.quote_count += 1;
        self.push(
            SegmentType::Blockquote,
            format!("blockquote:{}", self.quote_count),
            quote,
        );
    }

    fn finish_list(&mut self) {
        self.in_list = false;
        let list = self.list_buf.trim().to_owned();
        self.list_buf.clear();
        if list.is_empty() {
            return;
        }
        self.list_count += 1;
        self.push(SegmentType::List, format!("list:{}", self.list_count), list);
    }

    fn finish_code(&mut self) {
        self.in_code = false;
        self.code_count += 1;
        let lang = if self.code_lang.is_empty() {
            "text".to_owned()
        } else {
            slugify(&self.code_lang)
        };
        let code = self.code_buf.trim_end_matches('\n').to_owned();
        self.code_buf.clear();
        self.code_lang.clear();
        for piece in split_oversized(&code, MAX_SEGMENT_CHARS) {
            self.push(
                SegmentType::CodeFence,
                format!("code-fence:{lang}:{}", self.code_count),
                piece,
            );
        }
    }

    fn finish_table(&mut self) {
        self.in_table = false;
        let table = self.table_buf.trim().to_owned();
        self.table_buf.clear();
        if table.is_empty() {
            return;
        }
        self.table_count += 1;
        self.push(
            SegmentType::Table,
            format!("table:{}", self.table_count),
            table,
        );
    }

    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(value) | Event::Code(value) => self.text_or_code(value.as_ref()),
            Event::SoftBreak | Event::HardBreak => self.append_text(" "),
            Event::Rule => {
                self.warnings.push(ParserWarning {
                    code: "thematic_break".into(),
                    message: "thematic breaks are ignored as segments".into(),
                });
            }
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::TaskListMarker(_) => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.in_heading = true;
                self.heading_level = level as u8;
                self.text_buf.clear();
            }
            Tag::Paragraph => {
                if !self.in_list && !self.in_blockquote && !self.in_table {
                    self.in_paragraph = true;
                    self.text_buf.clear();
                }
            }
            Tag::BlockQuote(_) => {
                self.in_blockquote = true;
                self.blockquote_buf.clear();
            }
            Tag::List(_) => {
                self.in_list = true;
            }
            Tag::Item => {
                self.in_item = true;
                if !self.list_buf.is_empty() {
                    self.list_buf.push('\n');
                }
                self.list_buf.push_str("- ");
            }
            Tag::CodeBlock(kind) => {
                self.in_code = true;
                self.code_buf.clear();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::Table(_) => {
                self.in_table = true;
                self.table_buf.clear();
            }
            Tag::TableCell => {
                if self.in_table && !self.table_buf.is_empty() && !self.table_buf.ends_with('\n') {
                    self.table_buf.push('|');
                }
            }
            Tag::Link { dest_url, .. } => {
                self.link_texts
                    .push((String::new(), dest_url.into_string()));
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => self.finish_heading(),
            TagEnd::Paragraph => {
                if self.in_paragraph {
                    self.finish_paragraph();
                }
            }
            TagEnd::BlockQuote(_) => self.finish_blockquote(),
            TagEnd::List(_) => self.finish_list(),
            TagEnd::Item => self.in_item = false,
            TagEnd::CodeBlock => self.finish_code(),
            TagEnd::Table => self.finish_table(),
            TagEnd::TableHead => {
                if self.in_table && !self.table_buf.is_empty() {
                    self.table_buf.push('\n');
                }
            }
            TagEnd::TableRow => {
                if self.in_table && !self.table_buf.ends_with('\n') {
                    self.table_buf.push('\n');
                }
            }
            TagEnd::Link => {
                if let Some((label, url)) = self.link_texts.pop() {
                    let rendered = if label.is_empty() {
                        url
                    } else {
                        format!("{label} ({url})")
                    };
                    self.append_text(&rendered);
                }
            }
            _ => {}
        }
    }

    fn text_or_code(&mut self, value: &str) {
        if self.in_code {
            self.code_buf.push_str(value);
        } else if let Some((label, _)) = self.link_texts.last_mut() {
            label.push_str(value);
        } else {
            self.append_text(value);
        }
    }
}

fn parse_markdown(input: &str) -> ParsedDocument {
    let (frontmatter, body) = split_frontmatter(input);
    let mut state = MarkdownParseState {
        heading_level: 1,
        ..MarkdownParseState::default()
    };

    if let Some(frontmatter) = frontmatter {
        if let Some(front_title) = frontmatter_title(&frontmatter) {
            state.title = Some(front_title);
        }
        state.push(SegmentType::Frontmatter, "frontmatter".into(), frontmatter);
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    for event in Parser::new_ext(body, options) {
        state.handle_event(event);
    }

    if state.segments.is_empty() {
        let trimmed = body.trim();
        if !trimmed.is_empty() {
            state.push(
                SegmentType::Paragraph,
                "paragraph:1".into(),
                trimmed.to_owned(),
            );
        }
    }

    ParsedDocument {
        title: state.title,
        segments: state.segments,
        warnings: state.warnings,
    }
}

fn split_frontmatter(input: &str) -> (Option<String>, &str) {
    let bytes = input.as_bytes();
    if !input.starts_with("---") {
        return (None, input);
    }
    let rest = if bytes.get(3) == Some(&b'\n') {
        &input[4..]
    } else if bytes.get(3..5) == Some(b"\r\n") {
        &input[5..]
    } else {
        return (None, input);
    };

    for (index, _) in rest.match_indices("\n---") {
        let after = &rest[index + 1..];
        let Some(closing) = after.strip_prefix("---") else {
            continue;
        };
        if !(closing.starts_with('\n')
            || closing.starts_with("\r\n")
            || closing.is_empty()
            || closing == "\r")
        {
            continue;
        }
        let front = rest[..index].to_owned();
        let body = closing
            .strip_prefix("\r\n")
            .or_else(|| closing.strip_prefix('\n'))
            .unwrap_or(closing);
        return (Some(front), body);
    }
    (None, input)
}

fn frontmatter_title(frontmatter: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("title:") {
            let value = value.trim().trim_matches('"').trim_matches('\'').to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "section".into()
    } else {
        slug
    }
}

fn split_oversized(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return vec![text.to_owned()];
    }
    let mut pieces = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let hard_end = (start + max_chars).min(chars.len());
        let mut end = hard_end;
        if hard_end < chars.len()
            && let Some(relative) = chars[start..hard_end]
                .iter()
                .rposition(|ch| *ch == '\n' || *ch == ' ')
        {
            let candidate = start + relative + 1;
            if candidate > start {
                end = candidate;
            }
        }
        pieces.push(chars[start..end].iter().collect());
        start = end;
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DiscoveredDocument, RootId, Timestamp};
    use std::path::PathBuf;

    fn document() -> DiscoveredDocument {
        DiscoveredDocument {
            root_id: RootId::new(),
            canonical_path: PathBuf::from("/approved/a.md"),
            relative_path: PathBuf::from("a.md"),
            size_bytes: 1,
            modified_at: Timestamp::from_millis(1),
            file_type: SupportedFileType::Markdown,
        }
    }

    fn parse(bytes: &[u8]) -> ParsedDocument {
        MarkdownParser
            .parse(&SourceDocument {
                discovered: &document(),
                bytes,
            })
            .unwrap()
    }

    #[test]
    fn heading_hierarchy_and_paragraph_order() {
        let parsed =
            parse(b"# Architecture\n\nIntro paragraph.\n\n## Storage\n\nStorage details.\n");
        assert_eq!(parsed.title.as_deref(), Some("Architecture"));
        let anchors: Vec<_> = parsed
            .segments
            .iter()
            .map(|segment| segment.anchor.as_str())
            .collect();
        assert!(anchors.contains(&"heading:architecture"));
        assert!(anchors.contains(&"heading:architecture/storage"));
        let paragraphs: Vec<_> = parsed
            .segments
            .iter()
            .filter(|segment| segment.segment_type == SegmentType::Paragraph)
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(paragraphs, vec!["Intro paragraph.", "Storage details."]);
        for (index, segment) in parsed.segments.iter().enumerate() {
            assert_eq!(segment.ordinal, u32::try_from(index).unwrap());
        }
    }

    #[test]
    fn fenced_code_blocks_and_links() {
        let parsed = parse(b"See [docs](https://example.com).\n\n```rust\nfn main() {}\n```\n");
        let paragraph = parsed
            .segments
            .iter()
            .find(|segment| segment.segment_type == SegmentType::Paragraph)
            .unwrap();
        assert!(paragraph.text.contains("docs (https://example.com)"));
        let code = parsed
            .segments
            .iter()
            .find(|segment| segment.segment_type == SegmentType::CodeFence)
            .unwrap();
        assert_eq!(code.anchor, "code-fence:rust:1");
        assert!(code.text.contains("fn main()"));
    }

    #[test]
    fn frontmatter_is_extracted() {
        let parsed = parse(b"---\ntitle: Foundation fixture\n---\n\n# Architecture\n\nBody.\n");
        assert_eq!(parsed.title.as_deref(), Some("Foundation fixture"));
        assert_eq!(parsed.segments[0].segment_type, SegmentType::Frontmatter);
        assert!(
            parsed.segments[0]
                .text
                .contains("title: Foundation fixture")
        );
    }

    #[test]
    fn repeated_headings_get_stable_unique_anchors() {
        let parsed = parse(b"# Alpha\n\n# Alpha\n\n## Beta\n\n## Beta\n");
        let headings: Vec<_> = parsed
            .segments
            .iter()
            .filter(|segment| segment.segment_type == SegmentType::Heading)
            .map(|segment| segment.anchor.as_str())
            .collect();
        assert!(headings.contains(&"heading:alpha"));
        assert!(headings.contains(&"heading:alpha~2"));
        assert_eq!(
            headings.len(),
            headings
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        );
    }

    #[test]
    fn lists_blockquotes_and_tables() {
        let parsed = parse(b"> quoted\n\n- one\n- two\n\n| A | B |\n| - | - |\n| 1 | 2 |\n");
        assert!(parsed.segments.iter().any(|segment| {
            segment.segment_type == SegmentType::Blockquote && segment.text.contains("quoted")
        }));
        assert!(parsed.segments.iter().any(|segment| {
            segment.segment_type == SegmentType::List
                && segment.text.contains("- one")
                && segment.text.contains("- two")
        }));
        assert!(parsed.segments.iter().any(|segment| {
            segment.segment_type == SegmentType::Table && segment.text.contains('A')
        }));
    }

    #[test]
    fn invalid_utf8_is_rejected_without_partial_output() {
        let error = MarkdownParser
            .parse(&SourceDocument {
                discovered: &document(),
                bytes: b"# ok\n\xff",
            })
            .unwrap_err();
        assert_eq!(error, ParseError::InvalidUtf8);
    }

    #[test]
    fn anchors_are_stable_across_parses() {
        let source = b"# Title\n\nParagraph one.\n\n## Nested\n\nMore.\n";
        assert_eq!(parse(source), parse(source));
    }
}
