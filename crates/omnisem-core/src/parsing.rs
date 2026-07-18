//! Parser contracts. No parser implementation is selected in Milestone 0.

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
    use std::path::PathBuf;

    use super::*;
    use crate::domain::{RootId, SupportedFileType};

    struct ContractMarkdownParser;
    impl DocumentParser for ContractMarkdownParser {
        fn parser_id(&self) -> &'static str {
            "contract-markdown"
        }
        fn parser_version(&self) -> &'static str {
            "0"
        }
        fn supports(&self, document: &DiscoveredDocument) -> bool {
            document.file_type == SupportedFileType::Markdown
        }
        fn parse(&self, source: &SourceDocument<'_>) -> Result<ParsedDocument, ParseError> {
            let text = std::str::from_utf8(source.bytes).map_err(|_| ParseError::InvalidUtf8)?;
            Ok(ParsedDocument {
                title: None,
                segments: vec![ParsedSegment {
                    segment_type: SegmentType::Paragraph,
                    anchor: "paragraph:1".into(),
                    ordinal: 0,
                    text: text.trim().into(),
                }],
                warnings: vec![],
            })
        }
    }

    fn discovered() -> DiscoveredDocument {
        DiscoveredDocument {
            root_id: RootId::new(),
            canonical_path: PathBuf::from("/approved/a.md"),
            relative_path: PathBuf::from("a.md"),
            size_bytes: 6,
            file_type: SupportedFileType::Markdown,
        }
    }

    #[test]
    fn registry_selects_supporting_parser_and_contract_output_is_ordered() {
        let mut registry = ParserRegistry::new();
        registry.register(Box::new(ContractMarkdownParser)).unwrap();
        let document = discovered();
        let parser = registry.select(&document).unwrap();
        let parsed = parser
            .parse(&SourceDocument {
                discovered: &document,
                bytes: b"Hello\n",
            })
            .unwrap();
        assert_eq!(parsed.segments[0].anchor, "paragraph:1");
        assert_eq!(parsed.segments[0].ordinal, 0);
        assert_eq!(parsed.segments[0].text, "Hello");
    }

    #[test]
    fn registry_rejects_duplicate_parser_ids() {
        let mut registry = ParserRegistry::new();
        registry.register(Box::new(ContractMarkdownParser)).unwrap();
        assert_eq!(
            registry.register(Box::new(ContractMarkdownParser)),
            Err(ParseError::DuplicateParser("contract-markdown".into()))
        );
    }
}
