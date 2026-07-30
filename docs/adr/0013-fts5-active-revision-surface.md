# ADR-0013: Active-revision FTS5 with duplicated segment text

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Milestone 1 must maintain a searchable FTS5 surface that contains only active revisions. Choose an explicitly maintained FTS table that stores segment text plus unindexed identity columns (`segment_id`, `revision_id`, `source_file_id`, `root_id`, `relative_path`, `anchor`).

Promotion of a revision deletes prior FTS rows for the source's previous current revision, inserts new rows, and updates `source_files.current_revision_id` in one SQLite transaction.

## Alternatives and consequences

Contentless FTS would avoid text duplication but forces every future query path to join segments for text and complicates integrity checks. External-content FTS requires triggers tightly coupled to the segments table and is harder to reason about under partial failure. Duplicated text costs storage but keeps transactional promotion simple and makes active-only invariants local to the indexing service.

Stale revisions remain in `revisions`/`segments` history tables but never in `segments_fts` after promotion, deletion, or root revocation.
