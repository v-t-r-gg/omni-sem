//! Storage-independent domain types.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! identifier {
    ($name:ident) => {
        #[doc = concat!("Opaque ", stringify!($name), " value.")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a random identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

identifier!(RootId);
identifier!(SourceFileId);
identifier!(RevisionId);
identifier!(SegmentId);

/// Validated content digest including its algorithm prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub String);

/// An explicitly approved local filesystem root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub id: RootId,
    pub canonical_path: PathBuf,
    pub display_name: String,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub follow_symlinks: bool,
    pub enabled: bool,
}

/// A supported document found during discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDocument {
    pub root_id: RootId,
    pub canonical_path: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub file_type: SupportedFileType,
}

/// File formats understood by the current build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportedFileType {
    Markdown,
}

/// Current state of a discovered source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Active,
    Deleted,
    Excluded,
    Unsupported,
    Error,
}

/// Filesystem identity and current-revision pointer for a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: SourceFileId,
    pub root_id: RootId,
    pub relative_path: PathBuf,
    pub canonical_path_hash: ContentHash,
    pub size_bytes: u64,
    pub current_revision_id: Option<RevisionId>,
    pub state: SourceState,
}

/// Processing state of an immutable revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionStatus {
    Prepared,
    Indexed,
    Failed,
}

/// An immutable observed content version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub id: RevisionId,
    pub source_file_id: SourceFileId,
    pub content_hash: ContentHash,
    pub parser_id: String,
    pub parser_version: String,
    pub status: RevisionStatus,
    pub error_code: Option<String>,
}

/// Structure-aware evidence kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentType {
    DocumentTitle,
    Heading,
    Paragraph,
    List,
    Blockquote,
    CodeFence,
    Table,
    Frontmatter,
}

/// An addressable unit of source evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub id: SegmentId,
    pub revision_id: RevisionId,
    pub segment_type: SegmentType,
    pub anchor: String,
    pub ordinal: u32,
    pub text: String,
    pub text_hash: ContentHash,
    pub token_count: Option<u32>,
    pub metadata: serde_json::Value,
}

/// Validated maximum number of returned results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RetrievalLimit(u16);

impl RetrievalLimit {
    /// Creates a non-zero result limit.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidRetrievalLimit`] when `value` is zero.
    pub fn new(value: u16) -> Result<Self, DomainError> {
        if value == 0 {
            return Err(DomainError::InvalidRetrievalLimit);
        }
        Ok(Self(value))
    }

    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Validated context token budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenBudget(u32);

impl TokenBudget {
    /// Creates a non-zero token budget.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidTokenBudget`] when `value` is zero.
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if value == 0 {
            return Err(DomainError::InvalidTokenBudget);
        }
        Ok(Self(value))
    }

    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Retrieval modes reserved by the stable domain contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Lexical,
    Semantic,
    Hybrid,
    Auto,
}

/// Normalized retrieval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub query: String,
    pub root_ids: Vec<RootId>,
    pub mode: RetrievalMode,
    pub limit: RetrievalLimit,
    pub token_budget: TokenBudget,
}

/// Source-grounded retrieval result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalHit {
    pub segment_id: SegmentId,
    pub revision_id: RevisionId,
    pub relative_path: PathBuf,
    pub anchor: String,
    pub text: String,
    pub score: f32,
}

/// Normalized response shared by CLI and future protocol adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalResponse {
    pub query: String,
    pub mode: RetrievalMode,
    pub results: Vec<RetrievalHit>,
    pub token_estimate: u32,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

/// Domain validation failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("retrieval limit must be greater than zero")]
    InvalidRetrievalLimit,
    #[error("token budget must be greater than zero")]
    InvalidTokenBudget,
}

#[cfg(test)]
mod tests {
    use super::{RetrievalLimit, TokenBudget};

    #[test]
    fn budgets_reject_zero() {
        assert!(RetrievalLimit::new(0).is_err());
        assert!(TokenBudget::new(0).is_err());
    }

    #[test]
    fn budgets_preserve_valid_values() {
        assert_eq!(RetrievalLimit::new(8).unwrap().get(), 8);
        assert_eq!(TokenBudget::new(4_000).unwrap().get(), 4_000);
    }
}
