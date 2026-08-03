//! Deterministic retrieval evaluation over isolated temporary indexes.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::time::Instant;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::{EmbeddingConfig, add_root, init_installation};
use crate::embedding::EmbeddingProvider;
use crate::embedding_sync::synchronize_with_provider;
use blake3::Hasher;

use crate::domain::{FreshnessStatus, RetrievalLimit, RetrievalMode, RetrievalQuery, TokenBudget};
use crate::error::{IndexError, RetrievalError};
use crate::hash::blake3_hex;
use crate::index::index_roots;
use crate::paths::AppPaths;
use crate::retrieval::{
    RetrievalRuntime, SqliteExactVectorSearch, retrieve, retrieve_with_runtime,
};
use crate::storage::open_database;

/// One corpus document record from `corpus.jsonl`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorpusRecord {
    pub document_id: String,
    pub path: String,
    pub revision: String,
    pub segments: Vec<CorpusSegment>,
}

/// Segment fixture inside a corpus document.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorpusSegment {
    pub anchor: String,
    pub text: String,
}

/// One evaluation query.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryRecord {
    pub query_id: String,
    pub query: String,
    #[serde(default)]
    pub relationship_should_help: bool,
    #[serde(default)]
    pub newest_revision_matters: bool,
}

/// Judgment labels for one query.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgmentRecord {
    pub query_id: String,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub acceptable: Vec<String>,
    #[serde(default)]
    pub misleading: Vec<String>,
    #[serde(default)]
    pub expected_source_diversity: Option<u32>,
}

/// Aggregate evaluation report.
#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub run_id: String,
    pub mode: String,
    pub corpus_size: usize,
    pub query_count: usize,
    pub config_fingerprint: String,
    pub index_fingerprint: String,
    pub metrics: EvalMetrics,
    pub per_query: Vec<PerQueryResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EvalEmbedding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalEmbedding {
    pub provider: String,
    pub canonical_model: String,
    pub model_digest: String,
    pub dimensions: u32,
    pub embedding_space_id: String,
    pub corpus_embedding_ms: u64,
    pub active_vector_coverage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalComparison {
    pub lexical: EvalReport,
    pub semantic: EvalReport,
    pub hybrid: EvalReport,
    pub semantic_minus_lexical: EvalMetricDeltas,
    pub hybrid_minus_lexical: EvalMetricDeltas,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalMetricDeltas {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg: f64,
}

/// Aggregate metrics for a run.
#[derive(Debug, Clone, Serialize)]
pub struct EvalMetrics {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg: f64,
    pub duplicate_result_rate: f64,
    pub stale_result_rate: f64,
    pub misleading_result_rate: f64,
    pub source_diversity: f64,
    pub returned_tokens: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
}

/// Per-query metrics and hits.
#[derive(Debug, Clone, Serialize)]
pub struct PerQueryResult {
    pub query_id: String,
    pub latency_ms: f64,
    pub returned_tokens: u32,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub reciprocal_rank: f64,
    pub ndcg: f64,
    pub hits: Vec<EvalHit>,
}

/// One returned hit for evaluation export.
#[derive(Debug, Clone, Serialize)]
pub struct EvalHit {
    pub segment_ref: String,
    pub rank: u32,
    pub score: f32,
    /// Filesystem freshness observed for the hit at evaluation time.
    pub freshness: FreshnessStatus,
}

/// Runs evaluation against an isolated temporary installation.
///
/// # Errors
///
/// Returns evaluation-bundle or retrieval failures. Never mutates the caller's
/// ordinary index paths.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn run_evaluation(
    bundle_dir: &Path,
    mode: RetrievalMode,
) -> Result<EvalReport, RetrievalError> {
    run_evaluation_inner(bundle_dir, mode, None, None)
}

/// Runs semantic-capable evaluation in an isolated installation with an injected provider.
///
/// # Errors
/// Returns bundle, indexing, provider, compatibility, or retrieval failures.
pub fn run_evaluation_with_provider(
    bundle_dir: &Path,
    mode: RetrievalMode,
    embedding: &EmbeddingConfig,
    provider: &dyn EmbeddingProvider,
) -> Result<EvalReport, RetrievalError> {
    run_evaluation_inner(bundle_dir, mode, Some(embedding), Some(provider))
}

/// Compares all retrieval modes with deterministic provider wiring.
///
/// # Errors
/// Returns when any isolated mode run cannot be completed.
pub fn compare_evaluation_with_provider(
    bundle_dir: &Path,
    embedding: &EmbeddingConfig,
    provider: &dyn EmbeddingProvider,
) -> Result<EvalComparison, RetrievalError> {
    let lexical = run_evaluation(bundle_dir, RetrievalMode::Lexical)?;
    let semantic =
        run_evaluation_with_provider(bundle_dir, RetrievalMode::Semantic, embedding, provider)?;
    let hybrid =
        run_evaluation_with_provider(bundle_dir, RetrievalMode::Hybrid, embedding, provider)?;
    let deltas = |candidate: &EvalReport| EvalMetricDeltas {
        recall_at_5: candidate.metrics.recall_at_5 - lexical.metrics.recall_at_5,
        recall_at_10: candidate.metrics.recall_at_10 - lexical.metrics.recall_at_10,
        mrr: candidate.metrics.mrr - lexical.metrics.mrr,
        ndcg: candidate.metrics.ndcg - lexical.metrics.ndcg,
    };
    let semantic_minus_lexical = deltas(&semantic);
    let hybrid_minus_lexical = deltas(&hybrid);
    Ok(EvalComparison {
        lexical,
        semantic,
        hybrid,
        semantic_minus_lexical,
        hybrid_minus_lexical,
    })
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn run_evaluation_inner(
    bundle_dir: &Path,
    mode: RetrievalMode,
    embedding: Option<&EmbeddingConfig>,
    provider: Option<&dyn EmbeddingProvider>,
) -> Result<EvalReport, RetrievalError> {
    let effective = match mode {
        RetrievalMode::Lexical | RetrievalMode::Auto => RetrievalMode::Lexical,
        other => other,
    };

    let corpus = read_jsonl::<CorpusRecord>(&bundle_dir.join("corpus.jsonl"))?;
    let queries = read_jsonl::<QueryRecord>(&bundle_dir.join("queries.jsonl"))?;
    let judgments = read_jsonl::<JudgmentRecord>(&bundle_dir.join("judgments.jsonl"))?;
    validate_bundle(&corpus, &queries, &judgments)?;

    let temp = tempfile::TempDir::new()
        .map_err(|error| RetrievalError::Evaluation(format!("temp dir failed: {error}")))?;
    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths)?;
    let root_dir = temp.path().join("corpus");
    materialize_corpus(&root_dir, &corpus)?;
    add_root(&mut config, &root_dir, Some("eval".into()))?;
    config.save(&paths.config_file)?;
    let db_path = config.database_path()?;
    let mut connection = open_database(&db_path)?;
    index_roots(&mut connection, &config, None).map_err(|error| match error {
        IndexError::Config(error) => RetrievalError::Config(error),
        IndexError::Storage(error) => RetrievalError::Storage(error),
        IndexError::Domain(error) => RetrievalError::Domain(error),
        other => RetrievalError::Evaluation(other.to_string()),
    })?;
    let embedding_report = if effective == RetrievalMode::Lexical {
        None
    } else {
        let embedding = embedding.ok_or_else(|| {
            RetrievalError::Evaluation(
                "semantic evaluation requires embedding configuration".into(),
            )
        })?;
        let provider = provider.ok_or_else(|| {
            RetrievalError::Evaluation("semantic evaluation requires an embedding provider".into())
        })?;
        config.embeddings = embedding.clone();
        let started = Instant::now();
        let report = synchronize_with_provider(&mut connection, embedding, provider)
            .map_err(|error| RetrievalError::Evaluation(error.to_string()))?;
        let model = provider
            .resolve_model()
            .map_err(|error| RetrievalError::Evaluation(error.to_string()))?;
        Some(EvalEmbedding {
            provider: model.provider.to_string(),
            canonical_model: model.canonical_name,
            model_digest: model.model_digest,
            dimensions: report.dimensions.unwrap_or(0),
            embedding_space_id: report.embedding_space.unwrap_or_default(),
            corpus_embedding_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            active_vector_coverage: if report.active_segments == 0 {
                0.0
            } else {
                f64::from(report.linked_segments) / f64::from(report.active_segments)
            },
        })
    };

    let path_to_doc = corpus
        .iter()
        .map(|item| (item.path.clone(), item.document_id.clone()))
        .collect::<HashMap<_, _>>();
    let judgment_map = judgments
        .iter()
        .map(|item| (item.query_id.clone(), item.clone()))
        .collect::<HashMap<_, _>>();

    let mut per_query = Vec::new();
    let mut latencies = Vec::new();
    for query in &queries {
        let judgment = judgment_map.get(&query.query_id).ok_or_else(|| {
            RetrievalError::Evaluation(format!("missing judgment {}", query.query_id))
        })?;
        let request = RetrievalQuery {
            query: query.query.clone(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: effective,
            limit: RetrievalLimit::new(10)?,
            token_budget: TokenBudget::new(4_000)?,
            include_sensitive: true,
            budget_preset: None,
        };
        let started = Instant::now();
        let response = if effective == RetrievalMode::Lexical {
            retrieve(&connection, &config, &request)?
        } else {
            let scanner = SqliteExactVectorSearch;
            retrieve_with_runtime(
                &connection,
                &config,
                &request,
                &RetrievalRuntime {
                    embedding_provider: provider,
                    vector_search: &scanner,
                },
            )?
        };
        let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
        latencies.push(latency_ms);

        let refs = response
            .results
            .iter()
            .map(|hit| {
                let path = hit.relative_path.to_string_lossy().replace('\\', "/");
                let doc = path_to_doc
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| path.clone());
                format!("{doc}#{}", hit.anchor)
            })
            .collect::<Vec<_>>();
        let grades = relevance_grades(judgment);
        let recall_at_5 = recall_at(&refs, judgment, 5);
        let recall_at_10 = recall_at(&refs, judgment, 10);
        let reciprocal_rank = mrr_one(&refs, judgment);
        let ndcg = ndcg_at(&refs, &grades, 10);
        let hits = response
            .results
            .iter()
            .enumerate()
            .map(|(index, hit)| EvalHit {
                segment_ref: refs[index].clone(),
                rank: u32::try_from(index + 1).unwrap_or(u32::MAX),
                score: hit.score,
                freshness: hit.freshness,
            })
            .collect();
        per_query.push(PerQueryResult {
            query_id: query.query_id.clone(),
            latency_ms,
            returned_tokens: response.token_estimate,
            recall_at_5,
            recall_at_10,
            reciprocal_rank,
            ndcg,
            hits,
        });
    }

    let metrics = aggregate_metrics(&per_query, &judgment_map, &latencies);
    let config_fingerprint = blake3_hex(
        serde_json::to_string(&config.retrieval)
            .unwrap_or_default()
            .as_bytes(),
    )
    .0;
    let index_fingerprint = index_fingerprint(&connection)?;

    Ok(EvalReport {
        run_id: uuid::Uuid::new_v4().to_string(),
        mode: effective.as_str().into(),
        corpus_size: corpus.len(),
        query_count: queries.len(),
        config_fingerprint,
        index_fingerprint,
        metrics,
        per_query,
        embedding: embedding_report,
    })
}

fn materialize_corpus(root: &Path, corpus: &[CorpusRecord]) -> Result<(), RetrievalError> {
    for document in corpus {
        let path = root.join(&document.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                RetrievalError::Evaluation(format!("corpus write failed: {error}"))
            })?;
        }
        let mut body = String::new();
        for segment in &document.segments {
            // Reconstruct minimal markdown-ish content preserving anchors via headings.
            if let Some(title) = segment.anchor.strip_prefix("heading:") {
                body.push_str("# ");
                body.push_str(&title.replace('/', " / "));
                body.push_str("\n\n");
            }
            body.push_str(&segment.text);
            body.push_str("\n\n");
        }
        fs::write(&path, body)
            .map_err(|error| RetrievalError::Evaluation(format!("corpus write failed: {error}")))?;
    }
    Ok(())
}

fn validate_bundle(
    corpus: &[CorpusRecord],
    queries: &[QueryRecord],
    judgments: &[JudgmentRecord],
) -> Result<(), RetrievalError> {
    if corpus.is_empty() {
        return Err(RetrievalError::Evaluation("corpus is empty".into()));
    }
    if queries.is_empty() {
        return Err(RetrievalError::Evaluation("queries are empty".into()));
    }
    if judgments.is_empty() {
        return Err(RetrievalError::Evaluation("judgments are empty".into()));
    }
    let mut query_ids = BTreeSet::new();
    for query in queries {
        if !query_ids.insert(query.query_id.clone()) {
            return Err(RetrievalError::Evaluation(format!(
                "duplicate query_id {}",
                query.query_id
            )));
        }
    }
    for judgment in judgments {
        if !query_ids.contains(&judgment.query_id) {
            return Err(RetrievalError::Evaluation(format!(
                "judgment for unknown query_id {}",
                judgment.query_id
            )));
        }
        if judgment.required.is_empty() && judgment.acceptable.is_empty() {
            return Err(RetrievalError::Evaluation(format!(
                "judgment {} has no positive labels",
                judgment.query_id
            )));
        }
    }
    Ok(())
}

fn relevance_grades(judgment: &JudgmentRecord) -> HashMap<String, f64> {
    let mut grades = HashMap::new();
    for item in &judgment.required {
        grades.insert(item.clone(), 2.0);
    }
    for item in &judgment.acceptable {
        grades.entry(item.clone()).or_insert(1.0);
    }
    // Misleading labels are tracked separately and contribute zero nDCG gain.
    for item in &judgment.misleading {
        grades.entry(item.clone()).or_insert(0.0);
    }
    grades
}

fn recall_at(refs: &[String], judgment: &JudgmentRecord, k: usize) -> f64 {
    let required = judgment.required.iter().cloned().collect::<BTreeSet<_>>();
    if required.is_empty() {
        return 1.0;
    }
    let found = refs
        .iter()
        .take(k)
        .filter(|item| required.contains(*item))
        .count();
    found as f64 / required.len() as f64
}

fn mrr_one(refs: &[String], judgment: &JudgmentRecord) -> f64 {
    let positive = judgment
        .required
        .iter()
        .chain(judgment.acceptable.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    for (index, item) in refs.iter().enumerate() {
        if positive.contains(item) {
            return 1.0 / (index as f64 + 1.0);
        }
    }
    0.0
}

fn ndcg_at(refs: &[String], grades: &HashMap<String, f64>, k: usize) -> f64 {
    let mut dcg = 0.0;
    for (index, item) in refs.iter().take(k).enumerate() {
        let gain = grades.get(item).copied().unwrap_or(0.0);
        dcg += (2.0_f64.powf(gain) - 1.0) / ((index as f64 + 2.0).log2());
    }
    let mut ideal = grades
        .values()
        .copied()
        .filter(|value| *value > 0.0)
        .collect::<Vec<_>>();
    ideal.sort_by(|left, right| right.partial_cmp(left).unwrap_or(std::cmp::Ordering::Equal));
    let mut idcg = 0.0;
    for (index, gain) in ideal.into_iter().take(k).enumerate() {
        idcg += (2.0_f64.powf(gain) - 1.0) / ((index as f64 + 2.0).log2());
    }
    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

#[allow(clippy::cast_precision_loss)]
fn aggregate_metrics(
    per_query: &[PerQueryResult],
    judgments: &HashMap<String, JudgmentRecord>,
    latencies: &[f64],
) -> EvalMetrics {
    let n = per_query.len().max(1) as f64;
    let recall_at_5 = per_query.iter().map(|item| item.recall_at_5).sum::<f64>() / n;
    let recall_at_10 = per_query.iter().map(|item| item.recall_at_10).sum::<f64>() / n;
    let mrr = per_query
        .iter()
        .map(|item| item.reciprocal_rank)
        .sum::<f64>()
        / n;
    let ndcg = per_query.iter().map(|item| item.ndcg).sum::<f64>() / n;
    let returned_tokens = per_query
        .iter()
        .map(|item| f64::from(item.returned_tokens))
        .sum::<f64>()
        / n;

    let mut duplicate_num = 0.0;
    let mut duplicate_den = 0.0;
    let mut stale_num = 0.0;
    let mut stale_den = 0.0;
    let mut misleading_num = 0.0;
    let mut misleading_den = 0.0;
    let mut diversity = 0.0;
    for item in per_query {
        let judgment = &judgments[&item.query_id];
        let mut seen = BTreeSet::new();
        let mut sources = BTreeSet::new();
        for hit in &item.hits {
            duplicate_den += 1.0;
            misleading_den += 1.0;
            if !seen.insert(hit.segment_ref.clone()) {
                duplicate_num += 1.0;
            }
            // Stale-result rate uses filesystem freshness only.
            // Unknown is excluded from the denominator: it is indeterminate, not proven stale.
            match hit.freshness {
                FreshnessStatus::PendingReindex => {
                    stale_num += 1.0;
                    stale_den += 1.0;
                }
                FreshnessStatus::Current => {
                    stale_den += 1.0;
                }
                FreshnessStatus::Unknown => {}
            }
            if judgment
                .misleading
                .iter()
                .any(|label| label == &hit.segment_ref)
            {
                misleading_num += 1.0;
            }
            if let Some((doc, _)) = hit.segment_ref.split_once('#') {
                sources.insert(doc.to_owned());
            }
        }
        diversity += sources.len() as f64;
    }

    EvalMetrics {
        recall_at_5,
        recall_at_10,
        mrr,
        ndcg,
        duplicate_result_rate: if duplicate_den == 0.0 {
            0.0
        } else {
            duplicate_num / duplicate_den
        },
        stale_result_rate: if stale_den == 0.0 {
            0.0
        } else {
            stale_num / stale_den
        },
        misleading_result_rate: if misleading_den == 0.0 {
            0.0
        } else {
            misleading_num / misleading_den
        },
        source_diversity: diversity / n,
        returned_tokens,
        p50_latency_ms: percentile(latencies, 0.50),
        p95_latency_ms: percentile(latencies, 0.95),
    }
}

#[allow(clippy::cast_precision_loss)]
fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p * (sorted.len() as f64 - 1.0);
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;
    if low == high {
        sorted[low]
    } else {
        let weight = rank - low as f64;
        sorted[low] * (1.0 - weight) + sorted[high] * weight
    }
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, RetrievalError> {
    let text = fs::read_to_string(path).map_err(|error| {
        RetrievalError::Evaluation(format!("failed to read {}: {error}", path.display()))
    })?;
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<T>(line).map_err(|error| {
            RetrievalError::Evaluation(format!(
                "invalid JSONL in {} line {}: {error}",
                path.display(),
                index + 1
            ))
        })?;
        out.push(value);
    }
    Ok(out)
}

/// Computes a fingerprint of the **active** index surface.
///
/// Streams a deterministic ordered projection of content-stable identity fields:
/// `relative_path | anchor | ordinal | text_hash | content_hash | parser_id | parser_version`
///
/// Random row UUIDs are intentionally excluded so equivalent corpora fingerprint
/// identically across re-indexes. Historical non-current revisions do not contribute.
/// Raw segment text is not hashed.
///
/// # Errors
///
/// Returns storage errors when the active index cannot be read.
pub fn index_fingerprint(connection: &Connection) -> Result<String, RetrievalError> {
    let mut statement = connection
        .prepare(
            "SELECT
                sf.relative_path,
                s.anchor,
                s.ordinal,
                s.text_hash,
                rev.content_hash,
                rev.parser_id,
                rev.parser_version
             FROM segments_fts AS fts
             INNER JOIN source_files AS sf
                ON sf.id = fts.source_file_id
               AND sf.state = 'active'
               AND sf.current_revision_id = fts.revision_id
             INNER JOIN segments AS s
                ON s.id = fts.segment_id
             INNER JOIN revisions AS rev
                ON rev.id = fts.revision_id
             ORDER BY
                sf.relative_path ASC,
                s.ordinal ASC,
                s.anchor ASC,
                s.text_hash ASC",
        )
        .map_err(crate::storage::StorageError::from)?;
    let mut rows = statement
        .query([])
        .map_err(crate::storage::StorageError::from)?;
    let mut hasher = Hasher::new();
    let mut count = 0_u64;
    while let Some(row) = rows.next().map_err(crate::storage::StorageError::from)? {
        let relative_path: String = row.get(0).map_err(crate::storage::StorageError::from)?;
        let anchor: String = row.get(1).map_err(crate::storage::StorageError::from)?;
        let ordinal: i64 = row.get(2).map_err(crate::storage::StorageError::from)?;
        let text_hash: String = row.get(3).map_err(crate::storage::StorageError::from)?;
        let content_hash: String = row.get(4).map_err(crate::storage::StorageError::from)?;
        let parser_id: String = row.get(5).map_err(crate::storage::StorageError::from)?;
        let parser_version: String = row.get(6).map_err(crate::storage::StorageError::from)?;
        hasher.update(relative_path.as_bytes());
        hasher.update(&[0]);
        hasher.update(anchor.as_bytes());
        hasher.update(&[0]);
        hasher.update(&ordinal.to_le_bytes());
        hasher.update(&[0]);
        hasher.update(text_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(content_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(parser_id.as_bytes());
        hasher.update(&[0]);
        hasher.update(parser_version.as_bytes());
        hasher.update(&[0xff]);
        count = count.saturating_add(1);
    }
    hasher.update(&count.to_le_bytes());
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{add_root, init_installation};
    use crate::domain::{
        FreshnessStatus, RetrievalLimit, RetrievalMode, RetrievalQuery, TokenBudget,
    };
    use crate::embedding::{
        EmbeddingBatch, EmbeddingError, EmbeddingInput, EmbeddingProviderKind, EmbeddingVector,
        ResolvedEmbeddingModel,
    };
    use crate::index::index_roots;
    use crate::paths::AppPaths;
    use crate::retrieval::retrieve;
    use crate::storage::open_database;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    struct EvalFake;
    impl EmbeddingProvider for EvalFake {
        fn provider_kind(&self) -> EmbeddingProviderKind {
            EmbeddingProviderKind::Ollama
        }
        fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError> {
            Ok(ResolvedEmbeddingModel {
                provider: EmbeddingProviderKind::Ollama,
                configured_name: "eval".into(),
                canonical_name: "eval:latest".into(),
                model_digest: "sha256:eval".into(),
                dimensions: Some(8),
            })
        }
        fn embed(
            &self,
            inputs: &[EmbeddingInput],
            _: &ResolvedEmbeddingModel,
        ) -> Result<EmbeddingBatch, EmbeddingError> {
            Ok(EmbeddingBatch {
                vectors: inputs
                    .iter()
                    .map(|input| {
                        let mut values = vec![0.0; 8];
                        for byte in input.text.bytes() {
                            values[usize::from(byte) % 8] += 1.0;
                        }
                        EmbeddingVector { values }
                    })
                    .collect(),
            })
        }
    }

    fn eval_embedding() -> EmbeddingConfig {
        EmbeddingConfig {
            enabled: true,
            provider: crate::config::EmbeddingProviderConfig::Ollama,
            endpoint: "http://127.0.0.1:1".into(),
            model: "eval".into(),
            batch_size: 16,
            request_timeout_seconds: 1,
            keep_alive: "5m".into(),
            truncate: false,
            dimensions: 8,
        }
    }

    #[test]
    fn semantic_and_comparison_evaluation_are_isolated() {
        let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals");
        let semantic = run_evaluation_with_provider(
            &bundle,
            RetrievalMode::Semantic,
            &eval_embedding(),
            &EvalFake,
        )
        .unwrap();
        assert_eq!(semantic.mode, "semantic");
        assert!(semantic.embedding.is_some());
        let comparison =
            compare_evaluation_with_provider(&bundle, &eval_embedding(), &EvalFake).unwrap();
        assert_eq!(comparison.lexical.mode, "lexical");
        assert_eq!(comparison.hybrid.mode, "hybrid");
    }

    #[test]
    fn reference_bundle_runs_deterministically() {
        let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals");
        let first = run_evaluation(&bundle, RetrievalMode::Lexical).unwrap();
        let second = run_evaluation(&bundle, RetrievalMode::Lexical).unwrap();
        assert!((first.metrics.recall_at_5 - second.metrics.recall_at_5).abs() < f64::EPSILON);
        assert!((first.metrics.mrr - second.metrics.mrr).abs() < f64::EPSILON);
        assert_eq!(first.query_count, 2);
        assert!(first.metrics.p95_latency_ms >= 0.0);
        // Freshly materialized fixtures are current; Unknown is excluded from the denominator.
        assert!((first.metrics.stale_result_rate - 0.0).abs() < f64::EPSILON);
        assert_eq!(first.index_fingerprint, second.index_fingerprint);
    }

    #[test]
    fn metric_helpers_basic() {
        let judgment = JudgmentRecord {
            query_id: "q".into(),
            required: vec!["a#1".into()],
            acceptable: vec!["b#1".into()],
            misleading: vec!["c#1".into()],
            expected_source_diversity: Some(2),
        };
        let refs = vec!["c#1".into(), "a#1".into(), "b#1".into()];
        assert!((recall_at(&refs, &judgment, 5) - 1.0).abs() < f64::EPSILON);
        assert!((mrr_one(&refs, &judgment) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn stale_rate_uses_freshness_not_path_names() {
        let hits = vec![
            EvalHit {
                segment_ref: "architecture-old#heading:storage".into(),
                rank: 1,
                score: 1.0,
                freshness: FreshnessStatus::Current,
            },
            EvalHit {
                segment_ref: "architecture-current#heading:storage".into(),
                rank: 2,
                score: 0.5,
                freshness: FreshnessStatus::PendingReindex,
            },
            EvalHit {
                segment_ref: "missing#heading".into(),
                rank: 3,
                score: 0.1,
                freshness: FreshnessStatus::Unknown,
            },
        ];
        let per_query = vec![PerQueryResult {
            query_id: "q".into(),
            latency_ms: 1.0,
            returned_tokens: 10,
            recall_at_5: 1.0,
            recall_at_10: 1.0,
            reciprocal_rank: 1.0,
            ndcg: 1.0,
            hits,
        }];
        let judgments = HashMap::from([(
            "q".into(),
            JudgmentRecord {
                query_id: "q".into(),
                required: vec!["architecture-current#heading:storage".into()],
                acceptable: Vec::new(),
                misleading: Vec::new(),
                expected_source_diversity: None,
            },
        )]);
        let metrics = aggregate_metrics(&per_query, &judgments, &[1.0]);
        // Only Current + PendingReindex count; one of two is pending.
        assert!((metrics.stale_result_rate - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn stale_rate_detects_post_index_filesystem_change() {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        let file = notes.join("doc.md");
        fs::write(&file, "# Storage\n\nSQLite is the system of record.\n").unwrap();
        add_root(&mut config, &notes, Some("notes".into())).unwrap();
        config.save(&paths.config_file).unwrap();
        let mut connection = open_database(&config.database_path().unwrap()).unwrap();
        index_roots(&mut connection, &config, None).unwrap();

        thread::sleep(Duration::from_millis(1_100));
        let mut handle = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&file)
            .unwrap();
        handle
            .write_all(b"# Storage\n\nSQLite is the system of record (edited).\n")
            .unwrap();
        handle.flush().unwrap();

        let response = retrieve(
            &connection,
            &config,
            &RetrievalQuery {
                query: "SQLite".into(),
                root_ids: Vec::new(),
                file_types: Vec::new(),
                mode: RetrievalMode::Lexical,
                limit: RetrievalLimit::new(5).unwrap(),
                token_budget: TokenBudget::new(2_000).unwrap(),
                include_sensitive: false,
                budget_preset: None,
            },
        )
        .unwrap();
        assert!(!response.results.is_empty());
        assert!(
            response
                .results
                .iter()
                .any(|hit| hit.freshness == FreshnessStatus::PendingReindex)
        );
    }

    #[test]
    fn index_fingerprint_changes_with_content_not_only_count() {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("a.md"), "# A\n\nAlpha content about storage.\n").unwrap();
        add_root(&mut config, &notes, Some("notes".into())).unwrap();
        config.save(&paths.config_file).unwrap();
        let mut connection = open_database(&config.database_path().unwrap()).unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        let first = index_fingerprint(&connection).unwrap();
        let again = index_fingerprint(&connection).unwrap();
        assert_eq!(first, again);

        fs::write(notes.join("b.md"), "# B\n\nBeta content about graphs.\n").unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        let with_extra = index_fingerprint(&connection).unwrap();
        assert_ne!(first, with_extra);

        // Same row count, different text: rewrite a.md content and reindex.
        fs::write(
            notes.join("a.md"),
            "# A\n\nCompletely different alpha text.\n",
        )
        .unwrap();
        // remove b so count can match original if we want; instead compare rewrite keeps count
        fs::remove_file(notes.join("b.md")).unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        let rewritten = index_fingerprint(&connection).unwrap();
        assert_ne!(first, rewritten);
        assert_ne!(with_extra, rewritten);
    }
}
