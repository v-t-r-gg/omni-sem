# Changelog

All notable changes will be documented here. The format follows Keep a Changelog; the project does not yet promise semantic-version compatibility.

## Unreleased

### Added (Milestone 4A)

- Safe-default embedding configuration and feature-gated blocking Ollama transport using `/api/tags` and `/api/embed`.
- Schema v4 digest-aware embedding spaces, normalized binary f32 cache, active-segment backfill, failure state, and bounded sync history.
- Persisted embedding status and `omnisem doctor`; snapshot format 1 and retrieval remain lexical-only.

### Fixed (Milestone 4A corrective)

- Validate persisted vectors without renormalizing them and reject corrupt cache rows before segment linkage.
- Validate complete provider batches before writes, persist malformed-output failures as partial synchronization, and clear stale active-space selection on unknown-dimension model changes.
- Compare configured and active embedding compatibility in status and verify resolved model/digest/dimensions in doctor.
- Add transport and synchronization fault injection for timeouts, bounded responses, malformed output, batch continuation, retry, cache corruption, source changes, and transaction boundaries.

### Added

- Milestone 3 complete boundary:
  - Git-aware `omnisem index --since` with shared discovery path validation;
  - snapshot export/import with symlink-safe tree checks, payload integrity, atomic compensated registration;
  - `omnisem snapshot list|inspect|remove`;
  - federated lexical retrieval over mapped snapshots (`EvidenceOrigin`, RRF ranking, local exact-hash precedence);
  - loopback `omnisem status --serve` with GET/HEAD, 405/400/404, security headers, snapshot health.
- Schema v3 tables for Git base state, snapshot registry, and query-activity samples (no query text).
- ADR-0015 snapshot format v1; ADR-0016 snapshot federation RRF; ADR-0017 import compensation.

### Fixed (Milestone 3 corrective)

- Incremental indexing no longer bypasses include/exclude, hidden, symlink, size, and type policy.
- Non-UTF-8 Git paths abort incremental collection and force full safe discovery fallback.
- Successful full fallback advances the recorded Git base; partial failures do not.
- Snapshot import no longer leaves orphan managed payloads on registration failure.
- Status server enforces methods and request limits; does not leak internal error strings.

### Fixed (Milestone 2)

- Public BM25 score mapping (`-raw`), freshness-based stale-result rate, content-stable index fingerprints.

### Previously

- Milestone 1 operational indexing and Milestone 2 lexical retrieval/evaluation.
