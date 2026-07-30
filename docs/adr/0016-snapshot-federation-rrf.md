# ADR-0016: Cross-database snapshot federation via Reciprocal Rank Fusion

- Status: Accepted
- Date: 2026-07-30

## Context

Imported snapshots store active FTS rows in separate managed SQLite payloads. Raw FTS5 `bm25()` values depend on per-database corpus statistics and are **not** directly comparable across the local index and snapshot payloads.

Simply concatenating candidates and sorting by raw BM25 would be incorrect and unstable.

## Decision

1. Retrieve a bounded ranked candidate list from the local index (existing BM25 order).
2. Retrieve a bounded ranked list from each eligible snapshot payload (max 8 snapshots per query, 32 candidates each).
3. Combine lists with Reciprocal Rank Fusion:

   ```text
   score = Σ 1 / (k + rank)   with k = 60
   ```

4. Key exact-duplicate suppression by `text_hash`. Local current evidence wins exact duplicates; snapshot-only and differing content remain visible.
5. Retain per-index `raw_bm25` and public local BM25 mapping in signals; expose the final RRF value as `federation_score`.
6. Local-only queries (no eligible snapshots) keep the pre-federation BM25 public score path.
7. Stable tie-breakers: relative path, anchor, segment id.
8. Snapshot hits report `EvidenceOrigin::Snapshot { snapshot_id, snapshot_root_id }` and freshness `unknown`.

## Eligibility

A snapshot contributes only when registered, managed payload exists and is readable read-only, every needed root is mapped, the mapped local root exists and is enabled, query root/file-type filters allow it, and sensitivity rules allow the relative path.

Unhealthy payloads produce bounded warnings and are skipped without failing the whole query.

## Consequences

- Federation ranking differs from raw local BM25 when snapshots participate.
- Import volume cannot create unbounded query work (hard caps).
- Evaluation remains local-only by default (no imported snapshots in the isolated evaluator).
