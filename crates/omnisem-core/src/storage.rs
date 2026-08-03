//! Project-owned `SQLite` migrations and persistence helpers.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::domain::{
    ContentHash, Revision, RevisionId, RevisionStatus, Root, RootId, ScanRun, Segment,
    SensitivityScope, SourceFile, SourceFileId, SourceState, SupportedFileType, Timestamp,
};
use crate::error::ConfigError;
use crate::hash::blake3_hex;
use crate::paths::restrict_permissions;

/// Current schema version understood by this executable.
pub const CURRENT_SCHEMA_VERSION: i64 = 4;

const MIGRATION_1: &str = include_str!("../../../migrations/0001_initial.sql");
const MIGRATION_2: &str = include_str!("../../../migrations/0002_operational_indexing.sql");
const MIGRATION_3: &str = include_str!("../../../migrations/0003_m3_incremental_snapshots.sql");
const MIGRATION_4: &str = include_str!("../../../migrations/0004_embedding_foundation.sql");

/// Applies every pending migration transactionally.
///
/// # Errors
///
/// Returns [`StorageError::FutureSchema`] for an incompatible future database,
/// [`StorageError::FtsUnavailable`] when FTS5 cannot be created, or database errors.
pub fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let existing = schema_version(connection)?;
    if let Some(version) = existing
        && version > CURRENT_SCHEMA_VERSION
    {
        return Err(StorageError::FutureSchema(version));
    }
    let current = existing.unwrap_or(0);
    if current < 1 {
        apply(connection.transaction()?, 1, MIGRATION_1)?;
    }
    if schema_version(connection)?.unwrap_or(0) < 2 {
        let transaction = connection.transaction()?;
        apply(transaction, 2, MIGRATION_2)?;
        verify_fts5(connection)?;
    }
    if schema_version(connection)?.unwrap_or(0) < 3 {
        apply(connection.transaction()?, 3, MIGRATION_3)?;
    }
    if schema_version(connection)?.unwrap_or(0) < 4 {
        apply(connection.transaction()?, 4, MIGRATION_4)?;
    }
    Ok(())
}

/// Persisted Git base for incremental indexing of one root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RootGitState {
    pub root_id: String,
    pub repo_fingerprint: Option<String>,
    pub last_indexed_commit: Option<String>,
    pub observed_head: Option<String>,
    pub last_incremental_base: Option<String>,
    pub last_incremental_at_ms: Option<i64>,
}

/// Loads Git state for a root when present.
///
/// # Errors
///
/// Returns database errors.
pub fn load_root_git_state(
    connection: &Connection,
    root_id: &RootId,
) -> Result<Option<RootGitState>, StorageError> {
    connection
        .query_row(
            "SELECT root_id, repo_fingerprint, last_indexed_commit, observed_head,
                    last_incremental_base, last_incremental_at_ms
             FROM root_git_state WHERE root_id = ?1",
            [root_id.to_string()],
            |row| {
                Ok(RootGitState {
                    root_id: row.get(0)?,
                    repo_fingerprint: row.get(1)?,
                    last_indexed_commit: row.get(2)?,
                    observed_head: row.get(3)?,
                    last_incremental_base: row.get(4)?,
                    last_incremental_at_ms: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

/// Upserts Git state for a root.
///
/// # Errors
///
/// Returns database errors.
pub fn upsert_root_git_state(
    connection: &Connection,
    state: &RootGitState,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO root_git_state(
            root_id, repo_fingerprint, last_indexed_commit, observed_head,
            last_incremental_base, last_incremental_at_ms
        ) VALUES (?1,?2,?3,?4,?5,?6)
        ON CONFLICT(root_id) DO UPDATE SET
            repo_fingerprint=excluded.repo_fingerprint,
            last_indexed_commit=excluded.last_indexed_commit,
            observed_head=excluded.observed_head,
            last_incremental_base=excluded.last_incremental_base,
            last_incremental_at_ms=excluded.last_incremental_at_ms",
        params![
            state.root_id,
            state.repo_fingerprint,
            state.last_indexed_commit,
            state.observed_head,
            state.last_incremental_base,
            state.last_incremental_at_ms,
        ],
    )?;
    Ok(())
}

/// Records a query activity sample without storing query text.
///
/// # Errors
///
/// Returns database errors.
pub fn record_query_activity(
    connection: &Connection,
    observed_at_ms: i64,
    mode: &str,
    result_count: i64,
    elapsed_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO query_activity(observed_at_ms, mode, result_count, elapsed_ms)
         VALUES (?1,?2,?3,?4)",
        params![observed_at_ms, mode, result_count, elapsed_ms],
    )?;
    // Retain a bounded window.
    connection.execute(
        "DELETE FROM query_activity WHERE id NOT IN (
            SELECT id FROM query_activity ORDER BY observed_at_ms DESC LIMIT 100
         )",
        [],
    )?;
    Ok(())
}

/// Recent query activity samples for status display.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueryActivitySample {
    pub observed_at_ms: i64,
    pub mode: String,
    pub result_count: i64,
    pub elapsed_ms: i64,
}

/// Lists recent query activity.
///
/// # Errors
///
/// Returns database errors.
pub fn list_query_activity(
    connection: &Connection,
    limit: i64,
) -> Result<Vec<QueryActivitySample>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT observed_at_ms, mode, result_count, elapsed_ms
         FROM query_activity ORDER BY observed_at_ms DESC LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], |row| {
        Ok(QueryActivitySample {
            observed_at_ms: row.get(0)?,
            mode: row.get(1)?,
            result_count: row.get(2)?,
            elapsed_ms: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn schema_version(connection: &Connection) -> Result<Option<i64>, StorageError> {
    connection
        .query_row(
            "SELECT version FROM schema_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .or_else(|error| match error {
            rusqlite::Error::SqliteFailure(_, Some(ref message))
                if message.contains("no such table") =>
            {
                Ok(None)
            }
            other => Err(other),
        })
        .map_err(StorageError::from)
}

fn apply(transaction: Transaction<'_>, version: i64, sql: &str) -> Result<(), StorageError> {
    transaction.execute_batch(sql)?;
    transaction.execute(
        "INSERT INTO schema_metadata(singleton, version) VALUES(1, ?1)
        ON CONFLICT(singleton) DO UPDATE SET version = excluded.version",
        [version],
    )?;
    transaction.commit()?;
    Ok(())
}

fn verify_fts5(connection: &Connection) -> Result<(), StorageError> {
    let exists: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='segments_fts'",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Err(StorageError::FtsUnavailable);
    }
    connection
        .execute(
            "INSERT INTO segments_fts(segments_fts) VALUES('integrity-check')",
            [],
        )
        .map_err(|_| StorageError::FtsUnavailable)?;
    Ok(())
}

/// Opens a database file, applies migrations, and restricts permissions.
///
/// # Errors
///
/// Returns storage or configuration I/O errors.
pub fn open_database(path: &Path) -> Result<Connection, StorageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| StorageError::Io {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    let mut connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&mut connection)?;
    restrict_permissions(path).map_err(|error| match error {
        ConfigError::Io { path, message } => StorageError::Io { path, message },
        other => StorageError::Io {
            path: path.to_path_buf(),
            message: other.to_string(),
        },
    })?;
    Ok(connection)
}

/// Persistence foundation failures.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database schema version {0} is newer than this executable supports")]
    FutureSchema(i64),
    #[error("FTS5 is unavailable or failed to initialize")]
    FtsUnavailable,
    #[error("database I/O error for {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("domain decode error: {0}")]
    Decode(String),
}

impl PartialEq for StorageError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::FutureSchema(a), Self::FutureSchema(b)) => a == b,
            (Self::FtsUnavailable, Self::FtsUnavailable) => true,
            (
                Self::Io {
                    path: a,
                    message: ma,
                },
                Self::Io {
                    path: b,
                    message: mb,
                },
            ) => a == b && ma == mb,
            (Self::Decode(a), Self::Decode(b)) => a == b,
            (Self::Database(a), Self::Database(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for StorageError {}

/// Upserts a root row from domain state.
///
/// # Errors
///
/// Returns database errors.
pub fn upsert_root(connection: &Connection, root: &Root) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO roots(
            id, canonical_path, display_name, include_patterns_json, exclude_patterns_json,
            follow_symlinks, enabled, sensitivity_tags_json, created_at_ms, updated_at_ms,
            config_fingerprint
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
        ON CONFLICT(id) DO UPDATE SET
            canonical_path=excluded.canonical_path,
            display_name=excluded.display_name,
            include_patterns_json=excluded.include_patterns_json,
            exclude_patterns_json=excluded.exclude_patterns_json,
            follow_symlinks=excluded.follow_symlinks,
            enabled=excluded.enabled,
            sensitivity_tags_json=excluded.sensitivity_tags_json,
            updated_at_ms=excluded.updated_at_ms,
            config_fingerprint=excluded.config_fingerprint",
        params![
            root.id.to_string(),
            root.canonical_path.display().to_string(),
            root.display_name,
            serde_json::to_string(&root.include_patterns).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&root.exclude_patterns).unwrap_or_else(|_| "[]".into()),
            i64::from(root.follow_symlinks),
            i64::from(root.enabled),
            serde_json::to_string(&root.sensitivity_tags).unwrap_or_else(|_| "[]".into()),
            root.created_at.as_millis(),
            root.updated_at.as_millis(),
            root.config_fingerprint,
        ],
    )?;
    Ok(())
}

/// Deletes a root and all derived rows, including active FTS entries.
///
/// # Errors
///
/// Returns database errors.
pub fn delete_root_derived(
    connection: &mut Connection,
    root_id: &RootId,
) -> Result<RootRemovalCounts, StorageError> {
    let transaction = connection.transaction()?;
    let root_key = root_id.to_string();
    let source_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM source_files WHERE root_id = ?1",
        [&root_key],
        |row| row.get(0),
    )?;
    let revision_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM revisions WHERE source_file_id IN (
            SELECT id FROM source_files WHERE root_id = ?1
        )",
        [&root_key],
        |row| row.get(0),
    )?;
    let segment_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM segments WHERE revision_id IN (
            SELECT id FROM revisions WHERE source_file_id IN (
                SELECT id FROM source_files WHERE root_id = ?1
            )
        )",
        [&root_key],
        |row| row.get(0),
    )?;
    transaction.execute("DELETE FROM segments_fts WHERE root_id = ?1", [&root_key])?;
    transaction.execute(
        "DELETE FROM segments WHERE revision_id IN (
            SELECT id FROM revisions WHERE source_file_id IN (
                SELECT id FROM source_files WHERE root_id = ?1
            )
        )",
        [&root_key],
    )?;
    transaction.execute(
        "DELETE FROM revisions WHERE source_file_id IN (
            SELECT id FROM source_files WHERE root_id = ?1
        )",
        [&root_key],
    )?;
    transaction.execute("DELETE FROM scan_runs WHERE root_id = ?1", [&root_key])?;
    transaction.execute("DELETE FROM source_files WHERE root_id = ?1", [&root_key])?;
    transaction.execute("DELETE FROM roots WHERE id = ?1", [&root_key])?;
    transaction.commit()?;
    Ok(RootRemovalCounts {
        source_files: source_count,
        revisions: revision_count,
        segments: segment_count,
    })
}

/// Counts of derived records removed with a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct RootRemovalCounts {
    pub source_files: i64,
    pub revisions: i64,
    pub segments: i64,
}

/// Loads a source file by root and relative path when present.
///
/// # Errors
///
/// Returns database or decode errors.
pub fn find_source_file(
    connection: &Connection,
    root_id: &RootId,
    relative_path: &Path,
) -> Result<Option<SourceFile>, StorageError> {
    connection
        .query_row(
            "SELECT id, root_id, relative_path, canonical_path_hash, file_type, size_bytes,
                    modified_at_ms, current_revision_id, state, first_seen_at_ms, last_seen_at_ms
             FROM source_files WHERE root_id = ?1 AND relative_path = ?2",
            params![root_id.to_string(), relative_path.display().to_string()],
            map_source_file,
        )
        .optional()
        .map_err(StorageError::from)
}

/// Lists active source files for a root.
///
/// # Errors
///
/// Returns database or decode errors.
pub fn list_active_source_files(
    connection: &Connection,
    root_id: &RootId,
) -> Result<Vec<SourceFile>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id, root_id, relative_path, canonical_path_hash, file_type, size_bytes,
                modified_at_ms, current_revision_id, state, first_seen_at_ms, last_seen_at_ms
         FROM source_files WHERE root_id = ?1 AND state = 'active'
         ORDER BY relative_path ASC",
    )?;
    let rows = statement.query_map([root_id.to_string()], map_source_file)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn map_source_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceFile> {
    let file_type = SupportedFileType::from_str(row.get::<_, String>(4)?.as_str())
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let state = SourceState::from_str(row.get::<_, String>(8)?.as_str())
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let current = row
        .get::<_, Option<String>>(7)?
        .map(|value| RevisionId::from_str(&value))
        .transpose()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok(SourceFile {
        id: SourceFileId::from_str(&row.get::<_, String>(0)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        root_id: RootId::from_str(&row.get::<_, String>(1)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        relative_path: PathBuf::from(row.get::<_, String>(2)?),
        canonical_path_hash: ContentHash(row.get(3)?),
        file_type,
        size_bytes: u64::try_from(row.get::<_, i64>(5)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        modified_at: row.get::<_, Option<i64>>(6)?.map(Timestamp::from_millis),
        current_revision_id: current,
        state,
        first_seen_at: Timestamp::from_millis(row.get(9)?),
        last_seen_at: Timestamp::from_millis(row.get(10)?),
    })
}

/// Loads the current revision for a source file when present.
///
/// # Errors
///
/// Returns database or decode errors.
pub fn get_revision(
    connection: &Connection,
    revision_id: &RevisionId,
) -> Result<Option<Revision>, StorageError> {
    connection
        .query_row(
            "SELECT id, source_file_id, content_hash, parser_id, parser_version,
                    extracted_text_hash, observed_at_ms, indexed_at_ms, status, error_code,
                    error_message
             FROM revisions WHERE id = ?1",
            [revision_id.to_string()],
            map_revision,
        )
        .optional()
        .map_err(StorageError::from)
}

fn map_revision(row: &rusqlite::Row<'_>) -> rusqlite::Result<Revision> {
    let status = RevisionStatus::from_str(row.get::<_, String>(8)?.as_str())
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok(Revision {
        id: RevisionId::from_str(&row.get::<_, String>(0)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        source_file_id: SourceFileId::from_str(&row.get::<_, String>(1)?)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        content_hash: ContentHash(row.get(2)?),
        parser_id: row.get(3)?,
        parser_version: row.get(4)?,
        extracted_text_hash: row.get::<_, Option<String>>(5)?.map(ContentHash),
        observed_at: Timestamp::from_millis(row.get(6)?),
        indexed_at: row.get::<_, Option<i64>>(7)?.map(Timestamp::from_millis),
        status,
        error_code: row.get(9)?,
        error_message: row.get(10)?,
    })
}

/// Inserts an immutable indexed revision, segments, and active FTS rows, then
/// promotes the source file pointer inside one transaction.
///
/// # Errors
///
/// Returns database errors. Callers must not observe partial promotions.
pub fn promote_revision(
    connection: &mut Connection,
    source: &SourceFile,
    revision: &Revision,
    segments: &[Segment],
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    if let Some(previous) = source.current_revision_id {
        transaction.execute(
            "DELETE FROM segments_fts WHERE revision_id = ?1",
            [previous.to_string()],
        )?;
    }
    transaction.execute(
        "INSERT INTO revisions(
            id, source_file_id, content_hash, parser_id, parser_version, extracted_text_hash,
            observed_at_ms, indexed_at_ms, status, error_code, error_message
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            revision.id.to_string(),
            revision.source_file_id.to_string(),
            revision.content_hash.as_str(),
            revision.parser_id,
            revision.parser_version,
            revision
                .extracted_text_hash
                .as_ref()
                .map(ContentHash::as_str),
            revision.observed_at.as_millis(),
            revision.indexed_at.map(Timestamp::as_millis),
            revision.status.as_str(),
            revision.error_code,
            revision.error_message,
        ],
    )?;
    for segment in segments {
        transaction.execute(
            "INSERT INTO segments(
                id, revision_id, segment_type, anchor, ordinal, text, text_hash, token_count,
                metadata_json, sensitivity_scope
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                segment.id.to_string(),
                segment.revision_id.to_string(),
                segment.segment_type.as_str(),
                segment.anchor,
                segment.ordinal,
                segment.text,
                segment.text_hash.as_str(),
                segment.token_count.map(i64::from),
                segment.metadata.to_string(),
                segment.sensitivity_scope.map(SensitivityScope::as_str),
            ],
        )?;
        transaction.execute(
            "INSERT INTO segments_fts(
                text, segment_id, revision_id, source_file_id, root_id, relative_path, anchor
            ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                segment.text,
                segment.id.to_string(),
                revision.id.to_string(),
                source.id.to_string(),
                source.root_id.to_string(),
                source.relative_path.display().to_string(),
                segment.anchor,
            ],
        )?;
    }
    transaction.execute(
        "UPDATE source_files SET
            canonical_path_hash = ?1,
            file_type = ?2,
            size_bytes = ?3,
            modified_at_ms = ?4,
            current_revision_id = ?5,
            state = ?6,
            last_seen_at_ms = ?7
         WHERE id = ?8",
        params![
            source.canonical_path_hash.as_str(),
            source.file_type.as_str(),
            i64::try_from(source.size_bytes).unwrap_or(i64::MAX),
            source.modified_at.map(Timestamp::as_millis),
            revision.id.to_string(),
            SourceState::Active.as_str(),
            source.last_seen_at.as_millis(),
            source.id.to_string(),
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Inserts or updates a source-file identity row without promoting a revision.
///
/// # Errors
///
/// Returns database errors.
pub fn upsert_source_file(
    connection: &Connection,
    source: &SourceFile,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO source_files(
            id, root_id, relative_path, canonical_path_hash, file_type, size_bytes,
            modified_at_ms, current_revision_id, state, first_seen_at_ms, last_seen_at_ms
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
        ON CONFLICT(root_id, relative_path) DO UPDATE SET
            canonical_path_hash=excluded.canonical_path_hash,
            file_type=excluded.file_type,
            size_bytes=excluded.size_bytes,
            modified_at_ms=excluded.modified_at_ms,
            current_revision_id=excluded.current_revision_id,
            state=excluded.state,
            last_seen_at_ms=excluded.last_seen_at_ms",
        params![
            source.id.to_string(),
            source.root_id.to_string(),
            source.relative_path.display().to_string(),
            source.canonical_path_hash.as_str(),
            source.file_type.as_str(),
            i64::try_from(source.size_bytes).unwrap_or(i64::MAX),
            source.modified_at.map(Timestamp::as_millis),
            source.current_revision_id.map(|id| id.to_string()),
            source.state.as_str(),
            source.first_seen_at.as_millis(),
            source.last_seen_at.as_millis(),
        ],
    )?;
    Ok(())
}

/// Marks a source deleted, clears the current pointer, and removes active FTS rows.
///
/// # Errors
///
/// Returns database errors.
pub fn mark_source_deleted(
    connection: &mut Connection,
    source: &SourceFile,
    deleted_at: Timestamp,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    if let Some(revision_id) = source.current_revision_id {
        transaction.execute(
            "DELETE FROM segments_fts WHERE revision_id = ?1",
            [revision_id.to_string()],
        )?;
    }
    transaction.execute(
        "UPDATE source_files SET state = ?1, current_revision_id = NULL, last_seen_at_ms = ?2
         WHERE id = ?3",
        params![
            SourceState::Deleted.as_str(),
            deleted_at.as_millis(),
            source.id.to_string()
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Records a completed or failed scan run.
///
/// # Errors
///
/// Returns database errors.
pub fn insert_scan_run(connection: &Connection, run: &ScanRun) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO scan_runs(
            id, root_id, started_at_ms, completed_at_ms, status, additions, modifications,
            unchanged, deletions, skipped, failures, segments_indexed, error_code
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            run.id.to_string(),
            run.root_id.to_string(),
            run.started_at.as_millis(),
            run.completed_at.map(Timestamp::as_millis),
            run.status.as_str(),
            run.additions,
            run.modifications,
            run.unchanged,
            run.deletions,
            run.skipped,
            run.failures,
            run.segments_indexed,
            run.error_code,
        ],
    )?;
    Ok(())
}

/// Aggregate status counters for operational reporting.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct StatusSnapshot {
    pub schema_version: i64,
    pub root_count: i64,
    pub enabled_root_count: i64,
    pub active_source_files: i64,
    pub active_revisions: i64,
    pub active_segments: i64,
    pub fts_rows: i64,
    pub failed_sources: i64,
    pub sensitivity_tag_count: i64,
    pub last_successful_scan_ms: Option<i64>,
    pub last_failed_scan_ms: Option<i64>,
    pub database_size_bytes: u64,
    pub embedding: EmbeddingStatus,
}

/// Persisted-only embedding health projection. Reading it never contacts a provider.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct EmbeddingStatus {
    pub space_exists: bool,
    pub active_space_id: Option<String>,
    pub provider: Option<String>,
    pub canonical_model: Option<String>,
    pub model_digest: Option<String>,
    pub dimensions: Option<u32>,
    pub normalization: Option<String>,
    pub active_segments: i64,
    pub linked_active_segments: i64,
    pub missing_active_segments: i64,
    pub current_failure_count: i64,
    pub latest_sync_status: Option<String>,
    pub latest_sync_completed_at_ms: Option<i64>,
}

/// Network-free comparison of configured compatibility fields with the active space.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmbeddingCompatibility {
    pub configured_enabled: bool,
    pub configured_provider: String,
    pub configured_model: Option<String>,
    pub configured_dimensions: Option<u32>,
    pub state: String,
}

/// Compares fields available without resolving a mutable model tag.
#[must_use]
pub fn embedding_compatibility(
    config: &crate::config::EmbeddingConfig,
    status: &EmbeddingStatus,
) -> EmbeddingCompatibility {
    let provider = match config.provider {
        crate::config::EmbeddingProviderConfig::None => "none",
        crate::config::EmbeddingProviderConfig::Ollama => "ollama",
    };
    let configured_model = (!config.model.is_empty()).then(|| config.model.clone());
    let configured_dimensions = (config.dimensions != 0).then_some(config.dimensions);
    let canonical_candidate = if config.model.contains(':') {
        config.model.clone()
    } else {
        format!("{}:latest", config.model)
    };
    let state = if !config.enabled {
        "disabled"
    } else if !status.space_exists {
        "missing_active_space"
    } else if status.provider.as_deref() != Some(provider)
        || status.canonical_model.as_deref() != Some(canonical_candidate.as_str())
        || configured_dimensions.is_some_and(|value| status.dimensions != Some(value))
    {
        "incompatible"
    } else {
        "compatible_requires_digest_resolution"
    };
    EmbeddingCompatibility {
        configured_enabled: config.enabled,
        configured_provider: provider.into(),
        configured_model,
        configured_dimensions,
        state: state.into(),
    }
}

/// Collects operational status counters.
///
/// # Errors
///
/// Returns database errors.
#[allow(clippy::too_many_lines)]
pub fn status_snapshot(
    connection: &Connection,
    database_path: &Path,
) -> Result<StatusSnapshot, StorageError> {
    let schema_version = schema_version(connection)?.unwrap_or(0);
    let root_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM roots", [], |row| row.get(0))?;
    let enabled_root_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM roots WHERE enabled = 1", [], |row| {
            row.get(0)
        })?;
    let active_source_files: i64 = connection.query_row(
        "SELECT COUNT(*) FROM source_files WHERE state = 'active'",
        [],
        |row| row.get(0),
    )?;
    let active_revisions: i64 = connection.query_row(
        "SELECT COUNT(*) FROM source_files WHERE state = 'active' AND current_revision_id IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    let active_segments: i64 = connection.query_row(
        "SELECT COUNT(*) FROM segments WHERE revision_id IN (
            SELECT current_revision_id FROM source_files
            WHERE state = 'active' AND current_revision_id IS NOT NULL
        )",
        [],
        |row| row.get(0),
    )?;
    let fts_rows: i64 =
        connection.query_row("SELECT COUNT(*) FROM segments_fts", [], |row| row.get(0))?;
    let failed_sources: i64 = connection.query_row(
        "SELECT COUNT(*) FROM source_files WHERE state = 'error'",
        [],
        |row| row.get(0),
    )?;
    let sensitivity_tag_count: i64 = connection
        .prepare("SELECT sensitivity_tags_json FROM roots")?
        .query_map([], |row| row.get::<_, String>(0))?
        .try_fold(0_i64, |acc, item| {
            let json = item?;
            let tags: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
            Ok::<_, rusqlite::Error>(acc + i64::try_from(tags.len()).unwrap_or(0))
        })?;
    let last_successful_scan_ms = connection
        .query_row(
            "SELECT completed_at_ms FROM scan_runs
             WHERE status = 'completed' AND completed_at_ms IS NOT NULL
             ORDER BY completed_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let last_failed_scan_ms = connection
        .query_row(
            "SELECT completed_at_ms FROM scan_runs
             WHERE status = 'failed' AND completed_at_ms IS NOT NULL
             ORDER BY completed_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let database_size_bytes = std::fs::metadata(database_path).map_or(0, |meta| meta.len());
    let active_space_id: Option<String> = connection
        .query_row(
            "SELECT active_embedding_space_id FROM embedding_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let space_fields = active_space_id.as_ref().and_then(|id| connection.query_row("SELECT provider,canonical_model,model_digest,dimensions,normalization FROM embedding_spaces WHERE id=?1", [id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,u32>(3)?,row.get::<_,String>(4)?))).optional().ok().flatten());
    let linked_active_segments = if let Some(id) = &active_space_id {
        connection.query_row("SELECT COUNT(*) FROM segment_embeddings e JOIN source_files f ON f.current_revision_id=e.revision_id WHERE f.state='active' AND e.embedding_space_id=?1",[id],|row|row.get(0))?
    } else {
        0
    };
    let current_failure_count = if let Some(id) = &active_space_id {
        connection.query_row("SELECT COUNT(*) FROM embedding_failures ef JOIN segments s ON s.id=ef.segment_id JOIN source_files f ON f.current_revision_id=s.revision_id WHERE f.state='active' AND ef.embedding_space_id=?1",[id],|row|row.get(0))?
    } else {
        0
    };
    let latest_sync: Option<(String, Option<i64>)> = connection
        .query_row(
            "SELECT status,completed_at_ms FROM embedding_sync_runs ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let embedding = EmbeddingStatus {
        space_exists: active_space_id.is_some(),
        active_space_id,
        provider: space_fields.as_ref().map(|v| v.0.clone()),
        canonical_model: space_fields.as_ref().map(|v| v.1.clone()),
        model_digest: space_fields.as_ref().map(|v| v.2.clone()),
        dimensions: space_fields.as_ref().map(|v| v.3),
        normalization: space_fields.as_ref().map(|v| v.4.clone()),
        active_segments,
        linked_active_segments,
        missing_active_segments: active_segments.saturating_sub(linked_active_segments),
        current_failure_count,
        latest_sync_status: latest_sync.as_ref().map(|v| v.0.clone()),
        latest_sync_completed_at_ms: latest_sync.and_then(|v| v.1),
    };
    Ok(StatusSnapshot {
        schema_version,
        root_count,
        enabled_root_count,
        active_source_files,
        active_revisions,
        active_segments,
        fts_rows,
        failed_sources,
        sensitivity_tag_count,
        last_successful_scan_ms,
        last_failed_scan_ms,
        database_size_bytes,
        embedding,
    })
}

/// One addition/modification/deletion for `omnisem changes`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChangeEvent {
    pub kind: String,
    pub root_id: String,
    pub relative_path: String,
    pub previous_content_hash: Option<String>,
    pub current_content_hash: Option<String>,
    pub observed_at_ms: Option<i64>,
    pub indexed_at_ms: Option<i64>,
}

/// Lists deterministic change events after an optional timestamp lower bound.
///
/// # Errors
///
/// Returns database errors.
pub fn list_changes(
    connection: &Connection,
    root_id: Option<&RootId>,
    since_ms: Option<i64>,
) -> Result<Vec<ChangeEvent>, StorageError> {
    let mut events = Vec::new();
    let mut sql = String::from(
        "SELECT sf.root_id, sf.relative_path, sf.state, sf.last_seen_at_ms,
                cur.content_hash, cur.observed_at_ms, cur.indexed_at_ms,
                prev.content_hash
         FROM source_files sf
         LEFT JOIN revisions cur ON cur.id = sf.current_revision_id
         LEFT JOIN revisions prev ON prev.source_file_id = sf.id
            AND prev.id != IFNULL(sf.current_revision_id, '')
            AND prev.status = 'indexed'
         WHERE 1=1",
    );
    if root_id.is_some() {
        sql.push_str(" AND sf.root_id = ?1");
    }
    sql.push_str(" ORDER BY sf.relative_path ASC, sf.root_id ASC");

    let mut statement = connection.prepare(&sql)?;
    let mut rows = match root_id {
        Some(id) => statement.query(params![id.to_string()])?,
        None => statement.query([])?,
    };

    while let Some(row) = rows.next()? {
        let state = row.get::<_, String>(2)?;
        let last_seen: i64 = row.get(3)?;
        if let Some(since) = since_ms
            && last_seen < since
            && state != "deleted"
        {
            continue;
        }
        let current_hash: Option<String> = row.get(4)?;
        let observed_at_ms: Option<i64> = row.get(5)?;
        let indexed_at_ms: Option<i64> = row.get(6)?;
        let previous_hash: Option<String> = row.get(7)?;
        let kind = match state.as_str() {
            "deleted" => {
                if let Some(since) = since_ms
                    && last_seen < since
                {
                    continue;
                }
                "deletion"
            }
            "active" if previous_hash.is_none() => "addition",
            "active" if previous_hash != current_hash => "modification",
            _ => continue,
        };
        events.push(ChangeEvent {
            kind: kind.into(),
            root_id: row.get(0)?,
            relative_path: row.get(1)?,
            previous_content_hash: previous_hash,
            current_content_hash: current_hash,
            observed_at_ms,
            indexed_at_ms,
        });
    }
    events.sort_by(|left, right| {
        left.relative_path
            .cmp(&right.relative_path)
            .then(left.root_id.cmp(&right.root_id))
            .then(left.kind.cmp(&right.kind))
    });
    Ok(events)
}

/// Counts active source files for one root.
///
/// # Errors
///
/// Returns database errors.
pub fn count_active_sources(
    connection: &Connection,
    root_id: &RootId,
) -> Result<i64, StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM source_files WHERE root_id = ?1 AND state = 'active'",
            [root_id.to_string()],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

/// Hashes a canonical path for storage.
#[must_use]
pub fn path_hash(path: &Path) -> ContentHash {
    blake3_hex(path.display().to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn migration_creates_schema_and_is_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT version FROM schema_metadata", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        let fts: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='segments_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts, 1);
    }

    #[test]
    fn upgrades_from_schema_v1() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection
            .execute(
                "INSERT INTO schema_metadata(singleton, version) VALUES(1, 1)",
                [],
            )
            .unwrap();
        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT version FROM schema_metadata", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    fn database_at(version: i64) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection
            .execute(
                "INSERT INTO schema_metadata(singleton,version) VALUES(1,1)",
                [],
            )
            .unwrap();
        if version >= 2 {
            connection.execute_batch(MIGRATION_2).unwrap();
            connection
                .execute("UPDATE schema_metadata SET version=2", [])
                .unwrap();
        }
        if version >= 3 {
            connection.execute_batch(MIGRATION_3).unwrap();
            connection
                .execute("UPDATE schema_metadata SET version=3", [])
                .unwrap();
        }
        connection
    }

    #[test]
    fn upgrades_from_schema_v2_and_v3() {
        for version in [2, 3] {
            let mut connection = database_at(version);
            migrate(&mut connection).unwrap();
            assert_eq!(schema_version(&connection).unwrap(), Some(4));
        }
    }

    #[test]
    fn failed_embedding_migration_rolls_back() {
        let mut connection = database_at(3);
        connection
            .execute_batch("CREATE TABLE embedding_spaces(id TEXT PRIMARY KEY);")
            .unwrap();
        assert!(migrate(&mut connection).is_err());
        assert_eq!(schema_version(&connection).unwrap(), Some(3));
        let cache_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='embedding_vectors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cache_exists, 0);
    }

    #[test]
    fn embedding_cache_is_unique_and_space_isolated() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        for id in ["es_a", "es_b"] {
            connection.execute("INSERT INTO embedding_spaces(id,provider,canonical_model,model_digest,dimensions,normalization,input_contract_version,created_at_ms,config_fingerprint) VALUES(?1,'ollama','m',?1,8,'l2','v1',0,?1)",[id]).unwrap();
        }
        let bytes = vec![0_u8; 32];
        connection.execute("INSERT INTO embedding_vectors(embedding_space_id,text_hash,vector_bytes,dimensions,created_at_ms) VALUES('es_a','blake3:x',?1,8,0)",[&bytes]).unwrap();
        assert!(connection.execute("INSERT INTO embedding_vectors(embedding_space_id,text_hash,vector_bytes,dimensions,created_at_ms) VALUES('es_a','blake3:x',?1,8,0)",[&bytes]).is_err());
        connection.execute("INSERT INTO embedding_vectors(embedding_space_id,text_hash,vector_bytes,dimensions,created_at_ms) VALUES('es_b','blake3:x',?1,8,0)",[&bytes]).unwrap();
        assert!(connection.execute("INSERT INTO embedding_vectors(embedding_space_id,text_hash,vector_bytes,dimensions,created_at_ms) VALUES('es_a','blake3:y',x'00',8,0)",[]).is_err());
    }

    #[test]
    fn future_schema_is_rejected() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        connection
            .execute("UPDATE schema_metadata SET version = 999", [])
            .unwrap();
        assert!(matches!(
            migrate(&mut connection),
            Err(StorageError::FutureSchema(999))
        ));
    }

    #[test]
    fn open_database_creates_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        open_database(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn configured_active_space_compatibility_is_explicit_without_network() {
        let mut config = crate::config::EmbeddingConfig::default();
        let empty = EmbeddingStatus::default();
        assert_eq!(embedding_compatibility(&config, &empty).state, "disabled");
        config.enabled = true;
        config.provider = crate::config::EmbeddingProviderConfig::Ollama;
        config.endpoint = "http://localhost:11434".into();
        config.model = "fixture".into();
        config.dimensions = 8;
        assert_eq!(
            embedding_compatibility(&config, &empty).state,
            "missing_active_space"
        );
        let active = EmbeddingStatus {
            space_exists: true,
            provider: Some("ollama".into()),
            canonical_model: Some("fixture:latest".into()),
            dimensions: Some(8),
            ..EmbeddingStatus::default()
        };
        assert_eq!(
            embedding_compatibility(&config, &active).state,
            "compatible_requires_digest_resolution"
        );
        config.dimensions = 16;
        assert_eq!(
            embedding_compatibility(&config, &active).state,
            "incompatible"
        );
    }
}
