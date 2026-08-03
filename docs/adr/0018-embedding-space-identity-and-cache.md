# ADR-0018: Embedding-space identity and model-aware cache

Status: accepted.

The deterministic BLAKE3 space ID covers provider kind, canonical model, resolved model digest, output dimensions, L2 normalization, and embedding-input contract version. Any change creates a new space; mutable tags resolving to new digests backfill independently without overwriting old data.

Vectors are cached by `(embedding_space_id, text_hash)` and linked separately to segments, so identical text is requested once and reused across files/revisions. Storage is normalized f32 little-endian bytes with separate dimensions. Historical derived spaces may remain and are rebuildable.
