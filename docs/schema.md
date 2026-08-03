# SQLite schema

## Versions

| Version | Migration | Notes |
|---:|---|---|
| 1 | `0001_initial.sql` | foundation tables |
| 2 | `0002_operational_indexing.sql` | timestamps, sensitivity, scan runs, FTS5 |
| 3 | `0003_m3_incremental_snapshots.sql` | root Git state, snapshots, maps, query activity |
| 4 | `0004_embedding_foundation.sql` | spaces, vector cache, segment links, failures, sync history |

Current executable schema version: **4**.

Migration `0001` is never rewritten. Later migrations are additive. Future schema versions are rejected. Snapshot **format** version is independent (format v1).

Failed migrations are not marked complete. Foreign keys are enabled at the connection boundary.

Embedding spaces are unique across provider, canonical model, digest, dimensions, normalization, and input-contract version. Cache identity is `(embedding_space_id, text_hash)`; vector length must equal `dimensions * 4`. Segment links require that exact cache key through a composite foreign key. Failure messages are bounded. Fresh and v1/v2/v3 databases migrate transactionally to v4.

## Tables

### `roots`

Opaque ID, canonical path, display name, include/exclude JSON, sensitivity JSON, symlink flag, enabled flag, created/updated timestamps, configuration fingerprint.

### `source_files`

Root-relative identity, path hash, file type, size, source mtime, current revision pointer, state, first/last seen timestamps.

### `revisions`

Immutable content projection: content hash, parser identity/version, extracted-text hash, observed/indexed timestamps, status, safe error code/message.

### `segments`

Ordered evidence for one revision: type, anchor, ordinal, text, text hash, optional token count, metadata JSON, optional sensitivity scope.

### `scan_runs`

Per-root scan counters and completion status for operational reporting. Not a general job system.

### `segments_fts`

FTS5 virtual table with segment text and unindexed identity columns. Maintained explicitly so only active revisions are searchable.

### `root_git_state` (v3)

Per-root Git fingerprint, last successfully indexed commit, observed head, and last incremental base timestamps. Advanced only after successful full or incremental indexing (including successful full fallback). Not advanced after partial failure.

### `snapshots` / `snapshot_root_maps` (v3)

Registry of imported portable payloads: id, logical name, format version, import time, managed payload path, manifest JSON, checksum. Maps bind snapshot root IDs to local root IDs. Managed payloads live under the data directory `snapshots/` and are never source trees.

### `query_activity` (v3)

Bounded samples of query timing/mode/result counts **without** query strings or source text, for local status views.

## Compatibility

Upgrade path: empty / v1 / v2 → migrate to v3. Downgrades are unsupported. Derived databases may be rebuilt when needed.
Milestone 4B requires no migration: schema version 4 already provides embedding spaces, normalized vector cache rows, and active segment references. Query text, query hashes, query vectors, and retrieval results are deliberately not stored.
