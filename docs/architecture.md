# Architecture

## Crates

- `omnisem-core`: domain, config, discovery, parsers, indexing, retrieval, evaluation.
- `omnisem-cli`: command surface, output formatting, exit-code mapping.

## Operational pipeline

```text
configuration + approved roots
    → discovery/parse/index (immutable revisions, active FTS)
         ↳ optional Git changed-path selection (same path policy as discovery)
         ↳ optional post-commit embedding sync (digest-isolated space + text-hash cache)
    → lexical query (safe MATCH) on local active FTS
    → optional snapshot federation (read-only payloads, RRF)
    → BM25 / federation ranking + filters
    → sensitivity filter + exact-hash dedupe (local wins)
    → freshness metadata inspection (snapshot freshness = unknown)
    → token-budget packing
    → human/JSON output with EvidenceOrigin
```

## Lexical ranking

FTS5 `bm25(segments_fts)` is lower-is-better and typically negative. Public `score` is `-raw_bm25` so higher is better. This is **not** normalized to `[0, 1]` and is not comparable with future semantic or hybrid scores. Raw BM25 remains in `RetrievalSignals.raw_bm25`. Tie-breakers: relative path, anchor, segment id ascending. Candidates capped at `min(limit*8, 200)`.

When imported snapshots are eligible, lists are fused with Reciprocal Rank Fusion (`k=60`). Final score becomes the RRF federation score; raw BM25 is retained per channel. Local-only queries keep the BM25 public score path.

## Snapshots

Directory format v1 (`MANIFEST.json` + `payload.sqlite3`). Import is compensating-atomic. Lifecycle: list / inspect / remove. Queryability requires complete explicit root maps. See ADR-0015, ADR-0016, ADR-0017.

Format v1 remains lexical-only: local embedding spaces, vectors, references, failures, and sync runs are never exported.

## Embedding materialization

The synchronous `EmbeddingProvider` boundary contains no Ollama transport types. Lexical indexing commits first; enabled synchronization then resolves an exact model digest, derives a deterministic compatibility-space ID, links cache hits, and calls the provider outside SQLite transactions. Cache persistence and segment linking are transactional per successful batch. Provider failure cannot roll back a lexical revision. Semantic querying is deferred to Milestone 4B.

Persisted cache hits are loaded, dimension-checked, decoded, and verified as already L2-normalized before linkage. Fresh provider batches are count/dimension/value validated and normalized in memory before any database mutation. Malformed batches become bounded partial failures rather than storage failures.

## Token estimation

Heuristic: `ceil(char_count / 3)` plus fixed response/result overhead. Conservative, not model-exact.

## Evaluation

`omnisem eval` materializes `evals/corpus.jsonl` into a temporary root, indexes with production code, runs judged queries, and emits aggregate metrics without touching the user index. The isolated evaluator does not import snapshots by default.

## Security

Active revisions only, approved roots only, relative paths in results, safe FTS construction, no source logging, sensitivity filtering before packing, isolated evaluation databases. Incremental indexing shares discovery validation. Status HTTP is loopback-only, method-aware, and omits source/query text.
## Milestone 4B retrieval runtime

The production `retrieve` wrapper preserves a provider-free lexical path. Semantic-capable calls construct only the explicitly configured provider and invoke `retrieve_with_runtime`, whose provider and `VectorSearch` boundaries are injectable for deterministic tests. Model resolution and query embedding are synchronous and occur without an open SQLite transaction. Query vectors exist only on the stack/heap for one invocation.

The schema-v4 exact scanner joins segment references to current revisions, active files, and enabled roots, then applies request and sensitivity filters before ranking. Hybrid fusion accepts local lexical, per-snapshot lexical, and local semantic ranked lists in one RRF pass.
