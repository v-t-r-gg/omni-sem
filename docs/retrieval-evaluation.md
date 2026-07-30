# Retrieval evaluation

## Bundle layout

```text
evals/
  corpus.jsonl
  queries.jsonl
  judgments.jsonl
  schema/
```

## Relevance grades

| Label | Grade | nDCG gain |
|---|---:|---|
| required | 2 | yes |
| acceptable | 1 | yes |
| misleading | 0 | tracked separately |
| unjudged | 0 | none |

## Metrics

- Recall@5 / Recall@10 over required anchors
- MRR over required∪acceptable
- nDCG@10 with grades above
- duplicate-result rate, stale-result rate, misleading-result rate
- mean source diversity
- mean returned tokens
- p50 / p95 latency (monotonic clock, query only)

## Isolation

Evaluation always builds a temporary data root and database. The user’s configured index is never modified.
