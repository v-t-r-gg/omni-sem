# Snapshot format v1

## Layout

```text
snapshot-dir/
  MANIFEST.json
  payload.sqlite3
```

Directory export is the supported packaging. Tar is not required.

## Manifest

Required fields include format version (`1`), schema compatibility `{min,max}`, payload checksum (BLAKE3 hex), root descriptors with nonempty unique logical IDs, nonnegative counts, empty `embedding_spaces`, allowed capabilities (`lexical_fts5`, `read_only_retrieval`), and a sensitivity warning string.

Snapshot v1 remains lexical-only. Local caches and vectors are not exported, and import rejects nonempty `embedding_spaces`. Imported evidence cannot participate in semantic retrieval; portable vectors require a future format revision.

## Payload

Active-only sanitized tables: logical roots, relative source paths, revisions, segments, FTS. No absolute source paths, no operational secrets, no query text.

## Import validation (summary)

1. Symlink-safe tree inspection (`DirEntry::file_type` / `symlink_metadata`).
2. Manifest compatibility and limits.
3. Checksum match (necessary, not sufficient).
4. SQLite integrity, required tables, FK-style relationships, count match, portable paths, known file types, declared roots only.
5. Explicit complete root maps to enabled local roots.
6. Compensating registration into managed `snapshots/` (ADR-0017).

## Sensitivity

Snapshots contain derived segment text. Treat like index data: encrypt at rest if needed, do not publish publicly, delete with `omnisem snapshot remove`.
