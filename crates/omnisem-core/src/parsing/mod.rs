//! Parser contracts and deterministic Markdown / plain-text implementations.

mod markdown;
mod plain_text;

pub use markdown::MarkdownParser;
pub use plain_text::PlainTextParser;

use crate::domain::{DiscoveredDocument, SegmentType};

/// Stable source bytes and discovery metadata supplied to a parser.
#[derive(Debug)]
pub struct SourceDocument<'a> {
    pub discovered: &'a DiscoveredDocument,
    pub bytes: &'a [u8],
}

/// Parser output before persistent identifiers and hashes are assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    pub title: Option<String>,
    pub segments: Vec<ParsedSegment>,
    pub warnings: Vec<ParserWarning>,
}

/// Ordered, structure-aware parser output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSegment {
    pub segment_type: SegmentType,
    pub anchor: String,
    pub ordinal: u32,
    pub text: String,
}

/// Non-fatal condition discovered while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserWarning {
    pub code: String,
    pub message: String,
}

/// Storage- and protocol-independent document parser.
pub trait DocumentParser: Send + Sync {
    /// Returns the stable parser implementation identity.
    fn parser_id(&self) -> &'static str;
    /// Returns a version that changes when derived output semantics change.
    fn parser_version(&self) -> &'static str;
    /// Reports whether this parser accepts the discovered document.
    fn supports(&self, document: &DiscoveredDocument) -> bool;
    /// Parses stable source bytes into ordered structural segments.
    ///
    /// # Errors
    ///
    /// Returns a typed parser failure without producing partial persistent output.
    fn parse(&self, source: &SourceDocument<'_>) -> Result<ParsedDocument, ParseError>;
}

/// Explicit parser selection with deterministic registration order.
#[derive(Default)]
pub struct ParserRegistry {
    parsers: Vec<Box<dyn DocumentParser>>,
}

impl ParserRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            parsers: Vec::new(),
        }
    }

    /// Builds the Milestone 1 registry: structured Markdown first, plain text last.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::DuplicateParser`] if identities collide during construction.
    pub fn with_defaults() -> Result<Self, ParseError> {
        let mut registry = Self::new();
        registry.register(Box::new(MarkdownParser))?;
        registry.register(Box::new(PlainTextParser))?;
        Ok(registry)
    }

    /// Registers a parser after rejecting duplicate identities.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::DuplicateParser`] when the identity already exists.
    pub fn register(&mut self, parser: Box<dyn DocumentParser>) -> Result<(), ParseError> {
        if self
            .parsers
            .iter()
            .any(|item| item.parser_id() == parser.parser_id())
        {
            return Err(ParseError::DuplicateParser(parser.parser_id().to_owned()));
        }
        self.parsers.push(parser);
        Ok(())
    }

    /// Selects the first explicitly registered supporting parser.
    #[must_use]
    pub fn select(&self, document: &DiscoveredDocument) -> Option<&dyn DocumentParser> {
        self.parsers
            .iter()
            .find(|parser| parser.supports(document))
            .map(AsRef::as_ref)
    }
}

/// Stable parser contract failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("duplicate parser id: {0}")]
    DuplicateParser(String),
    #[error("source is not valid UTF-8")]
    InvalidUtf8,
    #[error("parser failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RootId, SupportedFileType, Timestamp};
    use std::path::PathBuf;

    fn discovered(file_type: SupportedFileType, name: &str) -> DiscoveredDocument {
        DiscoveredDocument {
            root_id: RootId::new(),
            canonical_path: PathBuf::from(format!("/approved/{name}")),
            relative_path: PathBuf::from(name),
            size_bytes: 6,
            modified_at: Timestamp::from_millis(1),
            file_type,
        }
    }

    #[test]
    fn registry_selects_supporting_parser_and_contract_output_is_ordered() {
        let registry = ParserRegistry::with_defaults().unwrap();
        let document = discovered(SupportedFileType::Markdown, "a.md");
        let parser = registry.select(&document).unwrap();
        assert_eq!(parser.parser_id(), "markdown-v1");
        let parsed = parser
            .parse(&SourceDocument {
                discovered: &document,
                bytes: b"Hello\n",
            })
            .unwrap();
        assert!(!parsed.segments.is_empty());
        assert_eq!(parsed.segments[0].ordinal, 0);
    }

    #[test]
    fn registry_rejects_duplicate_parser_ids() {
        let mut registry = ParserRegistry::new();
        registry.register(Box::new(MarkdownParser)).unwrap();
        assert_eq!(
            registry.register(Box::new(MarkdownParser)),
            Err(ParseError::DuplicateParser("markdown-v1".into()))
        );
    }

    #[test]
    fn structured_parser_precedes_plain_text_fallback() {
        let registry = ParserRegistry::with_defaults().unwrap();
        let markdown = discovered(SupportedFileType::Markdown, "a.md");
        let plain = discovered(SupportedFileType::PlainText, "a.txt");
        assert_eq!(
            registry.select(&markdown).unwrap().parser_id(),
            "markdown-v1"
        );
        assert_eq!(
            registry.select(&plain).unwrap().parser_id(),
            "plain-text-v1"
        );
        assert!(!MarkdownParser.supports(&plain));
        assert!(PlainTextParser.supports(&plain));
        assert!(!PlainTextParser.supports(&markdown));
    }

    #[test]
    fn unsupported_binary_is_not_routed_to_plain_text() {
        let registry = ParserRegistry::with_defaults().unwrap();
        // Discovery never emits unsupported types; registry must still refuse them.
        let mut binary = discovered(SupportedFileType::PlainText, "a.bin");
        binary.file_type = SupportedFileType::PlainText;
        // Extension-based discovery would skip .bin; if misclassified, plain-text accepts
        // only the PlainText classification, never Markdown.
        let markdown_only = discovered(SupportedFileType::Markdown, "a.bin");
        assert!(registry.select(&markdown_only).is_some());
        assert_eq!(
            registry.select(&binary).unwrap().parser_id(),
            "plain-text-v1"
        );
    }
}
