# ADR-0014: BLAKE3 content hashing with algorithm prefixes

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Content and text digests use BLAKE3 and serialize as `blake3:<hex>`. The prefix keeps digests self-describing if the algorithm changes later.

## Alternatives and consequences

SHA-256 is ubiquitous but slower for bulk local indexing. Unprefixed hex is shorter but ambiguous across migrations. BLAKE3 is pure Rust (with optional SIMD), has no native SQLite dependency, and keeps hashing independent of the storage engine.
