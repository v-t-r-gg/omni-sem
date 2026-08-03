//! Transport-neutral read-only MCP application boundary.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::domain::{
    EvidenceOrigin, FreshnessStatus, MatchExplanation, RetrievalAudience, RetrievalMode,
    RetrievalQuery, RetrievalSignals, RootId, SegmentId, SupportedFileType,
};
use crate::freshness::inspect_freshness;
use crate::retrieval::{path_allowed_for_audience, retrieve_for_audience};
use crate::snapshot::{eligible_snapshot_sources, list_snapshots, open_snapshot_readonly};
use crate::storage::{
    EmbeddingCompatibility, EmbeddingStatus, embedding_compatibility, open_database_readonly,
    status_snapshot,
};
use crate::tokens::{
    HARD_BYTE_CAP, HeuristicTokenEstimator, RESPONSE_OVERHEAD_TOKENS, RESULT_OVERHEAD_TOKENS,
    TokenEstimator, truncate_utf8,
};

/// Maximum resource URIs accepted by one hydration request.
pub const MCP_MAX_URIS: usize = 16;
/// Maximum neighbor radius on either side of an addressed segment.
pub const MCP_MAX_NEIGHBORS: u8 = 3;
/// Maximum result count accepted by MCP search.
pub const MCP_MAX_RESULTS: u16 = 32;
/// Maximum combined context budget accepted by MCP tools.
pub const MCP_MAX_TOKEN_BUDGET: u32 = 16_000;
/// Maximum root filters accepted by MCP search.
pub const MCP_MAX_ROOT_FILTERS: usize = 16;
/// Maximum query bytes accepted at the MCP boundary.
pub const MCP_MAX_QUERY_BYTES: usize = 4_096;
/// Marker attached to every returned source item.
pub const UNTRUSTED_SOURCE_EVIDENCE: &str = "untrusted_source_evidence";

/// Strict Omni-Sem resource identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpResourceUri {
    LocalSegment(SegmentId),
    SnapshotSegment {
        snapshot_id: String,
        segment_id: SegmentId,
    },
    Status,
}

impl McpResourceUri {
    /// Parses the fixed URI grammar without URL reinterpretation.
    ///
    /// # Errors
    /// Returns a stable safe resource error for malformed or unsupported input.
    pub fn parse(value: &str) -> Result<Self, McpServiceError> {
        if value.is_empty()
            || value.len() > 200
            || value.contains(['?', '#', '%', '\\'])
            || value.contains("..")
            || !value.starts_with("omnisem://")
        {
            return Err(McpServiceError::resource_invalid());
        }
        let remainder = &value[10..];
        if remainder == "status" {
            return Ok(Self::Status);
        }
        let parts = remainder.split('/').collect::<Vec<_>>();
        match parts.as_slice() {
            ["segment", segment] => SegmentId::from_str(segment)
                .map(Self::LocalSegment)
                .map_err(|_| McpServiceError::resource_invalid()),
            ["snapshot", snapshot, "segment", segment] if Uuid::parse_str(snapshot).is_ok() => {
                SegmentId::from_str(segment)
                    .map(|segment_id| Self::SnapshotSegment {
                        snapshot_id: (*snapshot).to_owned(),
                        segment_id,
                    })
                    .map_err(|_| McpServiceError::resource_invalid())
            }
            _ => Err(McpServiceError::resource_invalid()),
        }
    }
}

impl std::fmt::Display for McpResourceUri {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalSegment(id) => write!(formatter, "omnisem://segment/{id}"),
            Self::SnapshotSegment {
                snapshot_id,
                segment_id,
            } => {
                write!(
                    formatter,
                    "omnisem://snapshot/{snapshot_id}/segment/{segment_id}"
                )
            }
            Self::Status => formatter.write_str("omnisem://status"),
        }
    }
}

/// Safe application error translated by the protocol adapter.
#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct McpServiceError {
    pub code: &'static str,
    pub message: &'static str,
}

impl McpServiceError {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    const fn resource_invalid() -> Self {
        Self::new("RESOURCE_INVALID", "invalid Omni-Sem resource URI")
    }

    const fn unavailable() -> Self {
        Self::new("MCP_PROTOCOL_ERROR", "indexed context is unavailable")
    }
}

/// One source-grounded MCP context item.
#[derive(Debug, Clone, Serialize)]
pub struct McpContextItem {
    pub resource_uri: String,
    pub origin: String,
    pub logical_root_id: String,
    pub relative_path: String,
    pub anchor: String,
    pub segment_id: String,
    pub text: String,
    pub freshness: FreshnessStatus,
    pub explanation: MatchExplanation,
    pub score: f32,
    pub score_kind: String,
    pub signals: RetrievalSignals,
    pub content_trust: &'static str,
    pub token_estimate: u32,
    pub truncated: bool,
}

/// Search result without persisted or echoed query material.
#[derive(Debug, Clone, Serialize)]
pub struct McpSearchResponse {
    pub requested_mode: RetrievalMode,
    pub effective_mode: RetrievalMode,
    pub score_kind: String,
    pub items: Vec<McpContextItem>,
    pub token_estimate: u32,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

/// Hydrated resource result with one combined budget.
#[derive(Debug, Clone, Serialize)]
pub struct McpGetContextResponse {
    pub items: Vec<McpContextItem>,
    pub token_estimate: u32,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

/// Safe persisted-only status response.
#[derive(Debug, Clone, Serialize)]
pub struct McpIndexStatus {
    pub schema_version: i64,
    pub roots: McpRootStatus,
    pub active_source_files: i64,
    pub active_revisions: i64,
    pub active_segments: i64,
    pub fts_rows: i64,
    pub failed_sources: i64,
    pub last_successful_scan_ms: Option<i64>,
    pub last_failed_scan_ms: Option<i64>,
    pub snapshots: McpSnapshotStatus,
    pub embedding: EmbeddingStatus,
    pub embedding_compatibility: EmbeddingCompatibility,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpRootStatus {
    pub configured: i64,
    pub enabled: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpSnapshotStatus {
    pub registered: usize,
    pub queryable: usize,
    pub unhealthy: usize,
}

/// Request-scoped, transport-neutral read-only application service.
#[derive(Debug, Clone)]
pub struct McpContextService {
    config: AppConfig,
    database_path: PathBuf,
}

impl McpContextService {
    #[must_use]
    pub fn new(config: AppConfig, database_path: PathBuf) -> Self {
        Self {
            config,
            database_path,
        }
    }

    /// Searches approved indexed evidence using MCP audience rules.
    ///
    /// # Errors
    /// Returns bounded validation, availability, or retrieval errors.
    pub fn search_context(
        &self,
        request: &RetrievalQuery,
    ) -> Result<McpSearchResponse, McpServiceError> {
        validate_search_request(request)?;
        let connection = self.open_readonly()?;
        let response =
            retrieve_for_audience(&connection, &self.config, request, RetrievalAudience::Mcp)
                .map_err(|error| {
                    let code = match &error {
                        crate::error::RetrievalError::Semantic { code, .. } => *code,
                        crate::error::RetrievalError::Domain(_) => "MCP_INVALID_PARAMS",
                        _ => "MCP_PROTOCOL_ERROR",
                    };
                    McpServiceError::new(code, "context search failed safely")
                })?;
        let score_kind = response.score_kind.clone();
        let items = response
            .results
            .into_iter()
            .map(|hit| context_item(hit, &score_kind))
            .collect();
        Ok(McpSearchResponse {
            requested_mode: response.requested_mode,
            effective_mode: response.mode,
            score_kind,
            items,
            token_estimate: response.token_estimate,
            truncated: response.truncated,
            warnings: bounded_warnings(response.warnings),
        })
    }

    /// Hydrates strict resource URIs and bounded same-revision neighbors.
    ///
    /// # Errors
    /// Returns safe resource or availability failures.
    pub fn get_context(
        &self,
        uris: &[String],
        neighbor_segments: u8,
        token_budget: u32,
    ) -> Result<McpGetContextResponse, McpServiceError> {
        if uris.is_empty()
            || uris.len() > MCP_MAX_URIS
            || neighbor_segments > MCP_MAX_NEIGHBORS
            || token_budget == 0
            || token_budget > MCP_MAX_TOKEN_BUDGET
        {
            return Err(McpServiceError::new(
                "MCP_INVALID_PARAMS",
                "resource request exceeds a documented bound",
            ));
        }
        let parsed = uris
            .iter()
            .map(|uri| McpResourceUri::parse(uri))
            .collect::<Result<Vec<_>, _>>()?;
        if parsed
            .iter()
            .any(|uri| matches!(uri, McpResourceUri::Status))
        {
            return Err(McpServiceError::new(
                "RESOURCE_INVALID",
                "status must be read through resources/read or index_status",
            ));
        }
        let connection = self.open_readonly()?;
        let mut candidates = Vec::new();
        for uri in parsed {
            match uri {
                McpResourceUri::LocalSegment(segment) => candidates.extend(self.hydrate_local(
                    &connection,
                    segment,
                    neighbor_segments,
                )?),
                McpResourceUri::SnapshotSegment {
                    snapshot_id,
                    segment_id,
                } => candidates.extend(self.hydrate_snapshot(
                    &connection,
                    &snapshot_id,
                    segment_id,
                    neighbor_segments,
                )?),
                McpResourceUri::Status => unreachable!(),
            }
        }
        candidates.sort_by(|a, b| {
            a.relative_path
                .cmp(&b.relative_path)
                .then_with(|| a.anchor.cmp(&b.anchor))
                .then_with(|| a.segment_id.cmp(&b.segment_id))
        });
        candidates.dedup_by(|a, b| a.resource_uri == b.resource_uri);
        let (items, estimate, truncated) = pack_context(candidates, token_budget);
        Ok(McpGetContextResponse {
            items,
            token_estimate: estimate,
            truncated,
            warnings: Vec::new(),
        })
    }

    /// Returns persisted operational status without provider or write access.
    ///
    /// # Errors
    /// Returns a bounded availability error.
    pub fn index_status(&self) -> Result<McpIndexStatus, McpServiceError> {
        let connection = self.open_readonly()?;
        let status = status_snapshot(&connection, &self.database_path)
            .map_err(|_| McpServiceError::unavailable())?;
        let snapshots = list_snapshots(&connection).map_err(|_| McpServiceError::unavailable())?;
        Ok(McpIndexStatus {
            schema_version: status.schema_version,
            roots: McpRootStatus {
                configured: status.root_count,
                enabled: status.enabled_root_count,
            },
            active_source_files: status.active_source_files,
            active_revisions: status.active_revisions,
            active_segments: status.active_segments,
            fts_rows: status.fts_rows,
            failed_sources: status.failed_sources,
            last_successful_scan_ms: status.last_successful_scan_ms,
            last_failed_scan_ms: status.last_failed_scan_ms,
            snapshots: McpSnapshotStatus {
                registered: snapshots.len(),
                queryable: snapshots.iter().filter(|item| item.queryable).count(),
                unhealthy: snapshots
                    .iter()
                    .filter(|item| !item.payload_healthy)
                    .count(),
            },
            embedding_compatibility: embedding_compatibility(
                &self.config.embeddings,
                &status.embedding,
            ),
            embedding: status.embedding,
            read_only: true,
        })
    }

    /// Reads a single resource for the MCP resources/read operation.
    ///
    /// # Errors
    /// Returns a safe resource failure.
    pub fn read_resource(&self, uri: &str) -> Result<serde_json::Value, McpServiceError> {
        match McpResourceUri::parse(uri)? {
            McpResourceUri::Status => serde_json::to_value(self.index_status()?)
                .map_err(|_| McpServiceError::unavailable()),
            other => {
                let response = self.get_context(&[other.to_string()], 0, MCP_MAX_TOKEN_BUDGET)?;
                response
                    .items
                    .into_iter()
                    .next()
                    .map(|item| serde_json::to_value(item).unwrap_or(serde_json::Value::Null))
                    .ok_or_else(|| {
                        McpServiceError::new("RESOURCE_NOT_FOUND", "resource is not available")
                    })
            }
        }
    }

    fn open_readonly(&self) -> Result<Connection, McpServiceError> {
        open_database_readonly(&self.database_path).map_err(|_| McpServiceError::unavailable())
    }

    fn hydrate_local(
        &self,
        connection: &Connection,
        segment_id: SegmentId,
        neighbors: u8,
    ) -> Result<Vec<McpContextItem>, McpServiceError> {
        let addressed: Option<(String, i64)> = connection
            .query_row(
                "SELECT revision_id,ordinal FROM segments WHERE id=?1",
                [segment_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| McpServiceError::unavailable())?;
        let Some((revision_id, ordinal)) = addressed else {
            return Err(McpServiceError::new(
                "RESOURCE_NOT_FOUND",
                "resource is not available",
            ));
        };
        let radius = i64::from(neighbors);
        let mut statement = connection
            .prepare(
                "SELECT s.id,s.anchor,s.text,s.ordinal,sf.root_id,sf.relative_path,r.canonical_path,sf.modified_at_ms
                 FROM segments s
                 JOIN source_files sf ON sf.current_revision_id=s.revision_id AND sf.state='active'
                 JOIN roots r ON r.id=sf.root_id AND r.enabled=1
                 WHERE s.revision_id=?1 AND s.ordinal BETWEEN ?2 AND ?3
                 ORDER BY s.ordinal,s.id",
            )
            .map_err(|_| McpServiceError::unavailable())?;
        let rows = statement
            .query_map(
                rusqlite::params![revision_id, ordinal - radius, ordinal + radius],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .map_err(|_| McpServiceError::unavailable())?;
        let mut out = Vec::new();
        for row in rows {
            let (id, anchor, text, _, root, relative, root_path, modified) =
                row.map_err(|_| McpServiceError::unavailable())?;
            let root_id = RootId::from_str(&root).map_err(|_| McpServiceError::unavailable())?;
            let relative_path = PathBuf::from(&relative);
            if !path_allowed_for_audience(
                &self.config,
                &root_id,
                &relative_path,
                RetrievalAudience::Mcp,
                false,
            ) {
                continue;
            }
            out.push(hydrated_item(
                McpResourceUri::LocalSegment(
                    SegmentId::from_str(&id).map_err(|_| McpServiceError::unavailable())?,
                ),
                "local",
                &root,
                &relative,
                &anchor,
                text,
                inspect_freshness(
                    Path::new(&root_path),
                    &relative_path,
                    modified.map(crate::domain::Timestamp::from_millis),
                ),
            ));
        }
        if out.is_empty() {
            return Err(McpServiceError::new(
                "RESOURCE_FORBIDDEN",
                "resource is not available",
            ));
        }
        Ok(out)
    }

    fn hydrate_snapshot(
        &self,
        connection: &Connection,
        snapshot_id: &str,
        segment_id: SegmentId,
        neighbors: u8,
    ) -> Result<Vec<McpContextItem>, McpServiceError> {
        let request = unrestricted_lexical_request();
        let sources = eligible_snapshot_sources(connection, &self.config, &request)
            .map_err(|_| McpServiceError::unavailable())?;
        for source in sources
            .into_iter()
            .filter(|source| source.snapshot_id == snapshot_id)
        {
            let snapshot = open_snapshot_readonly(&source.payload_path)
                .map_err(|_| McpServiceError::unavailable())?;
            let addressed: Option<(String, i64)> = snapshot
                .query_row(
                    "SELECT revision_id,ordinal FROM segments WHERE id=?1 AND root_id=?2",
                    [segment_id.to_string(), source.snapshot_root_id.clone()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|_| McpServiceError::unavailable())?;
            let Some((revision, ordinal)) = addressed else {
                continue;
            };
            let radius = i64::from(neighbors);
            let mut statement = snapshot
                .prepare(
                    "SELECT s.id,s.anchor,s.text,s.ordinal,sf.relative_path
                     FROM segments s
                     JOIN source_files sf ON sf.current_revision_id=s.revision_id AND sf.state='active'
                     WHERE s.revision_id=?1 AND sf.root_id=?2 AND s.ordinal BETWEEN ?3 AND ?4
                     ORDER BY s.ordinal,s.id",
                )
                .map_err(|_| McpServiceError::unavailable())?;
            let mut rows = statement
                .query(rusqlite::params![
                    revision,
                    source.snapshot_root_id,
                    ordinal - radius,
                    ordinal + radius
                ])
                .map_err(|_| McpServiceError::unavailable())?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().map_err(|_| McpServiceError::unavailable())? {
                let id: String = row.get(0).map_err(|_| McpServiceError::unavailable())?;
                let relative: String = row.get(4).map_err(|_| McpServiceError::unavailable())?;
                if !path_allowed_for_audience(
                    &self.config,
                    &source.local_root_id,
                    Path::new(&relative),
                    RetrievalAudience::Mcp,
                    false,
                ) {
                    continue;
                }
                out.push(hydrated_item(
                    McpResourceUri::SnapshotSegment {
                        snapshot_id: snapshot_id.to_owned(),
                        segment_id: SegmentId::from_str(&id)
                            .map_err(|_| McpServiceError::unavailable())?,
                    },
                    "snapshot",
                    &source.local_root_id.to_string(),
                    &relative,
                    &row.get::<_, String>(1)
                        .map_err(|_| McpServiceError::unavailable())?,
                    row.get(2).map_err(|_| McpServiceError::unavailable())?,
                    FreshnessStatus::Unknown,
                ));
            }
            if out.is_empty() {
                return Err(McpServiceError::new(
                    "RESOURCE_FORBIDDEN",
                    "resource is not available",
                ));
            }
            return Ok(out);
        }
        Err(McpServiceError::new(
            "RESOURCE_NOT_FOUND",
            "resource is not available",
        ))
    }
}

fn validate_search_request(request: &RetrievalQuery) -> Result<(), McpServiceError> {
    if request.query.is_empty()
        || request.query.len() > MCP_MAX_QUERY_BYTES
        || request.root_ids.len() > MCP_MAX_ROOT_FILTERS
        || request.file_types.len() > 2
        || request.limit.get() > MCP_MAX_RESULTS
        || request.token_budget.get() > MCP_MAX_TOKEN_BUDGET
        || request.include_sensitive
    {
        return Err(McpServiceError::new(
            "MCP_INVALID_PARAMS",
            "search request exceeds a documented bound",
        ));
    }
    Ok(())
}

fn unrestricted_lexical_request() -> RetrievalQuery {
    RetrievalQuery {
        query: "snapshot".into(),
        root_ids: Vec::new(),
        file_types: Vec::<SupportedFileType>::new(),
        mode: RetrievalMode::Lexical,
        limit: crate::domain::RetrievalLimit::new(1).expect("constant is valid"),
        token_budget: crate::domain::TokenBudget::new(1).expect("constant is valid"),
        include_sensitive: false,
        budget_preset: None,
    }
}

fn context_item(hit: crate::domain::RetrievalHit, score_kind: &str) -> McpContextItem {
    let resource_uri = match &hit.origin {
        EvidenceOrigin::LocalIndex => McpResourceUri::LocalSegment(hit.segment_id),
        EvidenceOrigin::Snapshot { snapshot_id, .. } => McpResourceUri::SnapshotSegment {
            snapshot_id: snapshot_id.clone(),
            segment_id: hit.segment_id,
        },
    };
    McpContextItem {
        resource_uri: resource_uri.to_string(),
        origin: match hit.origin {
            EvidenceOrigin::LocalIndex => "local".into(),
            EvidenceOrigin::Snapshot { .. } => "snapshot".into(),
        },
        logical_root_id: hit.root_id.to_string(),
        relative_path: normalized_relative(&hit.relative_path),
        anchor: hit.anchor,
        segment_id: hit.segment_id.to_string(),
        text: hit.text,
        freshness: hit.freshness,
        explanation: hit.explanation,
        score: hit.score,
        score_kind: score_kind.into(),
        signals: hit.signals,
        content_trust: UNTRUSTED_SOURCE_EVIDENCE,
        token_estimate: hit.token_estimate,
        truncated: hit.truncated,
    }
}

fn hydrated_item(
    uri: McpResourceUri,
    origin: &str,
    root: &str,
    relative: &str,
    anchor: &str,
    text: String,
    freshness: FreshnessStatus,
) -> McpContextItem {
    let (text, truncated) = truncate_utf8(&text, HARD_BYTE_CAP.min(64 * 1024));
    let token_estimate =
        u32::try_from(RESULT_OVERHEAD_TOKENS + HeuristicTokenEstimator.estimate(&text))
            .unwrap_or(u32::MAX);
    let segment_id = match &uri {
        McpResourceUri::LocalSegment(id)
        | McpResourceUri::SnapshotSegment { segment_id: id, .. } => id.to_string(),
        McpResourceUri::Status => String::new(),
    };
    McpContextItem {
        resource_uri: uri.to_string(),
        origin: origin.into(),
        logical_root_id: root.into(),
        relative_path: normalized_relative(Path::new(relative)),
        anchor: anchor.into(),
        segment_id,
        text,
        freshness,
        explanation: MatchExplanation {
            matched_terms: Vec::new(),
            matched_excerpt: None,
            explanation_kind: crate::domain::ExplanationKind::StructuralExpansion,
        },
        score: 0.0,
        score_kind: "structural".into(),
        signals: RetrievalSignals {
            channel: "resource_hydration".into(),
            raw_bm25: None,
            public_score: 0.0,
            federation_score: None,
            lexical_rank: None,
            semantic_rank: None,
            cosine_similarity: None,
            fusion_score: None,
            embedding_space_id: None,
        },
        content_trust: UNTRUSTED_SOURCE_EVIDENCE,
        token_estimate,
        truncated,
    }
}

fn pack_context(
    candidates: Vec<McpContextItem>,
    token_budget: u32,
) -> (Vec<McpContextItem>, u32, bool) {
    let mut used = u32::try_from(RESPONSE_OVERHEAD_TOKENS).unwrap_or(u32::MAX);
    let mut items = Vec::new();
    let mut truncated = false;
    for candidate in candidates {
        if used.saturating_add(candidate.token_estimate) > token_budget {
            truncated = true;
            break;
        }
        used = used.saturating_add(candidate.token_estimate);
        items.push(candidate);
    }
    (items, used, truncated)
}

fn normalized_relative(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn bounded_warnings(warnings: Vec<String>) -> Vec<String> {
    warnings
        .into_iter()
        .take(16)
        .map(|warning| warning.chars().take(256).collect())
        .collect()
}
