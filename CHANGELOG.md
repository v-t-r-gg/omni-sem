# Changelog

All notable changes will be documented here. The format follows Keep a Changelog; the project does not yet promise semantic-version compatibility.

## Unreleased

### Added

- Milestone 2 lexical retrieval: safe query parsing, FTS5 BM25 ranking, match explanations, freshness status, sensitivity-aware CLI filtering, duplicate suppression, token budgeting, and context packing.
- Named budget presets (`small`, `standard`, `large`) in configuration.
- `omnisem query` with `--mode`, `--root`, `--file-type`, `--limit`, `--token-budget`, `--budget`, `--include-sensitive`, `--explain`, and `--json`.
- `omnisem eval` isolated evaluation runner with Recall@k, MRR, nDCG, diversity, duplicate/stale/misleading rates, and p50/p95 latency.
- Retrieval domain contract extensions and documentation.

### Previously

- Milestone 1 operational indexing CLI, schema v2, FTS5 active surface, discovery, and parsers.
