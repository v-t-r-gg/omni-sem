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

### Stale-result rate

Computed only from filesystem freshness on returned hits:

```text
stale-result rate =
    count(PendingReindex)
    / count(Current ∪ PendingReindex)
```

`Unknown` is excluded from the denominator (indeterminate, not proven stale). Path or document-id wording never contributes. A freshly materialized evaluation corpus should report `0`.

### Index fingerprint

BLAKE3 over a deterministic ordered projection of **active** segments:

`relative_path | anchor | ordinal | text_hash | content_hash | parser_id | parser_version`

Row UUIDs are omitted so equivalent corpora fingerprint identically across re-indexes. Inactive historical revisions are excluded.

## Isolation

Evaluation always builds a temporary data root and database. The user’s configured index is never modified.
