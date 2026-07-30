# ADR-0015: Portable derived-data snapshot format v1

- Status: Accepted
- Date: 2026-07-30
- Updated: 2026-07-30 (Milestone 3 corrective pass)

## Context and decision

Milestone 3 needs portable read-only index transfer without shipping original source files as separate artifacts. Use a directory snapshot containing:

- `MANIFEST.json` — format version, schema compatibility range, checksum, root descriptors, empty embedding-space list, declared capabilities, sensitivity warning;
- `payload.sqlite3` — sanitized active-only tables (logical root IDs, relative paths, revisions, segments, FTS).

Snapshots are sensitive because they contain indexed segment text.

A tar archive is not required for format v1. Directory export keeps symlink-safe tree inspection simple.

## Import, mapping, and queryability

- Import validates the tree (no symlinks, only expected regular files, size/entry limits), manifest compatibility, payload integrity, relationship invariants, and portable relative paths.
- Registration requires explicit `--map SNAPSHOT_ROOT_ID=LOCAL_ROOT_ID` for every snapshot root.
- Mapping never approves a local path and never infers by display name.
- Local roots must already exist and be enabled.
- Duplicate snapshot-root or local-root mappings are rejected (one snapshot root → one local root).
- Atomicity is compensate-not-single-TX: copy to a unique temp file under the managed snapshots directory → validate → transactionally register DB rows → rename into the final managed name; cleanup on any failure. Never overwrite an existing managed payload.

Imported snapshots become **queryable only after complete explicit mapping** and a healthy managed payload. They do not become visible solely because registration succeeded without maps.

## Retrieval federation

Eligible snapshot payloads are opened SQLite read-only during query. Cross-database ranking uses Reciprocal Rank Fusion (RRF) over bounded per-index candidate lists because raw FTS5 BM25 is not comparable across corpora. Local current evidence wins exact `text_hash` duplicates. Snapshot freshness is always `unknown` unless an explicit content-hash compare design is added later.

See ADR-0016 for federation ranking details.

## Lifecycle

```text
omnisem snapshot list|inspect|remove SNAPSHOT_ID
```

Remove deletes DB mappings/registration transactionally, then deletes only the managed payload under the Omni-Sem snapshots directory. Root disablement or removal also removes snapshot retrieval eligibility for that local root.

## Alternatives and consequences

Copying the live database would leak absolute paths and operational state. Tar/zip packaging remains optional later. Schema version 3 remains sufficient for the registry tables; snapshot format version is independent (v1).
