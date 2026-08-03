# ADR-0020: Exact semantic search and one-pass hybrid fusion

Status: accepted

Milestone 4B keeps query embeddings transient and verifies the configured provider, canonical model, digest, dimensions, L2 normalization, segment input contract, and positive active coverage against the persisted active space before embedding. A mutable tag resolving to a new digest fails with `EMBEDDING_MODEL_CHANGED` and directs the user to index; queries never create spaces or backfill.

The baseline scanner decodes schema-v4 little-endian normalized `f32` vectors and computes dot products in `f64`. It scans at most 50,000 vectors in the complete compatible active space, retains at most 200 candidates, and uses path, anchor, and segment ID tie-breakers. The scan bound intentionally applies before request root, file-type, and sensitivity filters: this conservative policy prevents a large corpus from bypassing the exact-scan safety boundary through narrow queries. Corrupt candidates are excluded; exhausting valid evidence is a typed failure. No ANN dependency, SQLite extension, or schema migration is introduced. Reconsider ANN when measured corpora regularly approach the scan bound or latency objectives fail.

Hybrid performs one RRF pass (`1 / (60 + rank)`) across local FTS, each eligible snapshot FTS list, and local exact-vector results, admitting at most 768 candidates. Exact text-hash duplicates collapse and local evidence wins snapshot duplicates. Raw BM25 and cosine remain diagnostics; only RRF is the hybrid score. Format-1 snapshots remain lexical-only.

Lexical never constructs a provider. Explicit semantic and hybrid fail if their semantic channel is unavailable. Auto chooses lexical when disabled or incompatible and falls back with a bounded warning after provider failure; after explicit enablement, auto may contact the configured provider.

Provider inputs carry a provider-neutral purpose (`segment` or transient `query`). Segment materialization remains `segment-text-v1`; query input is `query-text-v1`. The purpose does not alter Ollama's external JSON shape. Semantic compatibility directly checks configured dimensions when nonzero before model resolution or vector scanning.

Retrieval telemetry separately measures query embedding and exact scanning with a monotonic clock and reports active vectors examined, corrupt exclusions, per-channel candidate counts, fusion admissions, unique fused results, and exact duplicate suppression. It contains no query content or provider transport details.
