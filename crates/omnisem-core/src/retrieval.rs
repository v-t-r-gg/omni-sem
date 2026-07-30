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
    ContentHash, ExplanationKind, MatchExplanation, RetrievalHit, RetrievalLimit, RetrievalMode,
    RetrievalQuery, RetrievalResponse, RetrievalSignals, RevisionId, RootId, SegmentId,
    SensitivityScope, SourceFileId, SupportedFileType, Timestamp, TokenBudget,
};
use crate::error::RetrievalError;
use crate::freshness::inspect_freshness;
use crate::query_parse::{ParsedQuery, parse_lexical_query};
use crate::tokens::{
    HARD_BYTE_CAP, HeuristicTokenEstimator, MAX_HIT_TEXT_BYTES, RESPONSE_OVERHEAD_TOKENS,
    RESULT_OVERHEAD_TOKENS, TokenEstimator, estimate_response_tokens, truncate_utf8,
};

/// Maximum FTS candidates fetched before packing (hard bound).
pub const MAX_CANDIDATES: usize = 200;
/// Multiplier of the final limit used when fetching candidates.
pub const CANDIDATE_MULTIPLIER: usize = 8;

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
    let started = Instant::now();
    let effective_mode = resolve_mode(request.mode)?;
    let parsed = parse_lexical_query(&request.query)?;
    let estimator = HeuristicTokenEstimator;

    let candidate_limit =
        (usize::from(request.limit.get()) * CANDIDATE_MULTIPLIER).clamp(1, MAX_CANDIDATES);
    let mut candidates = search_fts(connection, request, &parsed, candidate_limit)?;
    let mut warnings = Vec::new();

    let before_dedupe = candidates.len();
    candidates = suppress_duplicates(candidates);
    let duplicates_suppressed =
        u32::try_from(before_dedupe.saturating_sub(candidates.len())).unwrap_or(u32::MAX);

    let sensitivity = load_sensitivity_sets(config);
    candidates = filter_sensitivity(candidates, &sensitivity, request.include_sensitive);

    let mut hits = Vec::new();
    for candidate in candidates {
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
        });
    }

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
        mode: effective_mode,
        results: packed.results,
        token_estimate,
        truncated: packed.truncated,
        applied_limit: request.limit.get(),
        applied_token_budget: request.token_budget.get(),
        budget_preset: request.budget_preset.clone(),
        duplicates_suppressed,
        warnings,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

fn resolve_mode(mode: RetrievalMode) -> Result<RetrievalMode, RetrievalError> {
    match mode {
        RetrievalMode::Lexical | RetrievalMode::Auto => Ok(RetrievalMode::Lexical),
        RetrievalMode::Semantic => Err(mode_unavailable("semantic")),
        RetrievalMode::Hybrid => Err(mode_unavailable("hybrid")),
    }
}

fn mode_unavailable(name: &str) -> RetrievalError {
    RetrievalError::Domain(crate::domain::DomainError::RetrievalModeUnavailable(
        name.into(),
    ))
}

/// Public score: higher is better. Raw FTS5 BM25 is lower-is-better.
#[must_use]
pub fn public_score_from_bm25(raw_bm25: f32) -> f32 {
    if !raw_bm25.is_finite() {
        return 0.0;
    }
    1.0 / (1.0 + raw_bm25.max(0.0))
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
) -> Vec<RawCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| {
            match sensitivity_for_path(index, &candidate.root_id, &candidate.relative_path) {
                Some(SensitivityScope::RequireExplicitQuery) if !include_sensitive => false,
                // NeverReturnToMcp remains eligible for local CLI retrieval.
                _ => true,
            }
        })
        .collect()
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
    use crate::config::{add_root, init_installation};
    use crate::index::index_roots;
    use crate::paths::AppPaths;
    use crate::storage::open_database;
    use std::fs;
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
        assert!(matches!(
            error,
            RetrievalError::Domain(crate::domain::DomainError::RetrievalModeUnavailable(_))
        ));
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
        assert!(public_score_from_bm25(0.1) > public_score_from_bm25(2.0));
    }
}
