# ADR-0015: Portable derived-data snapshot format v1

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Milestone 3 needs portable read-only index transfer without shipping original source files as separate artifacts. Use a directory snapshot containing:

- `MANIFEST.json` — format version, schema compatibility, checksum, root descriptors, empty embedding-space list, sensitivity warning;
- `payload.sqlite3` — sanitized active-only tables (logical root IDs, relative paths, revisions, segments, FTS).

Snapshots are sensitive because they contain indexed segment text.

## Alternatives and consequences

Copying the live database would leak absolute paths and operational state. Tar/zip packaging is optional later; directory export keeps validation simple. Imported payloads register only after explicit root mapping and never become queryable solely by import.