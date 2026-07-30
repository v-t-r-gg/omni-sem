//! Portable read-only snapshot export and import.
//!
//! Snapshots contain derived index text and must be treated as sensitive.
//! They never include original source files as filesystem artifacts, absolute
//! root paths, secrets, logs, or query text.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::domain::{RootId, Timestamp};
use crate::error::ConfigError;
use crate::hash::blake3_hex;
use crate::paths::restrict_permissions;
use crate::storage::StorageError;

/// Snapshot format version (independent of database schema version).
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;
/// Maximum payload size accepted on import (64 MiB).
pub const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

/// Manifest stored beside the sanitized payload database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotManifest {
    pub snapshot_format_version: u32,
    pub omnisem_version: String,
    pub schema_compatibility: SchemaCompatibility,
    pub created_at_ms: i64,
    pub payload_checksum: String,
    pub roots: Vec<SnapshotRootDescriptor>,
    pub counts: SnapshotCounts,
    pub embedding_spaces: Vec<String>,
    pub capabilities: Vec<String>,
    pub warning: String,
}

/// Database schema range required to read the payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaCompatibility {
    pub min: i64,
    pub max: i64,
}

/// Portable root descriptor without absolute source paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotRootDescriptor {
    pub snapshot_root_id: String,
    pub display_name: String,
    pub source_count: i64,
    pub segment_count: i64,
}

/// Aggregate payload counts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotCounts {
    pub sources: i64,
    pub revisions: i64,
    pub segments: i64,
    pub fts_rows: i64,
}

/// Result of exporting a snapshot archive directory.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotExportReport {
    pub path: String,
    pub payload_checksum: String,
    pub roots: usize,
    pub segments: i64,
}

/// Result of importing a snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotImportReport {
    pub snapshot_id: String,
    pub mapped_roots: Vec<String>,
    pub segments: i64,
}

/// Exports active index data for selected roots into `destination` directory.
///
/// # Errors
///
/// Returns configuration or storage failures. Refuses to overwrite an existing path.
pub fn export_snapshot(
    connection: &Connection,
    destination: &Path,
    only_root: Option<&RootId>,
) -> Result<SnapshotExportReport, ConfigError> {
    if destination.exists() {
        return Err(ConfigError::Invalid {
            path: destination.to_path_buf(),
            message: "snapshot destination already exists".into(),
        });
    }
    let temp = destination.with_extension("tmp-export");
    if temp.exists() {
        let _ = fs::remove_dir_all(&temp);
    }
    fs::create_dir_all(&temp).map_err(|error| ConfigError::Io {
        path: temp.clone(),
        message: error.to_string(),
    })?;

    let payload_path = temp.join("payload.sqlite3");
    let mut payload = Connection::open(&payload_path).map_err(|error| ConfigError::Io {
        path: payload_path.clone(),
        message: error.to_string(),
    })?;
    build_payload(connection, &mut payload, only_root).map_err(|error| ConfigError::Io {
        path: payload_path.clone(),
        message: error.to_string(),
    })?;
    drop(payload);

    let bytes = fs::read(&payload_path).map_err(|error| ConfigError::Io {
        path: payload_path.clone(),
        message: error.to_string(),
    })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SNAPSHOT_BYTES {
        let _ = fs::remove_dir_all(&temp);
        return Err(ConfigError::Invalid {
            path: destination.to_path_buf(),
            message: "snapshot payload exceeds size limit".into(),
        });
    }
    let checksum = blake3_hex(&bytes).0;
    let roots = list_payload_roots(&payload_path)?;
    let counts = count_payload(&payload_path)?;
    let manifest = SnapshotManifest {
        snapshot_format_version: SNAPSHOT_FORMAT_VERSION,
        omnisem_version: env!("CARGO_PKG_VERSION").into(),
        schema_compatibility: SchemaCompatibility { min: 2, max: 3 },
        created_at_ms: Timestamp::now().map_or(0, Timestamp::as_millis),
        payload_checksum: checksum.clone(),
        roots: roots.clone(),
        counts: counts.clone(),
        embedding_spaces: Vec::new(),
        capabilities: vec!["lexical_fts5".into(), "read_only_retrieval".into()],
        warning: "This snapshot contains derived indexed text and may include substantially all approved corpus content. Treat it as sensitive.".into(),
    };
    let manifest_path = temp.join("MANIFEST.json");
    let manifest_json =
        serde_json::to_string_pretty(&manifest).map_err(|error| ConfigError::Invalid {
            path: manifest_path.clone(),
            message: error.to_string(),
        })?;
    fs::write(&manifest_path, manifest_json).map_err(|error| ConfigError::Io {
        path: manifest_path.clone(),
        message: error.to_string(),
    })?;
    restrict_permissions(&payload_path)?;
    restrict_permissions(&manifest_path)?;
    fs::rename(&temp, destination).map_err(|error| ConfigError::Io {
        path: destination.to_path_buf(),
        message: error.to_string(),
    })?;
    restrict_permissions(destination)?;
    Ok(SnapshotExportReport {
        path: destination.display().to_string(),
        payload_checksum: checksum,
        roots: roots.len(),
        segments: counts.segments,
    })
}

/// Imports a snapshot directory after validation and explicit root mapping.
///
/// Mapping entries use `SNAPSHOT_ROOT_ID=LOCAL_ROOT_ID`.
///
/// # Errors
///
/// Returns validation failures. Invalid imports leave no registered partial state.
#[allow(clippy::too_many_lines)]
pub fn import_snapshot(
    connection: &mut Connection,
    snapshot_dir: &Path,
    maps: &[(String, String)],
) -> Result<SnapshotImportReport, ConfigError> {
    validate_snapshot_tree(snapshot_dir)?;
    let manifest = read_manifest(snapshot_dir)?;
    if manifest.snapshot_format_version != SNAPSHOT_FORMAT_VERSION {
        return Err(ConfigError::Invalid {
            path: snapshot_dir.to_path_buf(),
            message: format!(
                "unsupported snapshot format {}",
                manifest.snapshot_format_version
            ),
        });
    }
    if !manifest.embedding_spaces.is_empty() {
        return Err(ConfigError::Invalid {
            path: snapshot_dir.to_path_buf(),
            message: "snapshot declares unknown embedding spaces".into(),
        });
    }
    let payload = snapshot_dir.join("payload.sqlite3");
    let bytes = fs::read(&payload).map_err(|error| ConfigError::Io {
        path: payload.clone(),
        message: error.to_string(),
    })?;
    if blake3_hex(&bytes).0 != manifest.payload_checksum {
        return Err(ConfigError::Invalid {
            path: payload,
            message: "payload checksum mismatch".into(),
        });
    }
    if maps.is_empty() {
        return Err(ConfigError::Invalid {
            path: snapshot_dir.to_path_buf(),
            message: "snapshot import requires explicit --map SNAPSHOT_ROOT=LOCAL_ROOT".into(),
        });
    }
    for (snap_root, local_root) in maps {
        if !manifest
            .roots
            .iter()
            .any(|root| root.snapshot_root_id == *snap_root)
        {
            return Err(ConfigError::Invalid {
                path: snapshot_dir.to_path_buf(),
                message: format!("unknown snapshot root id {snap_root}"),
            });
        }
        let _ = local_root
            .parse::<RootId>()
            .map_err(|_| ConfigError::RootNotFound(local_root.clone()))?;
    }

    let snapshot_id = uuid::Uuid::new_v4().to_string();
    let data_dir = snapshot_dir
        .parent()
        .unwrap_or(snapshot_dir)
        .join(format!("imported-{snapshot_id}"));
    // Store payload under the DB parent/snapshots
    let store_dir = PathBuf::from("."); // replaced by caller via open path parent

    // Persist registration only; visibility requires maps.
    let tx = connection.transaction().map_err(|error| ConfigError::Io {
        path: snapshot_dir.to_path_buf(),
        message: error.to_string(),
    })?;
    // Copy payload next to main db if possible.
    let main_path: String = tx
        .query_row("PRAGMA database_list", [], |row| row.get::<_, String>(2))
        .unwrap_or_default();
    let store_parent = Path::new(&main_path)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        .join("snapshots");
    fs::create_dir_all(&store_parent).map_err(|error| ConfigError::Io {
        path: store_parent.clone(),
        message: error.to_string(),
    })?;
    let stored_payload = store_parent.join(format!("{snapshot_id}.sqlite3"));
    fs::copy(&payload, &stored_payload).map_err(|error| ConfigError::Io {
        path: stored_payload.clone(),
        message: error.to_string(),
    })?;
    restrict_permissions(&stored_payload)?;
    let manifest_json = serde_json::to_string(&manifest).map_err(|error| ConfigError::Invalid {
        path: snapshot_dir.to_path_buf(),
        message: error.to_string(),
    })?;
    tx.execute(
        "INSERT INTO snapshots(id, logical_name, format_version, imported_at_ms, payload_path, manifest_json, checksum)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            snapshot_id,
            "imported",
            i64::from(SNAPSHOT_FORMAT_VERSION),
            Timestamp::now().map_or(0, Timestamp::as_millis),
            stored_payload.display().to_string(),
            manifest_json,
            manifest.payload_checksum,
        ],
    )
    .map_err(|error| ConfigError::Io {
        path: snapshot_dir.to_path_buf(),
        message: error.to_string(),
    })?;
    for (snap_root, local_root) in maps {
        tx.execute(
            "INSERT INTO snapshot_root_maps(snapshot_id, snapshot_root_id, local_root_id)
             VALUES (?1,?2,?3)",
            params![snapshot_id, snap_root, local_root],
        )
        .map_err(|error| ConfigError::Io {
            path: snapshot_dir.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    tx.commit().map_err(|error| ConfigError::Io {
        path: snapshot_dir.to_path_buf(),
        message: error.to_string(),
    })?;
    let _ = data_dir;
    let _ = store_dir;
    Ok(SnapshotImportReport {
        snapshot_id,
        mapped_roots: maps
            .iter()
            .map(|(snap, local)| format!("{snap}->{local}"))
            .collect(),
        segments: manifest.counts.segments,
    })
}

fn validate_snapshot_tree(path: &Path) -> Result<(), ConfigError> {
    if !path.is_dir() {
        return Err(ConfigError::NotDirectory(path.to_path_buf()));
    }
    let mut entries = 0_u32;
    for entry in fs::read_dir(path).map_err(|error| ConfigError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })? {
        let entry = entry.map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        entries += 1;
        if entries > 8 {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                message: "snapshot contains too many entries".into(),
            });
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), "MANIFEST.json" | "payload.sqlite3") {
            let meta = entry.metadata().map_err(|error| ConfigError::Io {
                path: entry.path(),
                message: error.to_string(),
            })?;
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(ConfigError::Invalid {
                    path: entry.path(),
                    message: "snapshot entry must be a regular file".into(),
                });
            }
            if meta.len() > MAX_SNAPSHOT_BYTES {
                return Err(ConfigError::Invalid {
                    path: entry.path(),
                    message: "snapshot entry exceeds size limit".into(),
                });
            }
            continue;
        }
        return Err(ConfigError::Invalid {
            path: entry.path(),
            message: format!("unexpected snapshot entry {name}"),
        });
    }
    if !path.join("MANIFEST.json").is_file() || !path.join("payload.sqlite3").is_file() {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            message: "snapshot missing MANIFEST.json or payload.sqlite3".into(),
        });
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<SnapshotManifest, ConfigError> {
    let text = fs::read_to_string(path.join("MANIFEST.json")).map_err(|error| ConfigError::Io {
        path: path.join("MANIFEST.json"),
        message: error.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|error| ConfigError::Invalid {
        path: path.join("MANIFEST.json"),
        message: error.to_string(),
    })
}

#[allow(clippy::too_many_lines)]
fn build_payload(
    source: &Connection,
    payload: &mut Connection,
    only_root: Option<&RootId>,
) -> Result<(), StorageError> {
    payload.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE roots(
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL
         );
         CREATE TABLE source_files(
            id TEXT PRIMARY KEY,
            root_id TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            file_type TEXT NOT NULL,
            current_revision_id TEXT,
            state TEXT NOT NULL
         );
         CREATE TABLE revisions(
            id TEXT PRIMARY KEY,
            source_file_id TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            parser_id TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            status TEXT NOT NULL
         );
         CREATE TABLE segments(
            id TEXT PRIMARY KEY,
            revision_id TEXT NOT NULL,
            segment_type TEXT NOT NULL,
            anchor TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            text TEXT NOT NULL,
            text_hash TEXT NOT NULL
         );
         CREATE VIRTUAL TABLE segments_fts USING fts5(
            text,
            segment_id UNINDEXED,
            revision_id UNINDEXED,
            source_file_id UNINDEXED,
            root_id UNINDEXED,
            relative_path UNINDEXED,
            anchor UNINDEXED
         );",
    )?;

    let mut root_sql = String::from("SELECT id, display_name FROM roots WHERE enabled = 1");
    if only_root.is_some() {
        root_sql.push_str(" AND id = ?1");
    }
    let mut statement = source.prepare(&root_sql)?;
    let mut rows = match only_root {
        Some(id) => statement.query(params![id.to_string()])?,
        None => statement.query([])?,
    };
    let mut root_ids = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        payload.execute(
            "INSERT INTO roots(id, display_name) VALUES (?1,?2)",
            params![id.clone(), name],
        )?;
        root_ids.push(id);
    }

    for root_id in &root_ids {
        // sources
        let mut st = source.prepare(
            "SELECT id, root_id, relative_path, file_type, current_revision_id, state
             FROM source_files WHERE root_id = ?1 AND state = 'active'",
        )?;
        let mut rs = st.query(params![root_id])?;
        while let Some(row) = rs.next()? {
            payload.execute(
                "INSERT INTO source_files(id, root_id, relative_path, file_type, current_revision_id, state)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ],
            )?;
        }
        // revisions for active sources
        let mut st = source.prepare(
            "SELECT r.id, r.source_file_id, r.content_hash, r.parser_id, r.parser_version, r.status
             FROM revisions r
             INNER JOIN source_files s ON s.current_revision_id = r.id
             WHERE s.root_id = ?1 AND s.state = 'active'",
        )?;
        let mut rs = st.query(params![root_id])?;
        while let Some(row) = rs.next()? {
            payload.execute(
                "INSERT INTO revisions(id, source_file_id, content_hash, parser_id, parser_version, status)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ],
            )?;
        }
        // segments + fts
        let mut st = source.prepare(
            "SELECT s.id, s.revision_id, s.segment_type, s.anchor, s.ordinal, s.text, s.text_hash,
                    sf.id, sf.root_id, sf.relative_path
             FROM segments s
             INNER JOIN revisions r ON r.id = s.revision_id
             INNER JOIN source_files sf ON sf.current_revision_id = r.id
             WHERE sf.root_id = ?1 AND sf.state = 'active'",
        )?;
        let mut rs = st.query(params![root_id])?;
        while let Some(row) = rs.next()? {
            let segment_id: String = row.get(0)?;
            let revision_id: String = row.get(1)?;
            let segment_type: String = row.get(2)?;
            let anchor: String = row.get(3)?;
            let ordinal: i64 = row.get(4)?;
            let text: String = row.get(5)?;
            let text_hash: String = row.get(6)?;
            let source_file_id: String = row.get(7)?;
            let root_id_val: String = row.get(8)?;
            let relative_path: String = row.get(9)?;
            payload.execute(
                "INSERT INTO segments(id, revision_id, segment_type, anchor, ordinal, text, text_hash)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    segment_id.clone(),
                    revision_id.clone(),
                    segment_type,
                    anchor.clone(),
                    ordinal,
                    text.clone(),
                    text_hash,
                ],
            )?;
            payload.execute(
                "INSERT INTO segments_fts(text, segment_id, revision_id, source_file_id, root_id, relative_path, anchor)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    text,
                    segment_id,
                    revision_id,
                    source_file_id,
                    root_id_val,
                    relative_path,
                    anchor,
                ],
            )?;
        }
    }
    Ok(())
}

fn list_payload_roots(path: &Path) -> Result<Vec<SnapshotRootDescriptor>, ConfigError> {
    let connection = Connection::open(path).map_err(|error| ConfigError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut statement = connection
        .prepare(
            "SELECT r.id, r.display_name,
                    (SELECT COUNT(*) FROM source_files s WHERE s.root_id = r.id),
                    (SELECT COUNT(*) FROM segments_fts f WHERE f.root_id = r.id)
             FROM roots r",
        )
        .map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    let rows = statement
        .query_map([], |row| {
            Ok(SnapshotRootDescriptor {
                snapshot_root_id: row.get(0)?,
                display_name: row.get(1)?,
                source_count: row.get(2)?,
                segment_count: row.get(3)?,
            })
        })
        .map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

fn count_payload(path: &Path) -> Result<SnapshotCounts, ConfigError> {
    let connection = Connection::open(path).map_err(|error| ConfigError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let sources: i64 = connection
        .query_row("SELECT COUNT(*) FROM source_files", [], |row| row.get(0))
        .unwrap_or(0);
    let revisions: i64 = connection
        .query_row("SELECT COUNT(*) FROM revisions", [], |row| row.get(0))
        .unwrap_or(0);
    let segments: i64 = connection
        .query_row("SELECT COUNT(*) FROM segments", [], |row| row.get(0))
        .unwrap_or(0);
    let fts_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM segments_fts", [], |row| row.get(0))
        .unwrap_or(0);
    Ok(SnapshotCounts {
        sources,
        revisions,
        segments,
        fts_rows,
    })
}

/// Parses `SNAPSHOT=LOCAL` mapping arguments.
///
/// # Errors
///
/// Returns [`ConfigError::Invalid`] when a mapping is malformed.
pub fn parse_root_maps(values: &[String]) -> Result<Vec<(String, String)>, ConfigError> {
    let mut out = Vec::new();
    for value in values {
        let Some((left, right)) = value.split_once('=') else {
            return Err(ConfigError::Invalid {
                path: PathBuf::from("cli"),
                message: format!(
                    "invalid snapshot map '{value}', expected SNAPSHOT_ROOT=LOCAL_ROOT"
                ),
            });
        };
        if left.is_empty() || right.is_empty() {
            return Err(ConfigError::Invalid {
                path: PathBuf::from("cli"),
                message: format!("invalid snapshot map '{value}'"),
            });
        }
        out.push((left.to_owned(), right.to_owned()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{add_root, init_installation};
    use crate::index::index_roots;
    use crate::paths::AppPaths;
    use crate::storage::open_database;
    use tempfile::TempDir;

    #[test]
    fn export_import_round_trip_registers_mapping() {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(
            notes.join("a.md"),
            "# Storage\n\nSQLite system of record.\n",
        )
        .unwrap();
        let root = add_root(&mut config, &notes, Some("notes".into())).unwrap();
        config.save(&paths.config_file).unwrap();
        let mut connection = open_database(&config.database_path().unwrap()).unwrap();
        index_roots(&mut connection, &config, None).unwrap();

        let dest = temp.path().join("snap");
        let report = export_snapshot(&connection, &dest, None).unwrap();
        assert!(dest.join("MANIFEST.json").is_file());
        assert!(report.segments > 0);

        let maps = vec![(root.id.clone(), root.id.clone())];
        let imported = import_snapshot(&mut connection, &dest, &maps).unwrap();
        assert!(!imported.snapshot_id.is_empty());
    }
}
