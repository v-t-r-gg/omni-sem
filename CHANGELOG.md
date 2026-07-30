# Changelog

All notable changes will be documented here. The format follows Keep a Changelog; the project does not yet promise semantic-version compatibility.

## Unreleased

### Added

- Milestone 3: Git-aware `omnisem index --since`, snapshot export/import, and loopback `omnisem status --serve`.
- Schema v3 tables for Git base state, snapshot registry, and query-activity samples (no query text).
- ADR-0015 snapshot format v1.

### Fixed (Milestone 2)

- Public BM25 score mapping (`-raw`), freshness-based stale-result rate, content-stable index fingerprints.

### Previously

- Milestone 1 operational indexing and Milestone 2 lexical retrieval/evaluation.
