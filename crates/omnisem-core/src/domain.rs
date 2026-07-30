//! Storage-independent domain types.

use std::path::PathBuf;
use std::str::FromStr;
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

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the inner UUID.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
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

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|_| DomainError::InvalidIdentifier)
            }
        }
    };
}

identifier!(RootId);
identifier!(SourceFileId);
identifier!(RevisionId);
identifier!(SegmentId);
identifier!(ScanRunId);

/// Coordinated Universal Time as milliseconds since the Unix epoch.
///
/// Domain and configuration boundaries use this representation so timestamps stay
/// serializable without binding the domain to a clock or time crate. Filesystem
/// metadata is converted at discovery and indexing boundaries.
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

    /// Returns the current wall-clock time as a domain timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidTimestamp`] when the system clock is before
    /// the Unix epoch or overflows `i64` milliseconds.
    pub fn now() -> Result<Self, DomainError> {
        Self::try_from_system_time(SystemTime::now())
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

impl ContentHash {
    /// Returns the serialized digest, including the algorithm prefix.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

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

impl SensitivityScope {
    /// Returns the stable configuration/storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeverReturnToMcp => "never_return_to_mcp",
            Self::RequireExplicitQuery => "require_explicit_query",
        }
    }
}

impl FromStr for SensitivityScope {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "never_return_to_mcp" => Ok(Self::NeverReturnToMcp),
            "require_explicit_query" => Ok(Self::RequireExplicitQuery),
            _ => Err(DomainError::InvalidSensitivityScope),
        }
    }
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
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub config_fingerprint: String,
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

impl SupportedFileType {
    /// Returns the stable storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::PlainText => "plain_text",
        }
    }
}

impl FromStr for SupportedFileType {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "markdown" => Ok(Self::Markdown),
            "plain_text" => Ok(Self::PlainText),
            _ => Err(DomainError::InvalidFileType),
        }
    }
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

impl SourceState {
    /// Returns the stable storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deleted => "deleted",
            Self::Excluded => "excluded",
            Self::Unsupported => "unsupported",
            Self::Error => "error",
        }
    }
}

impl FromStr for SourceState {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "deleted" => Ok(Self::Deleted),
            "excluded" => Ok(Self::Excluded),
            "unsupported" => Ok(Self::Unsupported),
            "error" => Ok(Self::Error),
            _ => Err(DomainError::InvalidSourceState),
        }
    }
}

/// Filesystem identity and current-revision pointer for a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: SourceFileId,
    pub root_id: RootId,
    pub relative_path: PathBuf,
    pub canonical_path_hash: ContentHash,
    pub file_type: SupportedFileType,
    pub size_bytes: u64,
    pub modified_at: Option<Timestamp>,
    pub current_revision_id: Option<RevisionId>,
    pub state: SourceState,
    pub first_seen_at: Timestamp,
    pub last_seen_at: Timestamp,
}

/// Processing state of an immutable revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionStatus {
    Prepared,
    Indexed,
    Failed,
}

impl RevisionStatus {
    /// Returns the stable storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Indexed => "indexed",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for RevisionStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "indexed" => Ok(Self::Indexed),
            "failed" => Ok(Self::Failed),
            _ => Err(DomainError::InvalidRevisionStatus),
        }
    }
}

/// An immutable observed content version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub id: RevisionId,
    pub source_file_id: SourceFileId,
    pub content_hash: ContentHash,
    pub parser_id: String,
    pub parser_version: String,
    pub extracted_text_hash: Option<ContentHash>,
    pub observed_at: Timestamp,
    pub indexed_at: Option<Timestamp>,
    pub status: RevisionStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
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

impl SegmentType {
    /// Returns the stable storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocumentTitle => "document_title",
            Self::Heading => "heading",
            Self::Paragraph => "paragraph",
            Self::List => "list",
            Self::Blockquote => "blockquote",
            Self::CodeFence => "code_fence",
            Self::Table => "table",
            Self::Frontmatter => "frontmatter",
        }
    }
}

impl FromStr for SegmentType {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "document_title" => Ok(Self::DocumentTitle),
            "heading" => Ok(Self::Heading),
            "paragraph" => Ok(Self::Paragraph),
            "list" => Ok(Self::List),
            "blockquote" => Ok(Self::Blockquote),
            "code_fence" => Ok(Self::CodeFence),
            "table" => Ok(Self::Table),
            "frontmatter" => Ok(Self::Frontmatter),
            _ => Err(DomainError::InvalidSegmentType),
        }
    }
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
    pub sensitivity_scope: Option<SensitivityScope>,
}

/// Outcome of one completed root scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanRun {
    pub id: ScanRunId,
    pub root_id: RootId,
    pub started_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub status: ScanStatus,
    pub additions: u32,
    pub modifications: u32,
    pub unchanged: u32,
    pub deletions: u32,
    pub skipped: u32,
    pub failures: u32,
    pub segments_indexed: u32,
    pub error_code: Option<String>,
}

/// Terminal state of a root scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Completed,
    Failed,
}

impl ScanStatus {
    /// Returns the stable storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for ScanStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(DomainError::InvalidScanStatus),
        }
    }
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

impl RetrievalMode {
    /// Returns the stable storage/output token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Hybrid => "hybrid",
            Self::Auto => "auto",
        }
    }
}

impl FromStr for RetrievalMode {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "lexical" => Ok(Self::Lexical),
            "semantic" => Ok(Self::Semantic),
            "hybrid" => Ok(Self::Hybrid),
            "auto" => Ok(Self::Auto),
            _ => Err(DomainError::InvalidRetrievalMode),
        }
    }
}

/// Named context-budget profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetPreset {
    pub name: String,
    pub token_budget: u32,
    pub max_results: u16,
}

/// Normalized retrieval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub query: String,
    pub root_ids: Vec<RootId>,
    pub file_types: Vec<SupportedFileType>,
    pub mode: RetrievalMode,
    pub limit: RetrievalLimit,
    pub token_budget: TokenBudget,
    pub include_sensitive: bool,
    pub budget_preset: Option<String>,
}

/// Provenance of a retrieval hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceOrigin {
    LocalIndex,
    Snapshot {
        snapshot_id: String,
        snapshot_root_id: String,
    },
}

/// Ranking and channel signals retained for debugging and evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalSignals {
    pub channel: String,
    pub raw_bm25: Option<f32>,
    pub public_score: f32,
    /// Final federation score when multiple indexes contribute (RRF).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub federation_score: Option<f32>,
}

/// Deterministic match explanation without model inference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchExplanation {
    pub matched_terms: Vec<String>,
    pub matched_excerpt: Option<String>,
    pub explanation_kind: ExplanationKind,
}

/// How a result was justified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationKind {
    LexicalTermOverlap,
    SemanticNeighbor,
    StructuralExpansion,
}

impl ExplanationKind {
    /// Returns the stable output token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LexicalTermOverlap => "lexical_term_overlap",
            Self::SemanticNeighbor => "semantic_neighbor",
            Self::StructuralExpansion => "structural_expansion",
        }
    }
}

/// Relationship between indexed state and the live filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessStatus {
    Current,
    PendingReindex,
    Unknown,
}

impl FreshnessStatus {
    /// Returns the stable output token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::PendingReindex => "pending_reindex",
            Self::Unknown => "unknown",
        }
    }
}

/// Source-grounded retrieval result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalHit {
    pub segment_id: SegmentId,
    pub revision_id: RevisionId,
    pub source_file_id: SourceFileId,
    pub root_id: RootId,
    pub relative_path: PathBuf,
    pub file_type: SupportedFileType,
    pub anchor: String,
    pub text: String,
    pub text_hash: ContentHash,
    pub score: f32,
    pub signals: RetrievalSignals,
    pub explanation: MatchExplanation,
    pub freshness: FreshnessStatus,
    pub sensitivity_scope: Option<SensitivityScope>,
    pub token_estimate: u32,
    pub truncated: bool,
    pub origin: EvidenceOrigin,
}

/// Normalized response shared by CLI and future protocol adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalResponse {
    pub query: String,
    pub mode: RetrievalMode,
    pub results: Vec<RetrievalHit>,
    pub token_estimate: u32,
    pub truncated: bool,
    pub applied_limit: u16,
    pub applied_token_budget: u32,
    pub budget_preset: Option<String>,
    pub duplicates_suppressed: u32,
    pub warnings: Vec<String>,
    pub elapsed_ms: u64,
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
    #[error("invalid identifier")]
    InvalidIdentifier,
    #[error("invalid sensitivity scope")]
    InvalidSensitivityScope,
    #[error("invalid file type")]
    InvalidFileType,
    #[error("invalid source state")]
    InvalidSourceState,
    #[error("invalid revision status")]
    InvalidRevisionStatus,
    #[error("invalid segment type")]
    InvalidSegmentType,
    #[error("invalid scan status")]
    InvalidScanStatus,
    #[error("invalid duration syntax")]
    InvalidDuration,
    #[error("invalid retrieval mode")]
    InvalidRetrievalMode,
    #[error("QUERY_EMPTY")]
    QueryEmpty,
    #[error("QUERY_INVALID: {0}")]
    QueryInvalid(String),
    #[error("RETRIEVAL_MODE_UNAVAILABLE: {0}")]
    RetrievalModeUnavailable(String),
    #[error("BUDGET_PRESET_NOT_FOUND: {0}")]
    BudgetPresetNotFound(String),
}

/// Parses compact duration tokens such as `7d`, `12h`, `30m`, or `90s`.
///
/// # Errors
///
/// Returns [`DomainError::InvalidDuration`] for empty input, unknown units, or
/// values that overflow.
pub fn parse_duration_to_millis(input: &str) -> Result<i64, DomainError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(DomainError::InvalidDuration);
    }
    let (number, unit) = input.split_at(input.len().saturating_sub(1));
    let amount: i64 = number.parse().map_err(|_| DomainError::InvalidDuration)?;
    if amount < 0 {
        return Err(DomainError::InvalidDuration);
    }
    let multiplier = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err(DomainError::InvalidDuration),
    };
    amount
        .checked_mul(multiplier)
        .ok_or(DomainError::InvalidDuration)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let system = UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_123);
        let stamp = Timestamp::try_from_system_time(system).unwrap();
        assert_eq!(stamp.as_millis(), 1_700_000_000_123);
        assert_eq!(Timestamp::from_millis(42).as_millis(), 42);
    }

    #[test]
    fn duration_parser_accepts_compact_units() {
        assert_eq!(parse_duration_to_millis("7d").unwrap(), 7 * 86_400_000);
        assert_eq!(parse_duration_to_millis("12h").unwrap(), 12 * 3_600_000);
        assert_eq!(parse_duration_to_millis("30m").unwrap(), 30 * 60_000);
        assert_eq!(parse_duration_to_millis("90s").unwrap(), 90_000);
        assert!(parse_duration_to_millis("7w").is_err());
        assert!(parse_duration_to_millis("").is_err());
    }

    #[test]
    fn root_id_parses_uuid_text() {
        let id = RootId::new();
        let parsed: RootId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }
}
