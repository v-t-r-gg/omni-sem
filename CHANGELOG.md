# Changelog

All notable changes will be documented here. The format follows Keep a Changelog; the project does not yet promise semantic-version compatibility.

## Unreleased

### Added

- Operational Milestone 1 indexing CLI: `init`, `root add|list|suggest|remove`, `index`, `status`, `changes`.
- TOML configuration with platform-aware directories, unknown-field denial, and restrictive permissions.
- Schema v2 migration: timestamps, sensitivity storage, scan runs, and active-only FTS5.
- BLAKE3 content hashing (`blake3:<hex>`), stable bounded reads, immutable revision promotion.
- Deletion handling after successful root discovery; root revocation cascade for derived data.
- ADR-0013 (FTS active surface) and ADR-0014 (BLAKE3 hashing).
- CLI reference documentation and expanded integration tests.

### Previously

- Blueprint v0.3, discovery, Markdown/plain-text parsers, ADR-0007–0012, foundation workspace.
