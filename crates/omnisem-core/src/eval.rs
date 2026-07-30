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

use crate::config::{add_root, init_installation};
use crate::domain::{RetrievalLimit, RetrievalMode, RetrievalQuery, TokenBudget};
use crate::error::{IndexError, RetrievalError};
use crate::hash::blake3_hex;
use crate::index::index_roots;
use crate::paths::AppPaths;
use crate::retrieval::retrieve;
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
    let effective = match mode {
        RetrievalMode::Lexical | RetrievalMode::Auto => RetrievalMode::Lexical,
        other => {
            return Err(RetrievalError::Domain(
                crate::domain::DomainError::RetrievalModeUnavailable(other.as_str().into()),
            ));
        }
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
        let response = retrieve(&connection, &config, &request)?;
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
    let mut diversity = 0.0;
    for item in per_query {
        let judgment = &judgments[&item.query_id];
        let mut seen = BTreeSet::new();
        let mut sources = BTreeSet::new();
        for hit in &item.hits {
            duplicate_den += 1.0;
            stale_den += 1.0;
            if !seen.insert(hit.segment_ref.clone()) {
                duplicate_num += 1.0;
            }
            if hit.segment_ref.contains("old") || hit.segment_ref.contains("stale") {
                // Stale rate is approximated via fixture revision naming in the
                // reference corpus paths/document ids.
                stale_num += 1.0;
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
        misleading_result_rate: if stale_den == 0.0 {
            0.0
        } else {
            misleading_num / stale_den
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

fn index_fingerprint(connection: &Connection) -> Result<String, RetrievalError> {
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM segments_fts", [], |row| row.get(0))
        .map_err(crate::storage::StorageError::from)?;
    Ok(blake3_hex(count.to_string().as_bytes()).0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn reference_bundle_runs_deterministically() {
        let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals");
        let first = run_evaluation(&bundle, RetrievalMode::Lexical).unwrap();
        let second = run_evaluation(&bundle, RetrievalMode::Lexical).unwrap();
        assert!((first.metrics.recall_at_5 - second.metrics.recall_at_5).abs() < f64::EPSILON);
        assert!((first.metrics.mrr - second.metrics.mrr).abs() < f64::EPSILON);
        assert_eq!(first.query_count, 2);
        assert!(first.metrics.p95_latency_ms >= 0.0);
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
}
