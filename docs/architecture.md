# Architecture

## Crates

- `omnisem-core`: domain, config, discovery, parsers, indexing, retrieval, evaluation.
- `omnisem-cli`: command surface, output formatting, exit-code mapping.

## Operational pipeline

```text
configuration + approved roots
    → discovery/parse/index (immutable revisions, active FTS)
    → lexical query (safe MATCH)
    → BM25 ranking + filters
    → sensitivity filter + dedupe
    → freshness metadata inspection
    → token-budget packing
    → human/JSON output
```

## Lexical ranking

FTS5 `bm25(segments_fts)` is lower-is-better and typically negative. Public `score` is `-raw_bm25` so higher is better. This is **not** normalized to `[0, 1]` and is not comparable with future semantic or hybrid scores. Raw BM25 remains in `RetrievalSignals.raw_bm25`. Tie-breakers: relative path, anchor, segment id ascending. Candidates capped at `min(limit*8, 200)`.

## Token estimation

Heuristic: `ceil(char_count / 3)` plus fixed response/result overhead. Conservative, not model-exact.

## Evaluation

`omnisem eval` materializes `evals/corpus.jsonl` into a temporary root, indexes with production code, runs judged queries, and emits aggregate metrics without touching the user index.

## Security

Active revisions only, approved roots only, relative paths in results, safe FTS construction, no source logging, sensitivity filtering before packing, isolated evaluation databases.
