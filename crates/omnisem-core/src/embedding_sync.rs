//! Active-segment embedding materialization after lexical indexing commits.

use std::collections::BTreeMap;
use std::time::Instant;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::config::{EmbeddingConfig, EmbeddingProviderConfig};
use crate::domain::{ContentHash, Timestamp};
use crate::embedding::{
    EmbeddingError, EmbeddingInput, EmbeddingProvider, EmbeddingSpace, encode_vector,
};
use crate::hash::blake3_hex;
use crate::storage::StorageError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmbeddingSyncReport {
    pub enabled: bool,
    pub provider: String,
    pub embedding_space: Option<String>,
    pub model: Option<String>,
    pub model_digest: Option<String>,
    pub dimensions: Option<u32>,
    pub active_segments: u32,
    pub cache_hits: u32,
    pub provider_inputs: u32,
    pub linked_segments: u32,
    pub missing_segments: u32,
    pub failed_segments: u32,
    pub duration_ms: u64,
    pub status: String,
    pub error_code: Option<String>,
}

#[derive(Clone)]
struct ActiveSegment {
    id: String,
    revision_id: String,
    text_hash: String,
    text: String,
}

/// Synchronizes embeddings using production provider selection.
pub fn synchronize_embeddings(
    connection: &mut Connection,
    config: &EmbeddingConfig,
) -> Result<EmbeddingSyncReport, StorageError> {
    if !config.enabled {
        return Ok(disabled_report());
    }
    match config.provider {
        EmbeddingProviderConfig::None => Ok(disabled_report()),
        EmbeddingProviderConfig::Ollama => {
            #[cfg(feature = "embeddings-ollama")]
            {
                let provider = crate::embedding::ollama::OllamaProvider::new(config)
                    .map_err(|error| StorageError::Decode(error.to_string()))?;
                synchronize_with_provider(connection, config, &provider)
            }
            #[cfg(not(feature = "embeddings-ollama"))]
            {
                Ok(failed_report("ollama", EmbeddingError::FeatureDisabled))
            }
        }
    }
}

fn disabled_report() -> EmbeddingSyncReport {
    EmbeddingSyncReport {
        enabled: false,
        provider: "none".into(),
        embedding_space: None,
        model: None,
        model_digest: None,
        dimensions: None,
        active_segments: 0,
        cache_hits: 0,
        provider_inputs: 0,
        linked_segments: 0,
        missing_segments: 0,
        failed_segments: 0,
        duration_ms: 0,
        status: "disabled".into(),
        error_code: None,
    }
}

fn failed_report(provider: &str, error: EmbeddingError) -> EmbeddingSyncReport {
    EmbeddingSyncReport {
        enabled: true,
        provider: provider.into(),
        embedding_space: None,
        model: None,
        model_digest: None,
        dimensions: None,
        active_segments: 0,
        cache_hits: 0,
        provider_inputs: 0,
        linked_segments: 0,
        missing_segments: 0,
        failed_segments: 0,
        duration_ms: 0,
        status: "failed".into(),
        error_code: Some(error.code().into()),
    }
}

/// Synchronizes using an injected provider; intended for deterministic integration tests too.
pub fn synchronize_with_provider(
    connection: &mut Connection,
    config: &EmbeddingConfig,
    provider: &dyn EmbeddingProvider,
) -> Result<EmbeddingSyncReport, StorageError> {
    let started = Instant::now();
    let started_at = Timestamp::now()
        .map_err(|e| StorageError::Decode(e.to_string()))?
        .as_millis();
    let segments = active_segments(connection)?;
    let active_count = segments.len() as u32;
    let resolved = match provider.resolve_model() {
        Ok(model) => model,
        Err(error) => {
            record_sync_run(
                connection,
                None,
                started_at,
                active_count,
                0,
                0,
                0,
                active_count,
                "failed",
                Some(error.code()),
            )?;
            let mut report = failed_report(&provider.provider_kind().to_string(), error);
            report.active_segments = active_count;
            report.missing_segments = active_count;
            report.failed_segments = active_count;
            report.duration_ms = elapsed_ms(started);
            return Ok(report);
        }
    };
    let mut by_hash = BTreeMap::<String, Vec<ActiveSegment>>::new();
    for segment in segments {
        by_hash
            .entry(segment.text_hash.clone())
            .or_default()
            .push(segment);
    }
    let existing_dimensions = find_existing_dimensions(connection, &resolved)?;
    let dimensions = resolved.dimensions.or(existing_dimensions);
    let mut space = dimensions.map(|value| EmbeddingSpace::new(&resolved, value));
    if let Some(value) = &space {
        persist_space(connection, value, config)?;
    }
    let mut cache_hits = 0_u32;
    let mut provider_inputs = 0_u32;
    let mut linked = 0_u32;
    let mut failures = 0_u32;
    let mut pending = Vec::<(String, Vec<ActiveSegment>)>::new();
    for (hash, refs) in by_hash {
        if let Some(current_space) = &space {
            if cache_exists(connection, &current_space.id, &hash)? {
                link_refs(connection, current_space, &hash, &refs)?;
                cache_hits += refs.len() as u32;
                linked += refs.len() as u32;
                continue;
            }
        }
        pending.push((hash, refs));
    }
    for chunk in pending.chunks(config.batch_size) {
        let inputs = chunk
            .iter()
            .map(|(hash, refs)| EmbeddingInput {
                text_hash: ContentHash(hash.clone()),
                text: refs[0].text.clone(),
            })
            .collect::<Vec<_>>();
        provider_inputs += inputs.len() as u32;
        let batch = match provider.embed(&inputs, &resolved) {
            Ok(batch) => batch,
            Err(error) => {
                let affected = chunk.iter().map(|(_, refs)| refs.len() as u32).sum::<u32>();
                failures += affected;
                if let Some(current_space) = &space {
                    record_failures(connection, current_space, chunk, &error)?;
                }
                if error.systemic() {
                    break;
                }
                continue;
            }
        };
        let actual_dimensions = batch
            .vectors
            .first()
            .map(|v| v.values.len() as u32)
            .unwrap_or(0);
        if space.is_none() {
            let new_space = EmbeddingSpace::new(&resolved, actual_dimensions);
            persist_space(connection, &new_space, config)?;
            space = Some(new_space);
        }
        let current_space = space.as_ref().expect("space established");
        if actual_dimensions != current_space.dimensions || batch.vectors.len() != chunk.len() {
            let error = EmbeddingError::DimensionMismatch {
                expected: current_space.dimensions,
                actual: actual_dimensions,
            };
            failures += chunk.iter().map(|(_, refs)| refs.len() as u32).sum::<u32>();
            record_failures(connection, current_space, chunk, &error)?;
            continue;
        }
        let transaction = connection.transaction()?;
        for ((hash, refs), vector) in chunk.iter().zip(&batch.vectors) {
            let bytes = encode_vector(vector, current_space.dimensions)
                .map_err(|error| StorageError::Decode(error.to_string()))?;
            transaction.execute("INSERT OR IGNORE INTO embedding_vectors(embedding_space_id,text_hash,vector_bytes,dimensions,created_at_ms) VALUES(?1,?2,?3,?4,?5)", params![current_space.id, hash, bytes, current_space.dimensions, started_at])?;
            for segment in refs {
                transaction.execute("INSERT OR REPLACE INTO segment_embeddings(segment_id,revision_id,embedding_space_id,text_hash,linked_at_ms) VALUES(?1,?2,?3,?4,?5)", params![segment.id, segment.revision_id, current_space.id, hash, started_at])?;
                transaction.execute(
                    "DELETE FROM embedding_failures WHERE segment_id=?1 AND embedding_space_id=?2",
                    params![segment.id, current_space.id],
                )?;
                linked += 1;
            }
        }
        transaction.commit()?;
    }
    if let Some(current_space) = &space {
        connection.execute("INSERT INTO embedding_state(singleton,active_embedding_space_id) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET active_embedding_space_id=excluded.active_embedding_space_id", [&current_space.id])?;
    }
    let missing = active_count.saturating_sub(linked);
    let status = if failures == 0 && missing == 0 {
        "completed"
    } else {
        "partial"
    };
    let error_code = (failures > 0).then_some("EMBEDDING_FAILED".to_owned());
    record_sync_run(
        connection,
        space.as_ref().map(|s| s.id.as_str()),
        started_at,
        active_count,
        cache_hits,
        provider_inputs,
        linked,
        failures,
        status,
        error_code.as_deref(),
    )?;
    Ok(EmbeddingSyncReport {
        enabled: true,
        provider: provider.provider_kind().to_string(),
        embedding_space: space.as_ref().map(|s| s.id.clone()),
        model: Some(resolved.canonical_name),
        model_digest: Some(resolved.model_digest),
        dimensions: space.as_ref().map(|s| s.dimensions),
        active_segments: active_count,
        cache_hits,
        provider_inputs,
        linked_segments: linked,
        missing_segments: missing,
        failed_segments: failures,
        duration_ms: elapsed_ms(started),
        status: status.into(),
        error_code,
    })
}

fn active_segments(connection: &Connection) -> Result<Vec<ActiveSegment>, StorageError> {
    let mut stmt = connection.prepare("SELECT s.id,s.revision_id,s.text_hash,s.text FROM segments s JOIN source_files f ON f.current_revision_id=s.revision_id WHERE f.state='active' ORDER BY s.text_hash,s.id")?;
    let rows = stmt.query_map([], |row| {
        Ok(ActiveSegment {
            id: row.get(0)?,
            revision_id: row.get(1)?,
            text_hash: row.get(2)?,
            text: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}
fn find_existing_dimensions(
    connection: &Connection,
    model: &crate::embedding::ResolvedEmbeddingModel,
) -> Result<Option<u32>, StorageError> {
    connection.query_row("SELECT dimensions FROM embedding_spaces WHERE provider=?1 AND canonical_model=?2 AND model_digest=?3 AND normalization='l2' AND input_contract_version=?4 ORDER BY created_at_ms DESC LIMIT 1", params![model.provider.to_string(), model.canonical_name, model.model_digest, crate::embedding::EMBEDDING_INPUT_CONTRACT_VERSION], |row| row.get(0)).optional().map_err(StorageError::from)
}
fn persist_space(
    connection: &Connection,
    space: &EmbeddingSpace,
    config: &EmbeddingConfig,
) -> Result<(), StorageError> {
    let now = Timestamp::now()
        .map_err(|e| StorageError::Decode(e.to_string()))?
        .as_millis();
    let fingerprint = blake3_hex(
        serde_json::to_string(config)
            .map_err(|e| StorageError::Decode(e.to_string()))?
            .as_bytes(),
    )
    .0;
    connection.execute("INSERT OR IGNORE INTO embedding_spaces(id,provider,canonical_model,model_digest,dimensions,normalization,input_contract_version,created_at_ms,config_fingerprint,provider_metadata_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'{}')", params![space.id, space.provider.to_string(), space.canonical_model, space.model_digest, space.dimensions, space.normalization.to_string(), space.input_contract_version, now, fingerprint])?;
    Ok(())
}
fn cache_exists(connection: &Connection, space_id: &str, hash: &str) -> Result<bool, StorageError> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM embedding_vectors WHERE embedding_space_id=?1 AND text_hash=?2",
            params![space_id, hash],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
fn link_refs(
    connection: &mut Connection,
    space: &EmbeddingSpace,
    hash: &str,
    refs: &[ActiveSegment],
) -> Result<(), StorageError> {
    let now = Timestamp::now()
        .map_err(|e| StorageError::Decode(e.to_string()))?
        .as_millis();
    let tx = connection.transaction()?;
    for s in refs {
        tx.execute("INSERT OR REPLACE INTO segment_embeddings(segment_id,revision_id,embedding_space_id,text_hash,linked_at_ms) VALUES(?1,?2,?3,?4,?5)",params![s.id,s.revision_id,space.id,hash,now])?;
        tx.execute(
            "DELETE FROM embedding_failures WHERE segment_id=?1 AND embedding_space_id=?2",
            params![s.id, space.id],
        )?;
    }
    tx.commit()?;
    Ok(())
}
fn record_failures(
    connection: &mut Connection,
    space: &EmbeddingSpace,
    chunk: &[(String, Vec<ActiveSegment>)],
    error: &EmbeddingError,
) -> Result<(), StorageError> {
    let now = Timestamp::now()
        .map_err(|e| StorageError::Decode(e.to_string()))?
        .as_millis();
    let safe = error.to_string().chars().take(512).collect::<String>();
    let tx = connection.transaction()?;
    for (_, refs) in chunk {
        for s in refs {
            tx.execute("INSERT INTO embedding_failures(segment_id,embedding_space_id,attempted_at_ms,error_code,safe_message,retry_count) VALUES(?1,?2,?3,?4,?5,1) ON CONFLICT(segment_id,embedding_space_id) DO UPDATE SET attempted_at_ms=excluded.attempted_at_ms,error_code=excluded.error_code,safe_message=excluded.safe_message,retry_count=min(1000000,embedding_failures.retry_count+1)",params![s.id,space.id,now,error.code(),safe])?;
        }
    }
    tx.commit()?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn record_sync_run(
    connection: &Connection,
    space: Option<&str>,
    started: i64,
    attempted: u32,
    hits: u32,
    inputs: u32,
    linked: u32,
    failures: u32,
    status: &str,
    category: Option<&str>,
) -> Result<(), StorageError> {
    let done = Timestamp::now()
        .map_err(|e| StorageError::Decode(e.to_string()))?
        .as_millis();
    connection.execute("INSERT INTO embedding_sync_runs(embedding_space_id,started_at_ms,completed_at_ms,attempted_segments,cache_hits,provider_inputs,linked_segments,failures,status,failure_category) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![space,started,done,attempted,hits,inputs,linked,failures,status,category])?;
    connection.execute("DELETE FROM embedding_sync_runs WHERE id NOT IN (SELECT id FROM embedding_sync_runs ORDER BY id DESC LIMIT 100)",[])?;
    Ok(())
}
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, EmbeddingProviderConfig, add_root};
    use crate::embedding::{
        EmbeddingBatch, EmbeddingProviderKind, EmbeddingVector, ResolvedEmbeddingModel,
    };
    use crate::index::index_roots;
    use crate::paths::AppPaths;
    use crate::storage::{open_database, status_snapshot};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    struct Fake {
        inputs: AtomicUsize,
    }
    impl EmbeddingProvider for Fake {
        fn provider_kind(&self) -> EmbeddingProviderKind {
            EmbeddingProviderKind::Ollama
        }
        fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError> {
            Ok(ResolvedEmbeddingModel {
                provider: EmbeddingProviderKind::Ollama,
                configured_name: "test".into(),
                canonical_name: "test:latest".into(),
                model_digest: "sha256:fixture".into(),
                dimensions: Some(8),
            })
        }
        fn embed(
            &self,
            inputs: &[EmbeddingInput],
            _: &ResolvedEmbeddingModel,
        ) -> Result<EmbeddingBatch, EmbeddingError> {
            self.inputs.fetch_add(inputs.len(), Ordering::SeqCst);
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

    #[test]
    fn backfills_unchanged_segments_and_reuses_duplicate_cache() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("notes");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.txt"), "identical text").unwrap();
        std::fs::write(root.join("b.txt"), "identical text").unwrap();
        let paths = AppPaths::for_base(temp.path().join("state"));
        let mut config = AppConfig::default_for(&paths);
        add_root(&mut config, &root, Some("notes".into())).unwrap();
        let db = config.database_path().unwrap();
        let mut connection = open_database(&db).unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        config.embeddings.enabled = true;
        config.embeddings.provider = EmbeddingProviderConfig::Ollama;
        config.embeddings.endpoint = "http://127.0.0.1:1".into();
        config.embeddings.model = "test".into();
        config.embeddings.dimensions = 8;
        // Test wiring injects the fake; production provider selection remains only none/ollama.
        let fake = Fake {
            inputs: AtomicUsize::new(0),
        };
        let first = synchronize_with_provider(&mut connection, &config.embeddings, &fake).unwrap();
        assert_eq!(first.active_segments, 2);
        assert_eq!(first.provider_inputs, 1);
        assert_eq!(first.linked_segments, 2);
        let second = synchronize_with_provider(&mut connection, &config.embeddings, &fake).unwrap();
        assert_eq!(second.provider_inputs, 0);
        assert_eq!(fake.inputs.load(Ordering::SeqCst), 1);
        let status = status_snapshot(&connection, &db).unwrap();
        assert_eq!(status.embedding.missing_active_segments, 0);
    }
}
