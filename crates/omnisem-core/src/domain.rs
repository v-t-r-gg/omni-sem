//! Storage-independent domain types.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Coordinated Universal Time as milliseconds since the Unix epoch.
///
/// Domain and configuration boundaries use this representation so timestamps stay
/// serializable without binding the domain to a clock or time crate. Filesystem
/// metadata is converted at the discovery boundary. `SQLite` persistence of these
/// values is deferred to the schema-alignment slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates a timestamp from milliseconds since the Unix epoch.
    #[must_use]
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Returns milliseconds since the Unix epoch.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// Converts a filesystem `SystemTime` into a domain timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidTimestamp`] when the time is before the Unix
    /// epoch or cannot be represented as an `i64` millisecond offset.
    pub fn try_from_system_time(time: SystemTime) -> Result<Self, DomainError> {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| DomainError::InvalidTimestamp)?;
        let millis =
            i64::try_from(duration.as_millis()).map_err(|_| DomainError::InvalidTimestamp)?;
        Ok(Self(millis))
    }
}

/// Validated content digest including its algorithm prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub String);

/// How a sensitivity tag constrains later retrieval visibility.
///
/// Sensitivity never controls whether content is indexed. Exclusion patterns alone
/// decide indexing. These scopes gate whether already-indexed content may be
/// returned through MCP or ordinary retrieval once those surfaces exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityScope {
    /// Indexed content must never appear in MCP responses.
    NeverReturnToMcp,
    /// Indexed content may be returned only when a request explicitly opts in.
    RequireExplicitQuery,
}

/// Root-level pattern that marks matching paths as sensitive for retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensitivityTag {
    pub pattern: String,
    pub scope: SensitivityScope,
}

/// An explicitly approved local filesystem root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub id: RootId,
    pub canonical_path: PathBuf,
    pub display_name: String,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub sensitivity_tags: Vec<SensitivityTag>,
    pub follow_symlinks: bool,
    pub enabled: bool,
}

/// A supported document found during discovery.
///
/// This is discovery metadata only. Persistence row mappings belong in storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDocument {
    pub root_id: RootId,
    pub canonical_path: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub modified_at: Timestamp,
    pub file_type: SupportedFileType,
}

/// File formats understood by the current build.
///
/// `Markdown` is structure-aware. `PlainText` is the deterministic fallback for
/// valid textual files that no structured parser claims. It is not code-aware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportedFileType {
    Markdown,
    PlainText,
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
    pub modified_at: Option<Timestamp>,
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
    #[error("timestamp is outside the representable Unix-millisecond range")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use super::{
        DomainError, RetrievalLimit, SensitivityScope, SensitivityTag, SupportedFileType,
        Timestamp, TokenBudget,
    };
    use std::time::{Duration, UNIX_EPOCH};

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

    #[test]
    fn timestamp_round_trips_system_time_millis() {
        let system = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        let stamp = Timestamp::try_from_system_time(system).unwrap();
        assert_eq!(stamp.as_millis(), 1_700_000_000_123);
        assert_eq!(Timestamp::from_millis(42).as_millis(), 42);
    }

    #[test]
    fn timestamp_rejects_pre_epoch() {
        let pre_epoch = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(
            Timestamp::try_from_system_time(pre_epoch),
            Err(DomainError::InvalidTimestamp)
        );
    }

    #[test]
    fn sensitivity_is_distinct_from_file_classification() {
        let tag = SensitivityTag {
            pattern: "**/private/**".into(),
            scope: SensitivityScope::NeverReturnToMcp,
        };
        assert_eq!(tag.scope, SensitivityScope::NeverReturnToMcp);
        assert_ne!(
            format!("{:?}", SupportedFileType::Markdown),
            format!("{:?}", SupportedFileType::PlainText)
        );
    }
}
