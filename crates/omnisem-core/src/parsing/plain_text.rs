//! Deterministic plain-text fallback parser (`plain-text-v1`).
//!
//! Broadens corpus coverage for UTF-8 textual files that no structured parser
//! claims. It does not provide language-aware structure, symbols, imports, or
//! call relationships.

use crate::domain::{SegmentType, SupportedFileType};
use crate::parsing::{DocumentParser, ParseError, ParsedDocument, ParsedSegment, SourceDocument};

/// Default maximum characters retained in a single plain-text segment.
pub const DEFAULT_MAX_SEGMENT_CHARS: usize = 4_096;

/// Stable plain-text fallback parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlainTextParser;

impl DocumentParser for PlainTextParser {
    fn parser_id(&self) -> &'static str {
        "plain-text-v1"
    }

    fn parser_version(&self) -> &'static str {
        "1"
    }

    fn supports(&self, document: &crate::domain::DiscoveredDocument) -> bool {
        document.file_type == SupportedFileType::PlainText
    }

    fn parse(&self, source: &SourceDocument<'_>) -> Result<ParsedDocument, ParseError> {
        let text = std::str::from_utf8(source.bytes).map_err(|_| ParseError::InvalidUtf8)?;
        let segments = split_plain_text(text, DEFAULT_MAX_SEGMENT_CHARS);
        Ok(ParsedDocument {
            title: None,
            segments,
            warnings: Vec::new(),
        })
    }
}

/// Splits UTF-8 text into deterministic size-bounded segments.
///
/// Small inputs produce a single segment. Larger inputs split on the last newline
/// before the limit when possible, otherwise on the hard character boundary.
#[must_use]
pub fn split_plain_text(text: &str, max_chars: usize) -> Vec<ParsedSegment> {
    let max_chars = max_chars.max(1);
    if text.is_empty() {
        return vec![ParsedSegment {
            segment_type: SegmentType::Paragraph,
            anchor: "text:1".into(),
            ordinal: 0,
            text: String::new(),
        }];
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return vec![ParsedSegment {
            segment_type: SegmentType::Paragraph,
            anchor: "text:1".into(),
            ordinal: 0,
            text: text.to_owned(),
        }];
    }

    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut index = 1u32;

    while start < chars.len() {
        let hard_end = (start + max_chars).min(chars.len());
        let mut end = hard_end;
        if hard_end < chars.len()
            && let Some(relative) = chars[start..hard_end].iter().rposition(|ch| *ch == '\n')
        {
            let candidate = start + relative + 1;
            if candidate > start {
                end = candidate;
            }
        }

        let chunk: String = chars[start..end].iter().collect();
        segments.push(ParsedSegment {
            segment_type: SegmentType::Paragraph,
            anchor: format!("text:{index}"),
            ordinal: index - 1,
            text: chunk,
        });
        index += 1;
        start = end;
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DiscoveredDocument, RootId, Timestamp};
    use std::path::PathBuf;

    fn document() -> DiscoveredDocument {
        DiscoveredDocument {
            root_id: RootId::new(),
            canonical_path: PathBuf::from("/approved/a.txt"),
            relative_path: PathBuf::from("a.txt"),
            size_bytes: 1,
            modified_at: Timestamp::from_millis(1),
            file_type: SupportedFileType::PlainText,
        }
    }

    #[test]
    fn small_file_is_single_segment() {
        let discovered = document();
        let parsed = PlainTextParser
            .parse(&SourceDocument {
                discovered: &discovered,
                bytes: b"hello world",
            })
            .unwrap();
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].anchor, "text:1");
        assert_eq!(parsed.segments[0].ordinal, 0);
        assert_eq!(parsed.segments[0].text, "hello world");
    }

    #[test]
    fn large_text_chunks_deterministically_on_newlines() {
        let body = format!("{}\n{}\n", "a".repeat(20), "b".repeat(20));
        let first = split_plain_text(&body, 25);
        let second = split_plain_text(&body, 25);
        assert_eq!(first, second);
        assert!(first.len() > 1);
        assert!(
            first
                .iter()
                .all(|segment| segment.text.chars().count() <= 25)
        );
        for (index, segment) in first.iter().enumerate() {
            assert_eq!(segment.ordinal, u32::try_from(index).unwrap());
            assert_eq!(segment.anchor, format!("text:{}", index + 1));
        }
        assert_eq!(
            first
                .iter()
                .map(|segment| segment.text.clone())
                .collect::<String>(),
            body
        );
    }

    #[test]
    fn hard_boundary_when_no_newline() {
        let body = "x".repeat(10);
        let segments = split_plain_text(&body, 4);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "xxxx");
        assert_eq!(segments[1].text, "xxxx");
        assert_eq!(segments[2].text, "xx");
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        let discovered = document();
        let error = PlainTextParser
            .parse(&SourceDocument {
                discovered: &discovered,
                bytes: b"ok\xffno",
            })
            .unwrap_err();
        assert_eq!(error, ParseError::InvalidUtf8);
    }
}
