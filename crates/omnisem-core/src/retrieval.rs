//! Lexical retrieval over active-only FTS5 segments.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

use globset::{Glob, GlobSetBuilder};
use rusqlite::Connection;

use crate::storage::StorageError;

use crate::config::AppConfig;
use crate::domain::{
    ContentHash, EvidenceOrigin, ExplanationKind, MatchExplanation, RetrievalAudience,
    RetrievalHit, RetrievalLimit, RetrievalMode, RetrievalQuery, RetrievalResponse,
    RetrievalSignals, RetrievalTelemetry, RevisionId, RootId, SegmentId, SensitivityScope,
    SourceFileId, SupportedFileType, Timestamp, TokenBudget,
};
use crate::embedding::{
    EmbeddingInput, EmbeddingProvider, EmbeddingSpace, EmbeddingVector, configured_provider,
    normalize_vector,
};
use crate::error::RetrievalError;
use crate::freshness::inspect_freshness;
use crate::query_parse::{ParsedQuery, parse_lexical_query};
use crate::snapshot::{eligible_snapshot_sources, open_snapshot_readonly};
use crate::tokens::{
    HARD_BYTE_CAP, HeuristicTokenEstimator, MAX_HIT_TEXT_BYTES, RESPONSE_OVERHEAD_TOKENS,
    RESULT_OVERHEAD_TOKENS, TokenEstimator, estimate_response_tokens, truncate_utf8,
};

/// Maximum FTS candidates fetched before packing (hard bound).
pub const MAX_CANDIDATES: usize = 200;
/// Multiplier of the final limit used when fetching candidates.
pub const CANDIDATE_MULTIPLIER: usize = 8;
/// Maximum active vectors examined by the exact baseline.
pub const MAX_ACTIVE_VECTORS: usize = 50_000;
/// Maximum semantic candidates retained before packing or fusion.
pub const MAX_SEMANTIC_CANDIDATES: usize = 200;
/// Maximum candidates admitted to a single fusion pass.
pub const MAX_FUSION_CANDIDATES: usize = 768;

/// Provider-independent synchronous exact-vector boundary.
pub trait VectorSearch: Send + Sync {
    /// Searches compatible active vectors.
    ///
    /// # Errors
    /// Returns a typed retrieval failure for storage corruption or safety-bound violations.
    fn search(
        &self,
        connection: &Connection,
        config: &AppConfig,
        space: &EmbeddingSpace,
        query: &EmbeddingVector,
        filters: &RetrievalQuery,
        audience: RetrievalAudience,
        limit: usize,
    ) -> Result<VectorSearchReport, RetrievalError>;
}

#[derive(Debug)]
pub struct VectorSearchReport {
    pub hits: Vec<RetrievalHit>,
    pub examined: u32,
    pub corrupt_excluded: u32,
}

/// Injectable runtime used by deterministic tests and production transport.
pub struct RetrievalRuntime<'a> {
    pub embedding_provider: Option<&'a dyn EmbeddingProvider>,
    pub vector_search: &'a dyn VectorSearch,
}

pub struct SqliteExactVectorSearch {
    max_active_vectors: usize,
    max_candidates: usize,
}

impl Default for SqliteExactVectorSearch {
    fn default() -> Self {
        Self {
            max_active_vectors: MAX_ACTIVE_VECTORS,
            max_candidates: MAX_SEMANTIC_CANDIDATES,
        }
    }
}

#[cfg(test)]
impl SqliteExactVectorSearch {
    fn bounded(max_active_vectors: usize, max_candidates: usize) -> Self {
        Self {
            max_active_vectors,
            max_candidates,
        }
    }
}

#[derive(Debug, Clone)]
struct RawCandidate {
    segment_id: SegmentId,
    revision_id: RevisionId,
    source_file_id: SourceFileId,
    root_id: RootId,
    relative_path: PathBuf,
    anchor: String,
    text: String,
    text_hash: ContentHash,
    file_type: SupportedFileType,
    root_path: PathBuf,
    modified_at: Option<Timestamp>,
    raw_bm25: f32,
    highlighted: String,
}

impl VectorSearch for SqliteExactVectorSearch {
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    fn search(
        &self,
        connection: &Connection,
        config: &AppConfig,
        space: &EmbeddingSpace,
        query: &EmbeddingVector,
        filters: &RetrievalQuery,
        audience: RetrievalAudience,
        limit: usize,
    ) -> Result<VectorSearchReport, RetrievalError> {
        let eligible: u64 = connection.query_row(
            "SELECT count(*) FROM segment_embeddings se JOIN segments s ON s.id=se.segment_id JOIN source_files sf ON sf.current_revision_id=s.revision_id AND sf.state='active' JOIN roots r ON r.id=sf.root_id AND r.enabled=1 WHERE se.embedding_space_id=?1",
            [&space.id], |row| row.get(0)).map_err(StorageError::from)?;
        // The safety bound is deliberately conservative and applies to the complete
        // compatible active space before request filters.
        if eligible > self.max_active_vectors as u64 {
            return Err(semantic_error(
                "VECTOR_SCAN_LIMIT_EXCEEDED",
                "eligible active-vector count exceeds the exact-scan safety bound",
            ));
        }
        let sensitivity = load_sensitivity_sets(config);
        let mut statement = connection.prepare(
            "SELECT s.id,s.revision_id,sf.id,sf.root_id,sf.relative_path,sf.file_type,s.anchor,s.text,s.text_hash,r.canonical_path,sf.modified_at_ms,ev.vector_bytes,ev.dimensions FROM segment_embeddings se JOIN embedding_vectors ev ON ev.embedding_space_id=se.embedding_space_id AND ev.text_hash=se.text_hash JOIN segments s ON s.id=se.segment_id AND s.revision_id=se.revision_id JOIN source_files sf ON sf.current_revision_id=s.revision_id AND sf.state='active' JOIN roots r ON r.id=sf.root_id AND r.enabled=1 WHERE se.embedding_space_id=?1 ORDER BY sf.relative_path,s.anchor,s.id"
        ).map_err(StorageError::from)?;
        let mut rows = statement.query([&space.id]).map_err(StorageError::from)?;
        let mut scored: Vec<(f64, RetrievalHit)> = Vec::new();
        let mut examined = 0u32;
        let mut corrupt = 0u32;
        while let Some(row) = rows.next().map_err(StorageError::from)? {
            let root_id = RootId::from_str(&row.get::<_, String>(3).map_err(StorageError::from)?)
                .map_err(|e| RetrievalError::Internal(e.to_string()))?;
            let relative_path = PathBuf::from(row.get::<_, String>(4).map_err(StorageError::from)?);
            let file_type =
                SupportedFileType::from_str(&row.get::<_, String>(5).map_err(StorageError::from)?)
                    .map_err(|e| RetrievalError::Internal(e.to_string()))?;
            if (!filters.root_ids.is_empty() && !filters.root_ids.contains(&root_id))
                || (!filters.file_types.is_empty() && !filters.file_types.contains(&file_type))
            {
                continue;
            }
            let scope = sensitivity_for_path(&sensitivity, &root_id, &relative_path);
            if !sensitivity_allowed(scope, audience, filters.include_sensitive) {
                continue;
            }
            examined = examined.saturating_add(1);
            let dimensions = row.get::<_, u32>(12).map_err(StorageError::from)?;
            let bytes = row.get::<_, Vec<u8>>(11).map_err(StorageError::from)?;
            if dimensions != space.dimensions {
                corrupt = corrupt.saturating_add(1);
                continue;
            }
            let Ok(candidate) = crate::embedding::decode_vector(&bytes, dimensions) else {
                corrupt = corrupt.saturating_add(1);
                continue;
            };
            let similarity = query
                .values
                .iter()
                .zip(&candidate.values)
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum::<f64>();
            if !similarity.is_finite() {
                corrupt = corrupt.saturating_add(1);
                continue;
            }
            let similarity = similarity.clamp(-1.0, 1.0);
            let text: String = row.get(7).map_err(StorageError::from)?;
            let (text, truncated) = truncate_utf8(&text, MAX_HIT_TEXT_BYTES);
            let token_estimate =
                u32::try_from(RESULT_OVERHEAD_TOKENS + HeuristicTokenEstimator.estimate(&text))
                    .unwrap_or(u32::MAX);
            let segment_id =
                SegmentId::from_str(&row.get::<_, String>(0).map_err(StorageError::from)?)
                    .map_err(|e| RetrievalError::Internal(e.to_string()))?;
            let anchor: String = row.get(6).map_err(StorageError::from)?;
            scored.push((
                similarity,
                RetrievalHit {
                    segment_id,
                    revision_id: RevisionId::from_str(
                        &row.get::<_, String>(1).map_err(StorageError::from)?,
                    )
                    .map_err(|e| RetrievalError::Internal(e.to_string()))?,
                    source_file_id: SourceFileId::from_str(
                        &row.get::<_, String>(2).map_err(StorageError::from)?,
                    )
                    .map_err(|e| RetrievalError::Internal(e.to_string()))?,
                    root_id,
                    relative_path,
                    file_type,
                    anchor,
                    text,
                    text_hash: ContentHash(row.get(8).map_err(StorageError::from)?),
                    score: similarity as f32,
                    signals: RetrievalSignals {
                        channel: "local_semantic".into(),
                        raw_bm25: None,
                        public_score: similarity as f32,
                        federation_score: None,
                        lexical_rank: None,
                        semantic_rank: None,
                        cosine_similarity: Some(similarity as f32),
                        fusion_score: None,
                        embedding_space_id: Some(space.id.clone()),
                    },
                    explanation: MatchExplanation {
                        matched_terms: Vec::new(),
                        matched_excerpt: None,
                        explanation_kind: ExplanationKind::SemanticNeighbor,
                    },
                    freshness: inspect_freshness(
                        Path::new(&row.get::<_, String>(9).map_err(StorageError::from)?),
                        &PathBuf::from(row.get::<_, String>(4).map_err(StorageError::from)?),
                        row.get::<_, Option<i64>>(10)
                            .map_err(StorageError::from)?
                            .map(Timestamp::from_millis),
                    ),
                    sensitivity_scope: scope,
                    token_estimate,
                    truncated,
                    origin: EvidenceOrigin::LocalIndex,
                },
            ));
        }
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.relative_path.cmp(&b.1.relative_path))
                .then_with(|| a.1.anchor.cmp(&b.1.anchor))
                .then_with(|| a.1.segment_id.to_string().cmp(&b.1.segment_id.to_string()))
        });
        scored.truncate(limit.min(self.max_candidates));
        let hits: Vec<RetrievalHit> = scored
            .into_iter()
            .enumerate()
            .map(|(rank, (_, mut hit))| {
                hit.signals.semantic_rank = Some(u32::try_from(rank + 1).unwrap_or(u32::MAX));
                hit
            })
            .collect();
        if hits.is_empty() && corrupt > 0 {
            return Err(semantic_error(
                "VECTOR_SEARCH_FAILED",
                "no valid semantic candidates remained after corrupt-vector exclusion",
            ));
        }
        Ok(VectorSearchReport {
            hits,
            examined,
            corrupt_excluded: corrupt,
        })
    }
}

/// Executes lexical retrieval for a validated query against the open index.
///
/// # Errors
///
/// Returns typed retrieval, configuration, or database failures.
pub fn retrieve(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
) -> Result<RetrievalResponse, RetrievalError> {
    retrieve_for_audience(connection, config, request, RetrievalAudience::LocalUser)
}

/// Executes retrieval with audience-specific sensitivity enforcement.
///
/// # Errors
/// Returns typed retrieval, configuration, or database failures.
pub fn retrieve_for_audience(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    audience: RetrievalAudience,
) -> Result<RetrievalResponse, RetrievalError> {
    if request.mode == RetrievalMode::Lexical
        || (request.mode == RetrievalMode::Auto && !config.embeddings.enabled)
    {
        return retrieve_lexical(
            connection,
            config,
            request,
            request.mode,
            Vec::new(),
            Vec::new(),
            RetrievalTelemetry::default(),
            audience,
        );
    }
    let scanner = SqliteExactVectorSearch::default();
    match configured_provider(&config.embeddings) {
        Ok(provider) => retrieve_with_runtime_for_audience(
            connection,
            config,
            request,
            &RetrievalRuntime {
                embedding_provider: Some(provider.as_ref()),
                vector_search: &scanner,
            },
            audience,
        ),
        Err(error) if request.mode == RetrievalMode::Auto => retrieve_lexical(
            connection,
            config,
            request,
            request.mode,
            Vec::new(),
            vec![bounded_warning(
                "semantic channel unavailable",
                &error.to_string(),
            )],
            RetrievalTelemetry::default(),
            audience,
        ),
        Err(error) => Err(semantic_error(error.code(), &error.to_string())),
    }
}

/// Executes retrieval with injected provider and vector-search implementations.
///
/// # Errors
/// Returns typed validation, provider, compatibility, vector-search, or storage failures.
pub fn retrieve_with_runtime(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    runtime: &RetrievalRuntime<'_>,
) -> Result<RetrievalResponse, RetrievalError> {
    retrieve_with_runtime_for_audience(
        connection,
        config,
        request,
        runtime,
        RetrievalAudience::LocalUser,
    )
}

/// Executes retrieval with injected runtime and audience-specific visibility.
///
/// # Errors
/// Returns typed validation, provider, compatibility, vector-search, or storage failures.
pub fn retrieve_with_runtime_for_audience(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    runtime: &RetrievalRuntime<'_>,
    audience: RetrievalAudience,
) -> Result<RetrievalResponse, RetrievalError> {
    match request.mode {
        RetrievalMode::Lexical => retrieve_lexical(
            connection,
            config,
            request,
            request.mode,
            Vec::new(),
            Vec::new(),
            RetrievalTelemetry::default(),
            audience,
        ),
        RetrievalMode::Semantic => {
            semantic_response(connection, config, request, runtime, audience)
        }
        RetrievalMode::Hybrid => {
            let semantic = semantic_hits(connection, config, request, runtime, audience)?;
            retrieve_lexical(
                connection,
                config,
                request,
                request.mode,
                semantic.hits,
                semantic.warnings,
                semantic.telemetry,
                audience,
            )
        }
        RetrievalMode::Auto => {
            match semantic_hits(connection, config, request, runtime, audience) {
                Ok(semantic) => retrieve_lexical(
                    connection,
                    config,
                    request,
                    request.mode,
                    semantic.hits,
                    semantic.warnings,
                    semantic.telemetry,
                    audience,
                ),
                Err(error) => retrieve_lexical(
                    connection,
                    config,
                    request,
                    request.mode,
                    Vec::new(),
                    vec![bounded_warning(
                        "semantic channel unavailable; using lexical",
                        &error.to_string(),
                    )],
                    RetrievalTelemetry::default(),
                    audience,
                ),
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn retrieve_lexical(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    requested_mode: RetrievalMode,
    semantic_hits: Vec<RetrievalHit>,
    mut initial_warnings: Vec<String>,
    mut telemetry: RetrievalTelemetry,
    audience: RetrievalAudience,
) -> Result<RetrievalResponse, RetrievalError> {
    let started = Instant::now();
    let effective_mode = if semantic_hits.is_empty() {
        RetrievalMode::Lexical
    } else {
        RetrievalMode::Hybrid
    };
    let parsed = parse_lexical_query(&request.query)?;
    let estimator = HeuristicTokenEstimator;

    let candidate_limit =
        (usize::from(request.limit.get()) * CANDIDATE_MULTIPLIER).clamp(1, MAX_CANDIDATES);
    let mut candidates = search_fts(connection, request, &parsed, candidate_limit)?;
    let mut warnings = std::mem::take(&mut initial_warnings);

    let before_dedupe = candidates.len();
    candidates = suppress_duplicates(candidates);
    let duplicates_suppressed =
        u32::try_from(before_dedupe.saturating_sub(candidates.len())).unwrap_or(u32::MAX);

    let sensitivity = load_sensitivity_sets(config);
    candidates = filter_sensitivity(
        candidates,
        &sensitivity,
        request.include_sensitive,
        audience,
    );

    let mut hits = Vec::new();
    for (rank, candidate) in candidates.into_iter().enumerate() {
        let freshness = inspect_freshness(
            &candidate.root_path,
            &candidate.relative_path,
            candidate.modified_at,
        );
        let matched_terms = matched_terms_for(&parsed, &candidate.text);
        let excerpt = excerpt_from_highlight(&candidate.highlighted, &candidate.text);
        let (text, truncated) = truncate_utf8(&candidate.text, MAX_HIT_TEXT_BYTES);
        let token_estimate =
            u32::try_from(RESULT_OVERHEAD_TOKENS + estimator.estimate(&text)).unwrap_or(u32::MAX);
        let sensitivity_scope =
            sensitivity_for_path(&sensitivity, &candidate.root_id, &candidate.relative_path);
        let public_score = public_score_from_bm25(candidate.raw_bm25);
        hits.push(RetrievalHit {
            segment_id: candidate.segment_id,
            revision_id: candidate.revision_id,
            source_file_id: candidate.source_file_id,
            root_id: candidate.root_id,
            relative_path: candidate.relative_path,
            file_type: candidate.file_type,
            anchor: candidate.anchor,
            text,
            text_hash: candidate.text_hash,
            score: public_score,
            signals: RetrievalSignals {
                channel: "lexical_fts5".into(),
                raw_bm25: Some(candidate.raw_bm25),
                public_score,
                federation_score: None,
                lexical_rank: Some(u32::try_from(rank + 1).unwrap_or(u32::MAX)),
                semantic_rank: None,
                cosine_similarity: None,
                fusion_score: None,
                embedding_space_id: None,
            },
            explanation: MatchExplanation {
                matched_terms,
                matched_excerpt: Some(excerpt),
                explanation_kind: ExplanationKind::LexicalTermOverlap,
            },
            freshness,
            sensitivity_scope,
            token_estimate,
            truncated,
            origin: EvidenceOrigin::LocalIndex,
        });
    }

    // Federate eligible imported snapshots via Reciprocal Rank Fusion.
    telemetry.local_lexical_candidates = u32::try_from(hits.len()).unwrap_or(u32::MAX);
    let fusion = federate_with_snapshots(
        connection,
        config,
        request,
        &parsed,
        hits,
        &sensitivity,
        &mut warnings,
        semantic_hits,
        audience,
    );
    hits = fusion.hits;
    telemetry.snapshot_lexical_candidates = fusion.snapshot_candidates;
    telemetry.semantic_candidates = fusion.semantic_candidates;
    telemetry.candidates_admitted_to_fusion = fusion.admitted;
    telemetry.unique_fused_candidates = fusion.unique;
    telemetry.fusion_duplicates_suppressed = fusion.duplicates;

    let packed = pack_results(
        &request.query,
        hits,
        request.limit.get(),
        request.token_budget.get(),
        &estimator,
        &mut warnings,
    );

    let token_estimate = u32::try_from(estimate_response_tokens(
        &estimator,
        &request.query,
        &packed
            .results
            .iter()
            .map(|hit| hit.text.as_str())
            .collect::<Vec<_>>(),
    ))
    .unwrap_or(u32::MAX);

    Ok(RetrievalResponse {
        query: request.query.clone(),
        requested_mode,
        mode: effective_mode,
        score_kind:
            if effective_mode == RetrievalMode::Hybrid || hits_use_fusion(&packed.results) {
                "rrf"
            } else {
                "bm25_public"
            }
            .into(),
        results: packed.results,
        token_estimate,
        truncated: packed.truncated,
        applied_limit: request.limit.get(),
        applied_token_budget: request.token_budget.get(),
        budget_preset: request.budget_preset.clone(),
        duplicates_suppressed,
        warnings,
        telemetry,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

fn hits_use_fusion(hits: &[RetrievalHit]) -> bool {
    hits.iter().any(|hit| hit.signals.fusion_score.is_some())
}
fn semantic_error(code: &'static str, message: &str) -> RetrievalError {
    RetrievalError::Semantic {
        code,
        message: message.chars().take(240).collect(),
    }
}
fn bounded_warning(prefix: &str, message: &str) -> String {
    format!(
        "{prefix}: {}",
        message.chars().take(180).collect::<String>()
    )
}

struct SemanticChannel {
    hits: Vec<RetrievalHit>,
    warnings: Vec<String>,
    telemetry: RetrievalTelemetry,
}

fn validate_query_text(query: &str) -> Result<&str, RetrievalError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(semantic_error("QUERY_INVALID", "query is empty"));
    }
    if trimmed.chars().count() > crate::query_parse::MAX_QUERY_CHARS {
        return Err(semantic_error(
            "QUERY_INVALID",
            "query exceeds the configured character bound",
        ));
    }
    if trimmed
        .chars()
        .filter(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        .count()
        > 8
    {
        return Err(semantic_error(
            "QUERY_INVALID",
            "query contains too many control characters",
        ));
    }
    Ok(trimmed)
}

fn load_active_space(connection: &Connection) -> Result<EmbeddingSpace, RetrievalError> {
    let row = connection.query_row(
        "SELECT es.id,es.provider,es.canonical_model,es.model_digest,es.dimensions,es.normalization,es.input_contract_version FROM embedding_state st JOIN embedding_spaces es ON es.id=st.active_embedding_space_id WHERE st.singleton=1",
        [],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,u32>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?)),
    ).map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => semantic_error("EMBEDDING_SPACE_MISSING", "no active embedding space; run omnisem index"),
        other => RetrievalError::Storage(StorageError::from(other)),
    })?;
    if row.1 != "ollama"
        || row.5 != "l2"
        || row.6 != crate::embedding::EMBEDDING_INPUT_CONTRACT_VERSION
    {
        return Err(semantic_error(
            "EMBEDDING_SPACE_INCOMPATIBLE",
            "active embedding-space contract is unsupported",
        ));
    }
    Ok(EmbeddingSpace {
        id: row.0,
        provider: crate::embedding::EmbeddingProviderKind::Ollama,
        canonical_model: row.2,
        model_digest: row.3,
        dimensions: row.4,
        normalization: crate::embedding::VectorNormalization::L2,
        input_contract_version: row.6,
    })
}

#[allow(clippy::too_many_lines)]
fn semantic_hits(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    runtime: &RetrievalRuntime<'_>,
    audience: RetrievalAudience,
) -> Result<SemanticChannel, RetrievalError> {
    let query = validate_query_text(&request.query)?;
    let provider = runtime.embedding_provider.ok_or_else(|| {
        semantic_error("SEMANTIC_UNAVAILABLE", "embedding provider is unavailable")
    })?;
    let space = load_active_space(connection)?;
    if provider.provider_kind() != space.provider {
        return Err(semantic_error(
            "EMBEDDING_SPACE_INCOMPATIBLE",
            "configured provider differs from active space",
        ));
    }
    if config.embeddings.dimensions != 0 && config.embeddings.dimensions != space.dimensions {
        return Err(semantic_error(
            "EMBEDDING_SPACE_INCOMPATIBLE",
            "configured dimensions differ from active space; run omnisem index",
        ));
    }
    let linked: u32 = connection.query_row(
        "SELECT count(*) FROM segment_embeddings se JOIN segments s ON s.id=se.segment_id JOIN source_files sf ON sf.current_revision_id=s.revision_id AND sf.state='active' JOIN roots r ON r.id=sf.root_id AND r.enabled=1 WHERE se.embedding_space_id=?1",
        [&space.id], |row| row.get(0)).map_err(StorageError::from)?;
    if linked == 0 {
        return Err(semantic_error(
            "SEMANTIC_UNAVAILABLE",
            "active embedding space has no linked active segments; run omnisem index",
        ));
    }
    // Resolution and embedding occur with no SQLite transaction or statement alive.
    let model = provider
        .resolve_model()
        .map_err(|error| semantic_error(error.code(), &error.to_string()))?;
    if model.provider != space.provider || model.canonical_name != space.canonical_model {
        return Err(semantic_error(
            "EMBEDDING_SPACE_INCOMPATIBLE",
            "resolved model identity differs from active space; run omnisem index",
        ));
    }
    if model.model_digest != space.model_digest {
        return Err(semantic_error(
            "EMBEDDING_MODEL_CHANGED",
            "configured model digest changed; run omnisem index",
        ));
    }
    if model
        .dimensions
        .is_some_and(|dimensions| dimensions != space.dimensions)
    {
        return Err(semantic_error(
            "EMBEDDING_SPACE_INCOMPATIBLE",
            "configured dimensions differ from active space; run omnisem index",
        ));
    }
    let input = EmbeddingInput::query(query.to_owned());
    let embedding_started = Instant::now();
    let batch = provider
        .embed(&[input], &model)
        .map_err(|error| semantic_error("QUERY_EMBEDDING_FAILED", &error.to_string()))?;
    let query_embedding_ms = embedding_started.elapsed().as_secs_f64() * 1_000.0;
    if batch.vectors.len() != 1 {
        return Err(semantic_error(
            "QUERY_EMBEDDING_FAILED",
            "provider returned an invalid query-vector count",
        ));
    }
    let vector = normalize_vector(&batch.vectors[0].values)
        .map_err(|error| semantic_error("QUERY_EMBEDDING_FAILED", &error.to_string()))?;
    if vector.values.len() != space.dimensions as usize {
        return Err(semantic_error(
            "EMBEDDING_SPACE_INCOMPATIBLE",
            "query-vector dimensions differ from active space",
        ));
    }
    let scan_started = Instant::now();
    let report = runtime.vector_search.search(
        connection,
        config,
        &space,
        &vector,
        request,
        audience,
        MAX_SEMANTIC_CANDIDATES,
    )?;
    let vector_scan_ms = scan_started.elapsed().as_secs_f64() * 1_000.0;
    let warnings = (report.corrupt_excluded > 0)
        .then(|| {
            format!(
                "{} corrupt vectors excluded from semantic search",
                report.corrupt_excluded
            )
        })
        .into_iter()
        .collect();
    let semantic_candidates = u32::try_from(report.hits.len()).unwrap_or(u32::MAX);
    Ok(SemanticChannel {
        hits: report.hits,
        warnings,
        telemetry: RetrievalTelemetry {
            query_embedding_ms,
            vector_scan_ms,
            active_vectors_examined: report.examined,
            corrupt_vectors_excluded: report.corrupt_excluded,
            semantic_candidates,
            ..RetrievalTelemetry::default()
        },
    })
}

fn semantic_response(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    runtime: &RetrievalRuntime<'_>,
    audience: RetrievalAudience,
) -> Result<RetrievalResponse, RetrievalError> {
    let started = Instant::now();
    let mut channel = semantic_hits(connection, config, request, runtime, audience)?;
    let estimator = HeuristicTokenEstimator;
    let packed = pack_results(
        &request.query,
        channel.hits,
        request.limit.get(),
        request.token_budget.get(),
        &estimator,
        &mut channel.warnings,
    );
    let token_estimate = u32::try_from(estimate_response_tokens(
        &estimator,
        &request.query,
        &packed
            .results
            .iter()
            .map(|hit| hit.text.as_str())
            .collect::<Vec<_>>(),
    ))
    .unwrap_or(u32::MAX);
    Ok(RetrievalResponse {
        query: request.query.clone(),
        requested_mode: request.mode,
        mode: RetrievalMode::Semantic,
        score_kind: "cosine_similarity".into(),
        results: packed.results,
        token_estimate,
        truncated: packed.truncated,
        applied_limit: request.limit.get(),
        applied_token_budget: request.token_budget.get(),
        budget_preset: request.budget_preset.clone(),
        duplicates_suppressed: 0,
        warnings: channel.warnings,
        telemetry: channel.telemetry,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Maps FTS5 BM25 into a public lexical score.
///
/// `SQLite` `FTS5` BM25 is lower-is-better and is typically negative for ordinary
/// matches (more negative is better). The public score is:
///
/// ```text
/// public_score = -raw_bm25
/// ```
///
/// so higher public scores are better. This is a monotonic transform of the raw
/// rank value, not a probability and not normalized to `[0, 1]`. Lexical scores
/// are not comparable with future semantic or hybrid scores.
#[must_use]
pub fn public_score_from_bm25(raw_bm25: f32) -> f32 {
    if !raw_bm25.is_finite() {
        return f32::NEG_INFINITY;
    }
    -raw_bm25
}

#[allow(clippy::too_many_lines)]
fn search_fts(
    connection: &Connection,
    request: &RetrievalQuery,
    parsed: &ParsedQuery,
    limit: usize,
) -> Result<Vec<RawCandidate>, RetrievalError> {
    let mut sql = String::from(
        "SELECT
            fts.segment_id,
            fts.revision_id,
            fts.source_file_id,
            fts.root_id,
            fts.relative_path,
            fts.anchor,
            fts.text,
            s.text_hash,
            sf.file_type,
            r.canonical_path,
            sf.modified_at_ms,
            bm25(segments_fts) AS rank,
            highlight(segments_fts, 0, '«', '»') AS highlighted
         FROM segments_fts AS fts
         INNER JOIN source_files AS sf
            ON sf.id = fts.source_file_id
           AND sf.state = 'active'
           AND sf.current_revision_id = fts.revision_id
         INNER JOIN roots AS r
            ON r.id = fts.root_id
           AND r.enabled = 1
         INNER JOIN segments AS s
            ON s.id = fts.segment_id
         WHERE segments_fts MATCH ?1",
    );

    let mut bind: Vec<String> = vec![parsed.fts_match.clone()];
    if !request.root_ids.is_empty() {
        let placeholders = (0..request.root_ids.len())
            .map(|index| format!("?{}", index + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(sql, " AND fts.root_id IN ({placeholders})");
        for root in &request.root_ids {
            bind.push(root.to_string());
        }
    }
    if !request.file_types.is_empty() {
        let base = bind.len() + 1;
        let placeholders = (0..request.file_types.len())
            .map(|index| format!("?{}", base + index))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(sql, " AND sf.file_type IN ({placeholders})");
        for file_type in &request.file_types {
            bind.push(file_type.as_str().to_owned());
        }
    }
    sql.push_str(
        " ORDER BY rank ASC, fts.relative_path ASC, fts.anchor ASC, fts.segment_id ASC LIMIT ?",
    );

    let mut statement = connection.prepare(&sql).map_err(StorageError::from)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = bind
        .into_iter()
        .map(|value| Box::new(value) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    params.push(Box::new(i64::try_from(limit).unwrap_or(i64::MAX)));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();
    let mut rows = statement
        .query(param_refs.as_slice())
        .map_err(StorageError::from)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(StorageError::from)? {
        let get_string = |idx: usize| -> Result<String, RetrievalError> {
            row.get(idx)
                .map_err(|error| RetrievalError::Storage(StorageError::from(error)))
        };
        let get_f64 = |idx: usize| -> Result<f64, RetrievalError> {
            row.get(idx)
                .map_err(|error| RetrievalError::Storage(StorageError::from(error)))
        };
        let get_opt_i64 = |idx: usize| -> Result<Option<i64>, RetrievalError> {
            row.get(idx)
                .map_err(|error| RetrievalError::Storage(StorageError::from(error)))
        };
        out.push(RawCandidate {
            segment_id: SegmentId::from_str(&get_string(0)?)
                .map_err(|error| RetrievalError::Internal(error.to_string()))?,
            revision_id: RevisionId::from_str(&get_string(1)?)
                .map_err(|error| RetrievalError::Internal(error.to_string()))?,
            source_file_id: SourceFileId::from_str(&get_string(2)?)
                .map_err(|error| RetrievalError::Internal(error.to_string()))?,
            root_id: RootId::from_str(&get_string(3)?)
                .map_err(|error| RetrievalError::Internal(error.to_string()))?,
            relative_path: PathBuf::from(get_string(4)?),
            anchor: get_string(5)?,
            text: get_string(6)?,
            text_hash: ContentHash(get_string(7)?),
            file_type: SupportedFileType::from_str(&get_string(8)?)
                .map_err(|error| RetrievalError::Internal(error.to_string()))?,
            root_path: PathBuf::from(get_string(9)?),
            modified_at: get_opt_i64(10)?.map(Timestamp::from_millis),
            raw_bm25: {
                #[allow(clippy::cast_possible_truncation)]
                {
                    get_f64(11)? as f32
                }
            },
            highlighted: get_string(12)?,
        });
    }
    Ok(out)
}

fn suppress_duplicates(candidates: Vec<RawCandidate>) -> Vec<RawCandidate> {
    let mut seen_hashes = HashSet::new();
    let mut seen_segments = HashSet::new();
    let mut kept = Vec::new();
    for candidate in candidates {
        if !seen_segments.insert(candidate.segment_id.to_string()) {
            continue;
        }
        if !seen_hashes.insert(candidate.text_hash.0.clone()) {
            continue;
        }
        kept.push(candidate);
    }
    kept
}

struct SensitivityIndex {
    by_root: HashMap<String, Vec<(globset::GlobSet, SensitivityScope)>>,
}

fn load_sensitivity_sets(config: &AppConfig) -> SensitivityIndex {
    let mut by_root = HashMap::new();
    for root in &config.roots {
        let mut entries = Vec::new();
        for tag in &root.sensitivity {
            let Ok(scope) = SensitivityScope::from_str(&tag.scope) else {
                continue;
            };
            let mut builder = GlobSetBuilder::new();
            if let Ok(glob) = Glob::new(&tag.pattern) {
                builder.add(glob);
            }
            if let Ok(set) = builder.build() {
                entries.push((set, scope));
            }
        }
        by_root.insert(root.id.clone(), entries);
    }
    SensitivityIndex { by_root }
}

fn sensitivity_for_path(
    index: &SensitivityIndex,
    root_id: &RootId,
    relative_path: &Path,
) -> Option<SensitivityScope> {
    let path = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let entries = index.by_root.get(&root_id.to_string())?;
    let mut matched = None;
    for (set, scope) in entries {
        if set.is_match(&path) {
            // Prefer NeverReturnToMcp when both match.
            matched = Some(match (matched, *scope) {
                (Some(SensitivityScope::NeverReturnToMcp), _)
                | (_, SensitivityScope::NeverReturnToMcp) => SensitivityScope::NeverReturnToMcp,
                (_, other) => other,
            });
        }
    }
    matched
}

fn filter_sensitivity(
    candidates: Vec<RawCandidate>,
    index: &SensitivityIndex,
    include_sensitive: bool,
    audience: RetrievalAudience,
) -> Vec<RawCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| {
            sensitivity_allowed(
                sensitivity_for_path(index, &candidate.root_id, &candidate.relative_path),
                audience,
                include_sensitive,
            )
        })
        .collect()
}

fn sensitivity_allowed(
    scope: Option<SensitivityScope>,
    audience: RetrievalAudience,
    include_sensitive: bool,
) -> bool {
    match (audience, scope) {
        (RetrievalAudience::Mcp, Some(_)) => false,
        (RetrievalAudience::LocalUser, Some(SensitivityScope::RequireExplicitQuery)) => {
            include_sensitive
        }
        (RetrievalAudience::LocalUser, _) | (RetrievalAudience::Mcp, None) => true,
    }
}

/// Returns whether a configured path is visible to the requested audience.
#[must_use]
pub fn path_allowed_for_audience(
    config: &AppConfig,
    root_id: &RootId,
    relative_path: &Path,
    audience: RetrievalAudience,
    include_sensitive: bool,
) -> bool {
    let sensitivity = load_sensitivity_sets(config);
    sensitivity_allowed(
        sensitivity_for_path(&sensitivity, root_id, relative_path),
        audience,
        include_sensitive,
    )
}

fn matched_terms_for(parsed: &ParsedQuery, text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    parsed
        .terms
        .iter()
        .filter(|term| lower.contains(&term.to_lowercase()))
        .cloned()
        .collect()
}

fn excerpt_from_highlight(highlighted: &str, fallback: &str) -> String {
    let source = if highlighted.trim().is_empty() {
        fallback
    } else {
        highlighted
    };
    truncate_utf8(source, 240).0
}

struct Packed {
    results: Vec<RetrievalHit>,
    truncated: bool,
}

fn pack_results(
    query: &str,
    hits: Vec<RetrievalHit>,
    limit: u16,
    token_budget: u32,
    estimator: &dyn TokenEstimator,
    warnings: &mut Vec<String>,
) -> Packed {
    let budget = usize::try_from(token_budget).unwrap_or(usize::MAX);
    let limit = usize::from(limit);
    let mut selected = Vec::new();
    let mut used_tokens = RESPONSE_OVERHEAD_TOKENS + estimator.estimate(query);
    let mut used_bytes = 0usize;
    let mut truncated = false;

    for (index, mut hit) in hits.into_iter().enumerate() {
        if selected.len() >= limit {
            truncated = true;
            break;
        }
        let mut text = hit.text.clone();
        let mut text_truncated = hit.truncated;
        let mut cost = RESULT_OVERHEAD_TOKENS + estimator.estimate(&text);
        let mut bytes = text.len();

        if used_tokens.saturating_add(cost) > budget
            || used_bytes.saturating_add(bytes) > HARD_BYTE_CAP
        {
            if selected.is_empty() {
                // Always try to return the top result via UTF-8-safe truncation.
                let allowed_tokens = budget.saturating_sub(used_tokens + RESULT_OVERHEAD_TOKENS);
                let max_chars = allowed_tokens.saturating_mul(3).max(1);
                let char_bound = text.chars().take(max_chars).collect::<String>();
                let byte_bound = HARD_BYTE_CAP.saturating_sub(used_bytes).max(1);
                let (short, was_truncated) =
                    truncate_utf8(&char_bound, byte_bound.min(MAX_HIT_TEXT_BYTES));
                text = short;
                text_truncated = true;
                let _ = was_truncated;
                cost = RESULT_OVERHEAD_TOKENS + estimator.estimate(&text);
                bytes = text.len();
                if used_tokens.saturating_add(cost) > budget {
                    warnings.push("top result truncated to fit the configured token budget".into());
                }
            } else {
                truncated = true;
                break;
            }
        }

        hit.text = text;
        hit.truncated = text_truncated;
        hit.token_estimate = u32::try_from(cost).unwrap_or(u32::MAX);
        used_tokens = used_tokens.saturating_add(cost);
        used_bytes = used_bytes.saturating_add(bytes);
        selected.push(hit);
        let _ = index;
    }

    if hits_remaining_after(&selected, limit) {
        truncated = true;
    }

    Packed {
        results: selected,
        truncated,
    }
}

fn hits_remaining_after(selected: &[RetrievalHit], limit: usize) -> bool {
    selected.len() >= limit
}

/// Maximum imported snapshots consulted per query.
pub const MAX_SNAPSHOTS_PER_QUERY: usize = 8;
/// Candidates requested from each snapshot index.
pub const SNAPSHOT_CANDIDATES: usize = 32;
/// RRF constant k.
pub const RRF_K: f32 = 60.0;

struct FusionReport {
    hits: Vec<RetrievalHit>,
    snapshot_candidates: u32,
    semantic_candidates: u32,
    admitted: u32,
    unique: u32,
    duplicates: u32,
}

impl FusionReport {
    fn unfused(hits: Vec<RetrievalHit>) -> Self {
        Self {
            hits,
            snapshot_candidates: 0,
            semantic_candidates: 0,
            admitted: 0,
            unique: 0,
            duplicates: 0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn federate_with_snapshots(
    connection: &Connection,
    config: &AppConfig,
    request: &RetrievalQuery,
    parsed: &ParsedQuery,
    local_hits: Vec<RetrievalHit>,
    sensitivity: &SensitivityIndex,
    warnings: &mut Vec<String>,
    semantic_hits: Vec<RetrievalHit>,
    audience: RetrievalAudience,
) -> FusionReport {
    let semantic_candidates = u32::try_from(semantic_hits.len()).unwrap_or(u32::MAX);
    let sources = match eligible_snapshot_sources(connection, config, request) {
        Ok(sources) => sources,
        Err(error) => {
            warnings.push(format!("snapshot federation skipped: {error}"));
            if semantic_hits.is_empty() {
                return FusionReport::unfused(local_hits);
            }
            let mut report = rrf_merge(&[local_hits, semantic_hits]);
            report.semantic_candidates = semantic_candidates;
            return report;
        }
    };
    if sources.is_empty() && semantic_hits.is_empty() {
        // Local-only path remains stable: score stays public BM25-derived.
        return FusionReport::unfused(local_hits);
    }

    let mut lists: Vec<Vec<RetrievalHit>> = Vec::new();
    // Rank local hits as list 0 in current order.
    lists.push(local_hits.clone());

    let mut snapshot_candidates = 0u32;
    for source in sources.into_iter().take(MAX_SNAPSHOTS_PER_QUERY) {
        match search_snapshot_hits(request, parsed, &source, sensitivity, audience) {
            Ok(hits) if !hits.is_empty() => {
                snapshot_candidates = snapshot_candidates
                    .saturating_add(u32::try_from(hits.len()).unwrap_or(u32::MAX));
                lists.push(hits);
            }
            Ok(_) => {}
            Err(message) => warnings.push(format!(
                "snapshot {} excluded: {message}",
                source.snapshot_id
            )),
        }
    }

    if !semantic_hits.is_empty() {
        lists.push(semantic_hits);
    }

    if lists.len() == 1 {
        return FusionReport::unfused(local_hits);
    }
    let mut report = rrf_merge(&lists);
    report.snapshot_candidates = snapshot_candidates;
    report.semantic_candidates = semantic_candidates;
    report
}

#[allow(clippy::too_many_lines)]
fn search_snapshot_hits(
    request: &RetrievalQuery,
    parsed: &ParsedQuery,
    source: &crate::snapshot::SnapshotSource,
    sensitivity: &SensitivityIndex,
    audience: RetrievalAudience,
) -> Result<Vec<RetrievalHit>, String> {
    let snap = open_snapshot_readonly(&source.payload_path).map_err(|e| e.to_string())?;
    // Query snapshot FTS for this snapshot root id, then remap root to local.
    let mut sql = String::from(
        "SELECT fts.segment_id, fts.revision_id, fts.source_file_id, fts.root_id,
                fts.relative_path, fts.anchor, fts.text, s.text_hash, sf.file_type,
                bm25(segments_fts) AS rank,
                highlight(segments_fts, 0, '«', '»') AS highlighted
         FROM segments_fts AS fts
         INNER JOIN source_files AS sf ON sf.id = fts.source_file_id AND sf.state = 'active'
           AND sf.current_revision_id = fts.revision_id
         INNER JOIN segments AS s ON s.id = fts.segment_id
         WHERE segments_fts MATCH ?1 AND fts.root_id = ?2",
    );
    let mut bind: Vec<String> = vec![parsed.fts_match.clone(), source.snapshot_root_id.clone()];
    if !request.file_types.is_empty() {
        let base = bind.len() + 1;
        let placeholders = (0..request.file_types.len())
            .map(|i| format!("?{}", base + i))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(sql, " AND sf.file_type IN ({placeholders})");
        for ft in &request.file_types {
            bind.push(ft.as_str().to_owned());
        }
    }
    sql.push_str(
        " ORDER BY rank ASC, fts.relative_path ASC, fts.anchor ASC, fts.segment_id ASC LIMIT ?",
    );
    let mut statement = snap.prepare(&sql).map_err(|e| e.to_string())?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = bind
        .into_iter()
        .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    params.push(Box::new(i64::try_from(SNAPSHOT_CANDIDATES).unwrap_or(32)));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();
    let mut rows = statement
        .query(param_refs.as_slice())
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let relative = PathBuf::from(row.get::<_, String>(4).map_err(|e| e.to_string())?);
        let local_root = source.local_root_id;
        if !request.root_ids.is_empty() && !request.root_ids.contains(&local_root) {
            continue;
        }
        if !sensitivity_allowed(
            sensitivity_for_path(sensitivity, &local_root, &relative),
            audience,
            request.include_sensitive,
        ) {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let raw = row.get::<_, f64>(9).map_err(|e| e.to_string())? as f32;
        let public = public_score_from_bm25(raw);
        let text: String = row.get(6).map_err(|e| e.to_string())?;
        let highlighted: String = row.get(10).map_err(|e| e.to_string())?;
        let (text, truncated) = truncate_utf8(&text, MAX_HIT_TEXT_BYTES);
        let matched_terms = matched_terms_for(parsed, &text);
        let excerpt = excerpt_from_highlight(&highlighted, &text);
        let token_estimate =
            u32::try_from(RESULT_OVERHEAD_TOKENS + HeuristicTokenEstimator.estimate(&text))
                .unwrap_or(1);
        let file_type =
            SupportedFileType::from_str(&row.get::<_, String>(8).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let scope = sensitivity_for_path(sensitivity, &local_root, &relative);
        hits.push(RetrievalHit {
            segment_id: SegmentId::from_str(&row.get::<_, String>(0).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            revision_id: RevisionId::from_str(&row.get::<_, String>(1).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            source_file_id: SourceFileId::from_str(
                &row.get::<_, String>(2).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
            root_id: local_root,
            relative_path: relative,
            file_type,
            anchor: row.get(5).map_err(|e| e.to_string())?,
            text_hash: ContentHash(row.get(7).map_err(|e| e.to_string())?),
            text,
            score: public,
            signals: RetrievalSignals {
                channel: "snapshot_fts5".into(),
                raw_bm25: Some(raw),
                public_score: public,
                federation_score: None,
                lexical_rank: None,
                semantic_rank: None,
                cosine_similarity: None,
                fusion_score: None,
                embedding_space_id: None,
            },
            explanation: MatchExplanation {
                matched_terms,
                matched_excerpt: Some(excerpt),
                explanation_kind: ExplanationKind::LexicalTermOverlap,
            },
            freshness: crate::domain::FreshnessStatus::Unknown,
            sensitivity_scope: scope,
            token_estimate,
            truncated,
            origin: EvidenceOrigin::Snapshot {
                snapshot_id: source.snapshot_id.clone(),
                snapshot_root_id: source.snapshot_root_id.clone(),
            },
        });
    }
    for (rank, hit) in hits.iter_mut().enumerate() {
        hit.signals.lexical_rank = Some(u32::try_from(rank + 1).unwrap_or(u32::MAX));
    }
    Ok(hits)
}

fn rrf_merge(lists: &[Vec<RetrievalHit>]) -> FusionReport {
    use std::collections::HashMap;
    #[derive(Clone)]
    struct Acc {
        hit: RetrievalHit,
        score: f32,
        local: bool,
    }
    let mut best: HashMap<String, Acc> = HashMap::new();
    let mut admitted = 0usize;
    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            if admitted >= MAX_FUSION_CANDIDATES {
                break;
            }
            admitted += 1;
            let key = hit.text_hash.0.clone();
            #[allow(clippy::cast_precision_loss)]
            let rank_f = rank as f32;
            let contrib = 1.0 / (RRF_K + rank_f + 1.0);
            let is_local = matches!(hit.origin, EvidenceOrigin::LocalIndex);
            if let Some(existing) = best.get_mut(&key) {
                // Local exact duplicate wins and keeps accumulating only from local list.
                if existing.local {
                    if is_local {
                        existing.score += contrib;
                        merge_signals(&mut existing.hit, hit);
                    }
                } else if is_local {
                    let mut replaced = hit.clone();
                    replaced.signals.federation_score = Some(contrib);
                    replaced.score = contrib;
                    *existing = Acc {
                        hit: replaced,
                        score: contrib,
                        local: true,
                    };
                } else {
                    existing.score += contrib;
                }
            } else {
                let mut cloned = hit.clone();
                cloned.signals.federation_score = Some(contrib);
                cloned.score = contrib;
                best.insert(
                    key,
                    Acc {
                        hit: cloned,
                        score: contrib,
                        local: is_local,
                    },
                );
            }
        }
    }
    let mut merged: Vec<Acc> = best.into_values().collect();
    merged.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.hit.relative_path.cmp(&b.hit.relative_path))
            .then_with(|| a.hit.anchor.cmp(&b.hit.anchor))
            .then_with(|| {
                a.hit
                    .segment_id
                    .to_string()
                    .cmp(&b.hit.segment_id.to_string())
            })
    });
    let hits = merged
        .into_iter()
        .map(|mut acc| {
            acc.hit.signals.federation_score = Some(acc.score);
            acc.hit.signals.fusion_score = Some(acc.score);
            acc.hit.score = acc.score;
            acc.hit
        })
        .collect::<Vec<_>>();
    let unique = u32::try_from(hits.len()).unwrap_or(u32::MAX);
    let admitted = u32::try_from(admitted).unwrap_or(u32::MAX);
    FusionReport {
        hits,
        snapshot_candidates: 0,
        semantic_candidates: 0,
        admitted,
        unique,
        duplicates: admitted.saturating_sub(unique),
    }
}

fn merge_signals(target: &mut RetrievalHit, incoming: &RetrievalHit) {
    if target.signals.raw_bm25.is_none() {
        target.signals.raw_bm25 = incoming.signals.raw_bm25;
    }
    if target.signals.cosine_similarity.is_none() {
        target.signals.cosine_similarity = incoming.signals.cosine_similarity;
    }
    if target.signals.semantic_rank.is_none() {
        target.signals.semantic_rank = incoming.signals.semantic_rank;
    }
    if target.signals.lexical_rank.is_none() {
        target.signals.lexical_rank = incoming.signals.lexical_rank;
    }
    if target.signals.embedding_space_id.is_none() {
        target
            .signals
            .embedding_space_id
            .clone_from(&incoming.signals.embedding_space_id);
    }
    if incoming.explanation.explanation_kind == ExplanationKind::SemanticNeighbor
        && target.explanation.matched_terms.is_empty()
    {
        target.explanation = incoming.explanation.clone();
    }
}

/// Resolves CLI budget arguments into limit and token budget values.
///
/// Precedence:
/// 1. `--budget NAME` alone resolves both values from the preset;
/// 2. `--budget` combined with `--limit` or `--token-budget` is rejected;
/// 3. explicit `--limit` / `--token-budget` override defaults independently;
/// 4. otherwise configuration defaults apply.
///
/// # Errors
///
/// Returns configuration or domain validation errors.
pub fn resolve_budget_args(
    config: &AppConfig,
    budget: Option<&str>,
    limit: Option<u16>,
    token_budget: Option<u32>,
) -> Result<(RetrievalLimit, TokenBudget, Option<String>), RetrievalError> {
    if let Some(name) = budget {
        if limit.is_some() || token_budget.is_some() {
            return Err(RetrievalError::Config(crate::error::ConfigError::Invalid {
                path: PathBuf::from("cli"),
                message: "do not combine --budget with --limit or --token-budget".into(),
            }));
        }
        let preset = config.budget_preset(name)?;
        return Ok((
            RetrievalLimit::new(preset.max_results)?,
            TokenBudget::new(preset.token_budget)?,
            Some(preset.name),
        ));
    }
    let limit = RetrievalLimit::new(limit.unwrap_or(config.retrieval.default_limit))?;
    let token_budget =
        TokenBudget::new(token_budget.unwrap_or(config.retrieval.default_token_budget))?;
    Ok((limit, token_budget, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EmbeddingProviderConfig, add_root, init_installation};
    use crate::embedding::{
        EmbeddingBatch, EmbeddingError, EmbeddingProviderKind, ResolvedEmbeddingModel,
    };
    use crate::embedding_sync::synchronize_with_provider;
    use crate::index::index_roots;
    use crate::paths::AppPaths;
    use crate::storage::open_database;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn seeded() -> (TempDir, AppConfig, Connection) {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(notes.join("private")).unwrap();
        fs::write(
            notes.join("architecture.md"),
            "# Storage\n\nSQLite is the initial system of record.\n\n# Graph\n\nThe graph is derived, not canonical.\n",
        )
        .unwrap();
        fs::write(
            notes.join("copy.md"),
            "# Storage\n\nSQLite is the initial system of record.\n",
        )
        .unwrap();
        fs::write(
            notes.join("private/secret.md"),
            "# Secret\n\nSQLite credentials live here.\n",
        )
        .unwrap();
        let mut root = add_root(&mut config, &notes, Some("notes".into())).unwrap();
        root.sensitivity.push(crate::config::SensitivityConfig {
            pattern: "private/**".into(),
            scope: "require_explicit_query".into(),
        });
        if let Some(entry) = config.roots.iter_mut().find(|item| item.id == root.id) {
            *entry = root;
        }
        config.save(&paths.config_file).unwrap();
        let db = config.database_path().unwrap();
        let mut connection = open_database(&db).unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        (temp, config, connection)
    }

    struct SemanticFake {
        calls: AtomicUsize,
        digest: &'static str,
        fail: bool,
        dimensions: u32,
    }
    struct QueryTransactionProbe {
        db: PathBuf,
    }
    impl EmbeddingProvider for QueryTransactionProbe {
        fn provider_kind(&self) -> EmbeddingProviderKind {
            EmbeddingProviderKind::Ollama
        }
        fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError> {
            Ok(ResolvedEmbeddingModel {
                provider: EmbeddingProviderKind::Ollama,
                configured_name: "fixture".into(),
                canonical_name: "fixture:latest".into(),
                model_digest: "sha256:fixture".into(),
                dimensions: Some(8),
            })
        }
        fn embed(
            &self,
            inputs: &[EmbeddingInput],
            _: &ResolvedEmbeddingModel,
        ) -> Result<EmbeddingBatch, EmbeddingError> {
            assert!(
                inputs
                    .iter()
                    .all(|input| input.purpose == crate::embedding::EmbeddingInputPurpose::Query)
            );
            let probe = rusqlite::Connection::open(&self.db).unwrap();
            probe.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
            Ok(EmbeddingBatch {
                vectors: inputs
                    .iter()
                    .map(|_| EmbeddingVector {
                        values: vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    })
                    .collect(),
            })
        }
    }
    impl EmbeddingProvider for SemanticFake {
        fn provider_kind(&self) -> EmbeddingProviderKind {
            EmbeddingProviderKind::Ollama
        }
        fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError> {
            if self.fail {
                return Err(EmbeddingError::Unavailable);
            }
            Ok(ResolvedEmbeddingModel {
                provider: EmbeddingProviderKind::Ollama,
                configured_name: "fixture".into(),
                canonical_name: "fixture:latest".into(),
                model_digest: self.digest.into(),
                dimensions: Some(self.dimensions),
            })
        }
        fn embed(
            &self,
            inputs: &[EmbeddingInput],
            _: &ResolvedEmbeddingModel,
        ) -> Result<crate::embedding::EmbeddingBatch, EmbeddingError> {
            self.calls.fetch_add(inputs.len(), Ordering::SeqCst);
            if self.fail {
                return Err(EmbeddingError::Unavailable);
            }
            Ok(EmbeddingBatch {
                vectors: inputs
                    .iter()
                    .map(|input| EmbeddingVector {
                        values: if input.text.to_lowercase().contains("sqlite")
                            || input.text.contains("durable database")
                        {
                            vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                        } else {
                            vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                        },
                    })
                    .collect(),
            })
        }
    }

    fn semantic_seeded() -> (TempDir, AppConfig, Connection, SemanticFake) {
        let (temp, mut config, mut connection) = seeded();
        config.embeddings.enabled = true;
        config.embeddings.provider = EmbeddingProviderConfig::Ollama;
        config.embeddings.endpoint = "http://127.0.0.1:1".into();
        config.embeddings.model = "fixture".into();
        config.embeddings.dimensions = 8;
        let fake = SemanticFake {
            calls: AtomicUsize::new(0),
            digest: "sha256:fixture",
            fail: false,
            dimensions: 8,
        };
        synchronize_with_provider(&mut connection, &config.embeddings, &fake).unwrap();
        fake.calls.store(0, Ordering::SeqCst);
        (temp, config, connection, fake)
    }

    #[test]
    fn semantic_query_is_transient_and_ranks_by_cosine() {
        let (_temp, config, connection, fake) = semantic_seeded();
        let request = RetrievalQuery {
            query: "durable database".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Semantic,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let scanner = SqliteExactVectorSearch::default();
        let response = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&fake),
                vector_search: &scanner,
            },
        )
        .unwrap();
        assert_eq!(response.mode, RetrievalMode::Semantic);
        assert_eq!(response.score_kind, "cosine_similarity");
        assert!(
            response.results.iter().all(
                |hit| hit.signals.semantic_rank.is_some() && hit.signals.lexical_rank.is_none()
            )
        );
        assert!(response.telemetry.active_vectors_examined > 0);
        assert!(response.results[0].text.to_lowercase().contains("sqlite"));
        assert_eq!(
            response.results[0].explanation.explanation_kind,
            ExplanationKind::SemanticNeighbor
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        let query_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM embedding_vectors WHERE text_hash='transient-query'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(query_rows, 0);
    }

    #[test]
    fn hybrid_is_one_pass_and_auto_falls_back() {
        let (_temp, config, connection, fake) = semantic_seeded();
        let mut request = RetrievalQuery {
            query: "SQLite durable database".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Hybrid,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let scanner = SqliteExactVectorSearch::default();
        let response = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&fake),
                vector_search: &scanner,
            },
        )
        .unwrap();
        assert_eq!(response.mode, RetrievalMode::Hybrid);
        assert!(response.results[0].signals.fusion_score.is_some());
        assert!(
            response
                .results
                .iter()
                .any(|hit| hit.signals.raw_bm25.is_some())
        );
        assert!(
            response
                .results
                .iter()
                .any(|hit| hit.signals.cosine_similarity.is_some())
        );
        let agreed = response
            .results
            .iter()
            .find(|hit| hit.signals.raw_bm25.is_some() && hit.signals.cosine_similarity.is_some())
            .unwrap();
        assert!(agreed.signals.lexical_rank.is_some());
        assert!(agreed.signals.semantic_rank.is_some());
        assert_eq!(agreed.signals.fusion_score, Some(agreed.score));
        assert!(response.telemetry.local_lexical_candidates > 0);
        assert!(response.telemetry.semantic_candidates > 0);
        let unavailable = SemanticFake {
            calls: AtomicUsize::new(0),
            digest: "sha256:fixture",
            fail: true,
            dimensions: 8,
        };
        request.mode = RetrievalMode::Auto;
        let fallback = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&unavailable),
                vector_search: &scanner,
            },
        )
        .unwrap();
        assert_eq!(fallback.requested_mode, RetrievalMode::Auto);
        assert_eq!(fallback.mode, RetrievalMode::Lexical);
        assert_eq!(fallback.warnings.len(), 1);
    }

    #[test]
    fn changed_model_digest_requires_reindex() {
        let (_temp, config, connection, _fake) = semantic_seeded();
        let changed = SemanticFake {
            calls: AtomicUsize::new(0),
            digest: "sha256:changed",
            fail: false,
            dimensions: 8,
        };
        let request = RetrievalQuery {
            query: "database".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Semantic,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let scanner = SqliteExactVectorSearch::default();
        let error = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&changed),
                vector_search: &scanner,
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "EMBEDDING_MODEL_CHANGED",
                ..
            }
        ));
        assert_eq!(changed.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn configured_dimensions_are_checked_before_provider_calls() {
        let (_temp, mut config, connection, fake) = semantic_seeded();
        let request = RetrievalQuery {
            query: "database".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Semantic,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let scanner = SqliteExactVectorSearch::default();
        config.embeddings.dimensions = 7;
        let error = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&fake),
                vector_search: &scanner,
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "EMBEDDING_SPACE_INCOMPATIBLE",
                ..
            }
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        config.embeddings.dimensions = 0;
        assert!(
            retrieve_with_runtime(
                &connection,
                &config,
                &request,
                &RetrievalRuntime {
                    embedding_provider: Some(&fake),
                    vector_search: &scanner
                }
            )
            .is_ok()
        );
        let mismatched = SemanticFake {
            calls: AtomicUsize::new(0),
            digest: "sha256:fixture",
            fail: false,
            dimensions: 7,
        };
        let error = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&mismatched),
                vector_search: &scanner,
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "EMBEDDING_SPACE_INCOMPATIBLE",
                ..
            }
        ));
        assert_eq!(mismatched.calls.load(Ordering::SeqCst), 0);
        connection
            .execute("DELETE FROM segment_embeddings", [])
            .unwrap();
        let calls = fake.calls.load(Ordering::SeqCst);
        let error = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&fake),
                vector_search: &scanner,
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "SEMANTIC_UNAVAILABLE",
                ..
            }
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), calls);
    }

    #[test]
    fn query_provider_call_occurs_without_open_sqlite_transaction() {
        let (_temp, config, connection, _fake) = semantic_seeded();
        let probe = QueryTransactionProbe {
            db: config.database_path().unwrap(),
        };
        let request = RetrievalQuery {
            query: "database".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Semantic,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&probe),
                vector_search: &SqliteExactVectorSearch::default(),
            },
        )
        .unwrap();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn exact_scan_bounds_filters_corruption_and_does_not_mutate() {
        let (temp, config, mut connection, fake) = semantic_seeded();
        let space = load_active_space(&connection).unwrap();
        let model = fake.resolve_model().unwrap();
        let query = normalize_vector(
            &fake
                .embed(&[EmbeddingInput::query("database".into())], &model)
                .unwrap()
                .vectors[0]
                .values,
        )
        .unwrap();
        let mut request = RetrievalQuery {
            query: "database".into(),
            root_ids: Vec::new(),
            file_types: vec![SupportedFileType::Markdown],
            mode: RetrievalMode::Semantic,
            limit: RetrievalLimit::new(10).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let before = connection.total_changes();
        let report = SqliteExactVectorSearch::bounded(50_000, 1)
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap();
        assert_eq!(report.hits.len(), 1);
        assert!(
            report
                .hits
                .iter()
                .all(|hit| !hit.relative_path.starts_with("private"))
        );
        assert_eq!(connection.total_changes(), before);
        request.include_sensitive = true;
        let sensitive = SqliteExactVectorSearch::default()
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap();
        assert!(
            sensitive
                .hits
                .iter()
                .any(|hit| hit.relative_path.starts_with("private"))
        );
        for pair in sensitive
            .hits
            .windows(2)
            .filter(|pair| (pair[0].score - pair[1].score).abs() < f32::EPSILON)
        {
            assert!(pair[0].relative_path <= pair[1].relative_path);
        }
        request.include_sensitive = false;
        request.root_ids = vec![RootId::new()];
        assert!(
            SqliteExactVectorSearch::default()
                .search(
                    &connection,
                    &config,
                    &space,
                    &query,
                    &request,
                    RetrievalAudience::LocalUser,
                    10
                )
                .unwrap()
                .hits
                .is_empty()
        );
        request.root_ids.clear();
        request.file_types = vec![SupportedFileType::PlainText];
        assert!(
            SqliteExactVectorSearch::default()
                .search(
                    &connection,
                    &config,
                    &space,
                    &query,
                    &request,
                    RetrievalAudience::LocalUser,
                    10
                )
                .unwrap()
                .hits
                .is_empty()
        );
        request.file_types = vec![SupportedFileType::Markdown];
        connection
            .execute("UPDATE roots SET enabled=0", [])
            .unwrap();
        assert!(
            SqliteExactVectorSearch::default()
                .search(
                    &connection,
                    &config,
                    &space,
                    &query,
                    &request,
                    RetrievalAudience::LocalUser,
                    10
                )
                .unwrap()
                .hits
                .is_empty()
        );
        connection
            .execute("UPDATE roots SET enabled=1", [])
            .unwrap();
        fs::write(
            temp.path().join("notes/architecture.md"),
            "# Changed\n\nreplacement revision",
        )
        .unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        let superseded = SqliteExactVectorSearch::default()
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap();
        assert!(
            superseded
                .hits
                .iter()
                .all(|hit| hit.relative_path != Path::new("architecture.md"))
        );
        fs::remove_file(temp.path().join("notes/copy.md")).unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        let deleted = SqliteExactVectorSearch::default()
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap();
        assert!(
            deleted
                .hits
                .iter()
                .all(|hit| hit.relative_path != Path::new("copy.md"))
        );
        let error = SqliteExactVectorSearch::bounded(1, 10)
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "VECTOR_SCAN_LIMIT_EXCEEDED",
                ..
            }
        ));
        request.include_sensitive = true;
        let target: (String, String) = connection
            .query_row(
                "SELECT ev.embedding_space_id,ev.text_hash FROM embedding_vectors ev JOIN segment_embeddings se ON se.embedding_space_id=ev.embedding_space_id AND se.text_hash=ev.text_hash JOIN segments s ON s.id=se.segment_id JOIN source_files sf ON sf.current_revision_id=s.revision_id AND sf.state='active' LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        connection.execute("UPDATE embedding_vectors SET vector_bytes=zeroblob(dimensions*4) WHERE embedding_space_id=?1 AND text_hash=?2", rusqlite::params![target.0,target.1]).unwrap();
        let report = SqliteExactVectorSearch::default()
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap();
        assert!(report.corrupt_excluded > 0);
        let changes_before_retrieval = connection.total_changes();
        let response = retrieve_with_runtime(
            &connection,
            &config,
            &request,
            &RetrievalRuntime {
                embedding_provider: Some(&fake),
                vector_search: &SqliteExactVectorSearch::default(),
            },
        )
        .unwrap();
        assert!(
            response
                .warnings
                .iter()
                .any(|warning| warning.contains("corrupt"))
        );
        assert!(response.telemetry.corrupt_vectors_excluded > 0);
        assert_eq!(connection.total_changes(), changes_before_retrieval);
        connection.execute("UPDATE embedding_vectors SET vector_bytes=zeroblob(dimensions*4) WHERE embedding_space_id=?1", [&space.id]).unwrap();
        let error = SqliteExactVectorSearch::default()
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "VECTOR_SEARCH_FAILED",
                ..
            }
        ));
        connection.execute("UPDATE embedding_vectors SET dimensions=4,vector_bytes=zeroblob(16) WHERE embedding_space_id=?1", [&space.id]).unwrap();
        let error = SqliteExactVectorSearch::default()
            .search(
                &connection,
                &config,
                &space,
                &query,
                &request,
                RetrievalAudience::LocalUser,
                10,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            RetrievalError::Semantic {
                code: "VECTOR_SEARCH_FAILED",
                ..
            }
        ));
    }

    #[test]
    fn lexical_query_ranks_storage_hits() {
        let (_temp, config, connection) = seeded();
        let request = RetrievalQuery {
            query: "SQLite system of record".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Auto,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let response = retrieve(&connection, &config, &request).unwrap();
        assert_eq!(response.mode, RetrievalMode::Lexical);
        assert_eq!(response.results[0].signals.lexical_rank, Some(1));
        assert!(!response.results.is_empty());
        assert!(response.results[0].text.to_lowercase().contains("sqlite"));
        assert!(response.duplicates_suppressed >= 1);
        assert!(
            response
                .results
                .iter()
                .all(|hit| { !hit.relative_path.starts_with("private") })
        );
    }

    #[test]
    fn unavailable_modes_are_rejected() {
        let (_temp, config, connection) = seeded();
        let request = RetrievalQuery {
            query: "sqlite".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Semantic,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        };
        let error = retrieve(&connection, &config, &request).unwrap_err();
        assert!(matches!(error, RetrievalError::Semantic { .. }));
    }

    #[test]
    fn include_sensitive_returns_private_paths() {
        let (_temp, config, connection) = seeded();
        let request = RetrievalQuery {
            query: "credentials".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Lexical,
            limit: RetrievalLimit::new(5).unwrap(),
            token_budget: TokenBudget::new(2_000).unwrap(),
            include_sensitive: true,
            budget_preset: None,
        };
        let response = retrieve(&connection, &config, &request).unwrap();
        assert!(
            response
                .results
                .iter()
                .any(|hit| hit.relative_path.starts_with("private"))
        );
    }

    #[test]
    fn bm25_public_score_is_higher_for_lower_raw() {
        // Typical FTS5 BM25 values are negative; more negative is better.
        assert!(public_score_from_bm25(-3.5) > public_score_from_bm25(-0.5));
        assert!(public_score_from_bm25(-0.5) > public_score_from_bm25(1.0));
        assert!((public_score_from_bm25(-2.0) - 2.0).abs() < f32::EPSILON);
        assert!(public_score_from_bm25(f32::NAN).is_infinite());
        assert!(public_score_from_bm25(f32::INFINITY).is_infinite());
    }

    #[test]
    fn distinct_documents_receive_distinct_bm25_public_scores() {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(
            notes.join("focused.md"),
            "# Focused\n\nSQLite SQLite SQLite is the selected storage engine for Omni-Sem.\n",
        )
        .unwrap();
        fs::write(
            notes.join("mention.md"),
            "# Mention\n\nThe notes briefly mention SQLite once among other topics like graphs and agents.\n",
        )
        .unwrap();
        add_root(&mut config, &notes, Some("notes".into())).unwrap();
        config.save(&paths.config_file).unwrap();
        let mut connection = open_database(&config.database_path().unwrap()).unwrap();
        index_roots(&mut connection, &config, None).unwrap();

        let response = retrieve(
            &connection,
            &config,
            &RetrievalQuery {
                query: "SQLite".into(),
                root_ids: Vec::new(),
                file_types: Vec::new(),
                mode: RetrievalMode::Lexical,
                limit: RetrievalLimit::new(10).unwrap(),
                token_budget: TokenBudget::new(4_000).unwrap(),
                include_sensitive: false,
                budget_preset: None,
            },
        )
        .unwrap();
        assert!(response.results.len() >= 2);
        let first = &response.results[0];
        let second = &response.results[1];
        let first_raw = first.signals.raw_bm25.expect("raw bm25");
        let second_raw = second.signals.raw_bm25.expect("raw bm25");
        assert!(
            (first_raw - second_raw).abs() > f32::EPSILON,
            "expected distinct raw BM25 values, got {first_raw} and {second_raw}"
        );
        assert!(
            (first.score - second.score).abs() > f32::EPSILON,
            "expected distinct public scores, got {} and {}",
            first.score,
            second.score
        );
        assert!(
            first.score > second.score,
            "better-ranked hit must have higher public score"
        );
        assert!(
            first_raw < second_raw,
            "better-ranked hit must have lower raw BM25"
        );
        assert!(first_raw.is_sign_negative() || first_raw == 0.0);
    }
}
