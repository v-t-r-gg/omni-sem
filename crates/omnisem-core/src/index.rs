//! Operational indexing: stable reads, immutable revisions, and active FTS promotion.

use std::collections::HashSet;
use std::time::Instant;

use rusqlite::Connection;
use serde::Serialize;

use crate::config::AppConfig;
use crate::discovery::{DEFAULT_MAX_FILE_SIZE_BYTES, DiscoveryOptions, discover_root};
use crate::domain::{
    ContentHash, DiscoveredDocument, Revision, RevisionId, RevisionStatus, Root, RootId, ScanRun,
    ScanRunId, ScanStatus, Segment, SegmentId, SourceFile, SourceFileId, SourceState, Timestamp,
};
use crate::error::IndexError;
use crate::hash::blake3_hex;
use crate::io::read_stable_file;
use crate::parsing::{DocumentParser, ParseError, ParserRegistry, SourceDocument};
use crate::storage::{
    StorageError, find_source_file, insert_scan_run, list_active_source_files, mark_source_deleted,
    path_hash, promote_revision, upsert_root, upsert_source_file,
};

/// Indexing mode for a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexMode {
    Full,
    Incremental,
}

/// Summary of one multi-root indexing invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexReport {
    pub roots_scanned: u32,
    pub files_discovered: u32,
    pub additions: u32,
    pub modifications: u32,
    pub unchanged: u32,
    pub deletions: u32,
    pub skipped: u32,
    pub failures: u32,
    pub segments_indexed: u32,
    pub duration_ms: u64,
    pub root_reports: Vec<RootIndexReport>,
}

/// Per-root indexing counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootIndexReport {
    pub root_id: String,
    pub root_name: String,
    pub mode: IndexMode,
    pub requested_base: Option<String>,
    pub resolved_base: Option<String>,
    pub current_head: Option<String>,
    pub changed_paths: u32,
    pub explicit_deletions: u32,
    pub fallback_reason: Option<String>,
    pub files_discovered: u32,
    pub additions: u32,
    pub modifications: u32,
    pub unchanged: u32,
    pub deletions: u32,
    pub skipped: u32,
    pub failures: u32,
    pub segments_indexed: u32,
    pub failed: bool,
    pub error_code: Option<String>,
}

/// Controls indexing behavior for [`index_roots`].
#[derive(Debug, Clone, Default)]
pub struct IndexOptions {
    /// When `Some`, attempt Git-aware incremental indexing.
    ///
    /// - `Some(None)` uses the last recorded successful Git base, or full scans when missing.
    /// - `Some(Some(rev))` uses the supplied revision as the comparison base.
    pub since: Option<Option<String>>,
}

/// Indexes all enabled roots or a single selected root.
///
/// # Errors
///
/// Returns configuration, database, or systemic filesystem failures. Document-level
/// failures are accumulated into the report and surface as partial success to the CLI.
pub fn index_roots(
    connection: &mut Connection,
    config: &AppConfig,
    only_root: Option<&RootId>,
) -> Result<IndexReport, IndexError> {
    index_roots_with_options(connection, config, only_root, &IndexOptions::default())
}

/// Indexes roots with optional Git-aware incremental selection.
///
/// # Errors
///
/// Returns configuration, database, or systemic filesystem failures.
pub fn index_roots_with_options(
    connection: &mut Connection,
    config: &AppConfig,
    only_root: Option<&RootId>,
    options: &IndexOptions,
) -> Result<IndexReport, IndexError> {
    let started = Instant::now();
    let registry = ParserRegistry::with_defaults().map_err(|error| {
        IndexError::Internal(format!("parser registry initialization failed: {error}"))
    })?;
    let mut roots = config.domain_roots()?;
    if let Some(filter) = only_root {
        roots.retain(|root| root.id == *filter);
        if roots.is_empty() {
            return Err(IndexError::Config(crate::error::ConfigError::RootNotFound(
                filter.to_string(),
            )));
        }
    } else {
        roots.retain(|root| root.enabled);
    }
    if roots.is_empty() {
        return Err(IndexError::NoRoots);
    }

    let mut report = IndexReport {
        roots_scanned: 0,
        files_discovered: 0,
        additions: 0,
        modifications: 0,
        unchanged: 0,
        deletions: 0,
        skipped: 0,
        failures: 0,
        segments_indexed: 0,
        duration_ms: 0,
        root_reports: Vec::new(),
    };

    for root in roots {
        let root_report = match &options.since {
            None => {
                let report =
                    index_one_root_full(connection, &root, &registry, None, None, None, None)?;
                // Record Git head after successful full scans so later `--since` has a base.
                if report.failures == 0 && !report.failed {
                    if let Some(ctx) = crate::git::detect_git_root(&root.canonical_path) {
                        let _ = crate::storage::upsert_root_git_state(
                            connection,
                            &crate::storage::RootGitState {
                                root_id: root.id.to_string(),
                                repo_fingerprint: Some(ctx.repo_fingerprint),
                                last_indexed_commit: Some(ctx.head.clone()),
                                observed_head: Some(ctx.head),
                                last_incremental_base: None,
                                last_incremental_at_ms: None,
                            },
                        );
                    }
                }
                report
            }
            Some(requested) => index_one_root_maybe_incremental(
                connection,
                &root,
                &registry,
                requested.as_deref(),
            )?,
        };
        report.roots_scanned += 1;
        report.files_discovered += root_report.files_discovered;
        report.additions += root_report.additions;
        report.modifications += root_report.modifications;
        report.unchanged += root_report.unchanged;
        report.deletions += root_report.deletions;
        report.skipped += root_report.skipped;
        report.failures += root_report.failures;
        report.segments_indexed += root_report.segments_indexed;
        report.root_reports.push(root_report);
    }

    report.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(report)
}

#[allow(clippy::too_many_lines)]
fn index_one_root_maybe_incremental(
    connection: &mut Connection,
    root: &Root,
    registry: &ParserRegistry,
    requested: Option<&str>,
) -> Result<RootIndexReport, IndexError> {
    use crate::git::{GitChangeKind, collect_changes, detect_git_root, revision_exists};
    use crate::storage::{RootGitState, load_root_git_state, upsert_root_git_state};

    let Some(ctx) = detect_git_root(&root.canonical_path) else {
        return index_one_root_full(
            connection,
            root,
            registry,
            requested.map(str::to_owned),
            None,
            None,
            Some("not a git repository".into()),
        );
    };

    let prior = load_root_git_state(connection, &root.id)?;
    let resolved_base = match requested {
        Some(rev) => {
            if revision_exists(&root.canonical_path, rev) {
                Some(rev.to_owned())
            } else {
                let report = index_one_root_full(
                    connection,
                    root,
                    registry,
                    Some(rev.to_owned()),
                    None,
                    Some(ctx.head.clone()),
                    Some(format!("git revision unavailable: {rev}")),
                )?;
                if report.failures == 0 && !report.failed {
                    let _ = upsert_root_git_state(
                        connection,
                        &RootGitState {
                            root_id: root.id.to_string(),
                            repo_fingerprint: Some(ctx.repo_fingerprint.clone()),
                            last_indexed_commit: Some(ctx.head.clone()),
                            observed_head: Some(ctx.head.clone()),
                            last_incremental_base: None,
                            last_incremental_at_ms: Some(Timestamp::now()?.as_millis()),
                        },
                    );
                }
                return Ok(report);
            }
        }
        None => prior
            .as_ref()
            .and_then(|state| state.last_indexed_commit.clone()),
    };

    let Some(base) = resolved_base else {
        let report = index_one_root_full(
            connection,
            root,
            registry,
            None,
            None,
            Some(ctx.head.clone()),
            Some("no prior git base; performing full scan".into()),
        )?;
        if report.failures == 0 && !report.failed {
            let _ = upsert_root_git_state(
                connection,
                &RootGitState {
                    root_id: root.id.to_string(),
                    repo_fingerprint: Some(ctx.repo_fingerprint.clone()),
                    last_indexed_commit: Some(ctx.head.clone()),
                    observed_head: Some(ctx.head.clone()),
                    last_incremental_base: None,
                    last_incremental_at_ms: Some(Timestamp::now()?.as_millis()),
                },
            );
        }
        return Ok(report);
    };

    let changes = match collect_changes(&root.canonical_path, &ctx, &base) {
        Ok(changes) => changes,
        Err(error) => {
            return index_one_root_full(
                connection,
                root,
                registry,
                Some(base),
                None,
                Some(ctx.head.clone()),
                Some(format!("git change collection failed: {error}")),
            );
        }
    };

    let started_at = Timestamp::now()?;
    upsert_root(connection, root)?;
    let mut additions = 0_u32;
    let mut modifications = 0_u32;
    let mut unchanged = 0_u32;
    let mut failures = 0_u32;
    let mut segments_indexed = 0_u32;
    let mut explicit_deletions = 0_u32;
    let mut files_discovered = 0_u32;

    for change in &changes {
        match change.kind {
            GitChangeKind::Deleted => {
                if let Some(source) = find_source_file(connection, &root.id, &change.relative_path)?
                {
                    mark_source_deleted(connection, &source, Timestamp::now()?)?;
                    explicit_deletions += 1;
                }
            }
            GitChangeKind::Added | GitChangeKind::Modified => {
                let absolute = root.canonical_path.join(&change.relative_path);
                if !absolute.is_file() {
                    continue;
                }
                let Ok(meta) = std::fs::metadata(&absolute) else {
                    failures += 1;
                    continue;
                };
                let Ok(modified_at) = meta
                    .modified()
                    .ok()
                    .and_then(|value| Timestamp::try_from_system_time(value).ok())
                    .ok_or(())
                else {
                    failures += 1;
                    continue;
                };
                let Some(file_type) =
                    crate::discovery::classify_supported_file(&change.relative_path)
                else {
                    continue;
                };
                files_discovered += 1;
                let document = DiscoveredDocument {
                    root_id: root.id,
                    canonical_path: absolute,
                    relative_path: change.relative_path.clone(),
                    size_bytes: meta.len(),
                    modified_at,
                    file_type,
                };
                match process_document(connection, root, &document, registry) {
                    Ok(DocumentOutcome::Addition { segments }) => {
                        additions += 1;
                        segments_indexed += segments;
                    }
                    Ok(DocumentOutcome::Modification { segments }) => {
                        modifications += 1;
                        segments_indexed += segments;
                    }
                    Ok(DocumentOutcome::Unchanged) => unchanged += 1,
                    Err(_) => failures += 1,
                }
            }
        }
    }

    if failures == 0 {
        upsert_root_git_state(
            connection,
            &RootGitState {
                root_id: root.id.to_string(),
                repo_fingerprint: Some(ctx.repo_fingerprint),
                last_indexed_commit: Some(ctx.head.clone()),
                observed_head: Some(ctx.head.clone()),
                last_incremental_base: Some(base.clone()),
                last_incremental_at_ms: Some(Timestamp::now()?.as_millis()),
            },
        )?;
    }

    let completed_at = Timestamp::now()?;
    insert_scan_run(
        connection,
        &ScanRun {
            id: ScanRunId::new(),
            root_id: root.id,
            started_at,
            completed_at: Some(completed_at),
            status: ScanStatus::Completed,
            additions,
            modifications,
            unchanged,
            deletions: explicit_deletions,
            skipped: 0,
            failures,
            segments_indexed,
            error_code: None,
        },
    )?;

    Ok(RootIndexReport {
        root_id: root.id.to_string(),
        root_name: root.display_name.clone(),
        mode: IndexMode::Incremental,
        requested_base: requested.map(str::to_owned),
        resolved_base: Some(base),
        current_head: Some(ctx.head),
        changed_paths: u32::try_from(changes.len()).unwrap_or(u32::MAX),
        explicit_deletions,
        fallback_reason: None,
        files_discovered,
        additions,
        modifications,
        unchanged,
        deletions: explicit_deletions,
        skipped: 0,
        failures,
        segments_indexed,
        failed: false,
        error_code: None,
    })
}

#[allow(clippy::too_many_lines)]
fn index_one_root_full(
    connection: &mut Connection,
    root: &Root,
    registry: &ParserRegistry,
    requested_base: Option<String>,
    resolved_base: Option<String>,
    current_head: Option<String>,
    fallback_reason: Option<String>,
) -> Result<RootIndexReport, IndexError> {
    let started_at = Timestamp::now()?;
    upsert_root(connection, root)?;

    let discovery = match discover_root(root, &DiscoveryOptions::default()) {
        Ok(report) => report,
        Err(error) => {
            let completed_at = Timestamp::now()?;
            let run = ScanRun {
                id: ScanRunId::new(),
                root_id: root.id,
                started_at,
                completed_at: Some(completed_at),
                status: ScanStatus::Failed,
                additions: 0,
                modifications: 0,
                unchanged: 0,
                deletions: 0,
                skipped: 0,
                failures: 0,
                segments_indexed: 0,
                error_code: Some("DISCOVERY_FAILED".into()),
            };
            insert_scan_run(connection, &run)?;
            return Ok(RootIndexReport {
                root_id: root.id.to_string(),
                root_name: root.display_name.clone(),
                mode: IndexMode::Full,
                requested_base,
                resolved_base,
                current_head,
                changed_paths: 0,
                explicit_deletions: 0,
                fallback_reason,
                files_discovered: 0,
                additions: 0,
                modifications: 0,
                unchanged: 0,
                deletions: 0,
                skipped: 0,
                failures: 0,
                segments_indexed: 0,
                failed: true,
                error_code: Some(format!("discovery failed: {error}")),
            });
        }
    };

    let mut additions = 0_u32;
    let mut modifications = 0_u32;
    let mut unchanged = 0_u32;
    let mut failures = 0_u32;
    let mut segments_indexed = 0_u32;
    let skipped = u32::try_from(discovery.skipped.len()).unwrap_or(u32::MAX);
    let mut seen_paths = HashSet::new();

    for document in &discovery.documents {
        seen_paths.insert(document.relative_path.clone());
        match process_document(connection, root, document, registry) {
            Ok(DocumentOutcome::Addition { segments }) => {
                additions += 1;
                segments_indexed += segments;
            }
            Ok(DocumentOutcome::Modification { segments }) => {
                modifications += 1;
                segments_indexed += segments;
            }
            Ok(DocumentOutcome::Unchanged) => unchanged += 1,
            Err(_) => failures += 1,
        }
    }

    let mut deletions = 0_u32;
    let active = list_active_source_files(connection, &root.id)?;
    let deleted_at = Timestamp::now()?;
    for source in active {
        if !seen_paths.contains(&source.relative_path) {
            mark_source_deleted(connection, &source, deleted_at)?;
            deletions += 1;
        }
    }

    let completed_at = Timestamp::now()?;
    let run = ScanRun {
        id: ScanRunId::new(),
        root_id: root.id,
        started_at,
        completed_at: Some(completed_at),
        status: ScanStatus::Completed,
        additions,
        modifications,
        unchanged,
        deletions,
        skipped,
        failures,
        segments_indexed,
        error_code: None,
    };
    insert_scan_run(connection, &run)?;

    Ok(RootIndexReport {
        root_id: root.id.to_string(),
        root_name: root.display_name.clone(),
        mode: IndexMode::Full,
        requested_base,
        resolved_base,
        current_head,
        changed_paths: 0,
        explicit_deletions: 0,
        fallback_reason,
        files_discovered: u32::try_from(discovery.documents.len()).unwrap_or(u32::MAX),
        additions,
        modifications,
        unchanged,
        deletions,
        skipped,
        failures,
        segments_indexed,
        failed: false,
        error_code: None,
    })
}

enum DocumentOutcome {
    Addition { segments: u32 },
    Modification { segments: u32 },
    Unchanged,
}

#[allow(clippy::too_many_lines)]
fn process_document(
    connection: &mut Connection,
    root: &Root,
    document: &DiscoveredDocument,
    registry: &ParserRegistry,
) -> Result<DocumentOutcome, IndexError> {
    let now = Timestamp::now()?;
    let existing = find_source_file(connection, &root.id, &document.relative_path)?;
    let source_id = existing
        .as_ref()
        .map_or_else(SourceFileId::new, |source| source.id);
    let first_seen_at = existing.as_ref().map_or(now, |source| source.first_seen_at);
    let previous_revision = existing
        .as_ref()
        .and_then(|source| source.current_revision_id)
        .map(|id| crate::storage::get_revision(connection, &id))
        .transpose()?
        .flatten();

    let stable = read_stable_file(&document.canonical_path, DEFAULT_MAX_FILE_SIZE_BYTES)?;
    let content_hash = blake3_hex(&stable.bytes);
    let parser = registry
        .select(document)
        .ok_or_else(|| IndexError::Internal("no parser selected for discovered document".into()))?;

    if let Some(previous) = &previous_revision
        && previous.content_hash == content_hash
        && previous.parser_id == parser.parser_id()
        && previous.parser_version == parser.parser_version()
        && previous.status == RevisionStatus::Indexed
    {
        let mut source = existing.expect("unchanged path requires existing source");
        source.size_bytes = stable.size_bytes;
        source.modified_at = Some(stable.modified_at);
        source.last_seen_at = now;
        source.state = SourceState::Active;
        source.canonical_path_hash = path_hash(&document.canonical_path);
        upsert_source_file(connection, &source)?;
        return Ok(DocumentOutcome::Unchanged);
    }

    let parsed = match parser.parse(&SourceDocument {
        discovered: document,
        bytes: &stable.bytes,
    }) {
        Ok(parsed) => parsed,
        Err(error) => {
            record_parse_failure(
                connection,
                existing.as_ref(),
                root,
                document,
                source_id,
                first_seen_at,
                now,
                &content_hash,
                parser,
                &error,
            )?;
            return Err(IndexError::Internal(error.to_string()));
        }
    };

    let revision_id = RevisionId::new();
    let mut extracted = String::new();
    let mut segments = Vec::with_capacity(parsed.segments.len());
    for item in &parsed.segments {
        extracted.push_str(&item.text);
        extracted.push('\n');
        segments.push(Segment {
            id: SegmentId::new(),
            revision_id,
            segment_type: item.segment_type,
            anchor: item.anchor.clone(),
            ordinal: item.ordinal,
            text: item.text.clone(),
            text_hash: blake3_hex(item.text.as_bytes()),
            token_count: None,
            metadata: serde_json::json!({}),
            sensitivity_scope: None,
        });
    }

    let revision = Revision {
        id: revision_id,
        source_file_id: source_id,
        content_hash: content_hash.clone(),
        parser_id: parser.parser_id().to_owned(),
        parser_version: parser.parser_version().to_owned(),
        extracted_text_hash: Some(blake3_hex(extracted.as_bytes())),
        observed_at: now,
        indexed_at: Some(now),
        status: RevisionStatus::Indexed,
        error_code: None,
        error_message: None,
    };

    let source = SourceFile {
        id: source_id,
        root_id: root.id,
        relative_path: document.relative_path.clone(),
        canonical_path_hash: path_hash(&document.canonical_path),
        file_type: document.file_type,
        size_bytes: stable.size_bytes,
        modified_at: Some(stable.modified_at),
        current_revision_id: existing.as_ref().and_then(|item| item.current_revision_id),
        state: SourceState::Active,
        first_seen_at,
        last_seen_at: now,
    };
    upsert_source_file(connection, &source)?;
    promote_revision(connection, &source, &revision, &segments)?;

    let segment_count = u32::try_from(segments.len()).unwrap_or(u32::MAX);
    if previous_revision.is_none() {
        Ok(DocumentOutcome::Addition {
            segments: segment_count,
        })
    } else {
        Ok(DocumentOutcome::Modification {
            segments: segment_count,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn record_parse_failure(
    connection: &mut Connection,
    existing: Option<&SourceFile>,
    root: &Root,
    document: &DiscoveredDocument,
    source_id: SourceFileId,
    first_seen_at: Timestamp,
    now: Timestamp,
    content_hash: &ContentHash,
    parser: &dyn DocumentParser,
    error: &ParseError,
) -> Result<(), IndexError> {
    let source = SourceFile {
        id: source_id,
        root_id: root.id,
        relative_path: document.relative_path.clone(),
        canonical_path_hash: path_hash(&document.canonical_path),
        file_type: document.file_type,
        size_bytes: document.size_bytes,
        modified_at: Some(document.modified_at),
        current_revision_id: existing.and_then(|item| item.current_revision_id),
        state: if existing.and_then(|item| item.current_revision_id).is_some() {
            SourceState::Active
        } else {
            SourceState::Error
        },
        first_seen_at,
        last_seen_at: now,
    };
    upsert_source_file(connection, &source)?;
    connection
        .execute(
            "INSERT INTO revisions(
            id, source_file_id, content_hash, parser_id, parser_version, extracted_text_hash,
            observed_at_ms, indexed_at_ms, status, error_code, error_message
        ) VALUES (?1,?2,?3,?4,?5,NULL,?6,NULL,?7,?8,?9)",
            rusqlite::params![
                RevisionId::new().to_string(),
                source_id.to_string(),
                content_hash.as_str(),
                parser.parser_id(),
                parser.parser_version(),
                now.as_millis(),
                RevisionStatus::Failed.as_str(),
                parse_error_code(error),
                "parser failed without promoting revision",
            ],
        )
        .map_err(StorageError::from)?;
    let _ = root;
    let _ = document;
    Ok(())
}

fn parse_error_code(error: &ParseError) -> &'static str {
    match error {
        ParseError::InvalidUtf8 => "INVALID_UTF8",
        ParseError::DuplicateParser(_) => "DUPLICATE_PARSER",
        ParseError::Failed(_) => "PARSE_FAILED",
    }
}

/// Convenience: convert optional root filter text into a root ID.
///
/// # Errors
///
/// Returns invalid identifier errors.
pub fn parse_root_filter(value: Option<&str>) -> Result<Option<RootId>, IndexError> {
    value.map(str::parse).transpose().map_err(IndexError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, add_root, init_installation};
    use crate::paths::AppPaths;
    use crate::storage::{open_database, status_snapshot};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn prepared_env() -> (TempDir, AppPaths, AppConfig, Connection, PathBuf) {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("a.md"), "# Title\n\nHello world.\n").unwrap();
        fs::write(notes.join("b.txt"), "plain text body\n").unwrap();
        add_root(&mut config, &notes, Some("notes".into())).unwrap();
        config.save(&paths.config_file).unwrap();
        let db_path = config.database_path().unwrap();
        let connection = open_database(&db_path).unwrap();
        (temp, paths, config, connection, notes)
    }

    #[test]
    fn indexes_mixed_corpus_and_is_unchanged_on_second_pass() {
        let (_temp, _paths, config, mut connection, _notes) = prepared_env();
        let first = index_roots(&mut connection, &config, None).unwrap();
        assert_eq!(first.files_discovered, 2);
        assert_eq!(first.additions, 2);
        assert_eq!(first.failures, 0);
        assert!(first.segments_indexed >= 2);
        let second = index_roots(&mut connection, &config, None).unwrap();
        assert_eq!(second.additions, 0);
        assert_eq!(second.modifications, 0);
        assert_eq!(second.unchanged, 2);
        let status = status_snapshot(&connection, &config.database_path().unwrap()).unwrap();
        assert_eq!(status.active_source_files, 2);
        assert!(status.fts_rows >= 2);
        assert_eq!(status.fts_rows, status.active_segments);
    }

    #[test]
    fn modified_and_deleted_files_update_state() {
        let (_temp, _paths, config, mut connection, notes) = prepared_env();
        index_roots(&mut connection, &config, None).unwrap();
        fs::write(notes.join("a.md"), "# Title\n\nChanged.\n").unwrap();
        fs::remove_file(notes.join("b.txt")).unwrap();
        let report = index_roots(&mut connection, &config, None).unwrap();
        assert_eq!(report.modifications, 1);
        assert_eq!(report.deletions, 1);
        let status = status_snapshot(&connection, &config.database_path().unwrap()).unwrap();
        assert_eq!(status.active_source_files, 1);
    }
}
