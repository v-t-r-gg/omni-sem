# SQLite schema

## Versions

| Version | Migration | Notes |
|---:|---|---|
| 1 | `0001_initial.sql` | foundation tables |
| 2 | `0002_operational_indexing.sql` | timestamps, sensitivity, scan runs, FTS5 |

Current executable schema version: **2**.

Migration `0001` is never rewritten. `0002` uses `ALTER TABLE` and creates `scan_runs` plus `segments_fts`. Future schema versions are rejected.

Failed migrations are not marked complete. Foreign keys are enabled at the connection boundary.

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

## Compatibility

Upgrade path: empty DB or v1 DB → migrate to v2. Downgrades are unsupported. Derived databases may be rebuilt when needed.
