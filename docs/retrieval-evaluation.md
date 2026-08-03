# Retrieval evaluation

Milestone 4B supports isolated lexical, semantic, hybrid, and comparison evaluation. Semantic-capable runs copy only the explicit embedding configuration into a temporary installation, materialize the fixture corpus and its vectors there, and never use the user's roots, snapshots, or database. Lexical evaluation constructs no provider.

## Bundle layout

```text
evals/
  corpus.jsonl
  queries.jsonl
  judgments.jsonl
  schema/
  semantic/
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

Evaluation always builds a temporary data root and database. The user’s configured index is never modified. Imported snapshots are **not** part of the default evaluation path; reference lexical metrics remain local-only unless a bundle explicitly includes snapshots.

`omnisem eval --compare` reports lexical, semantic, and hybrid metrics plus semantic-minus-lexical and hybrid-minus-lexical deltas. Results depend on corpus and exact model digest; the report does not assert that one mode is intrinsically superior. Provider/model cold start and corpus materialization are excluded from per-query latency and reported separately as corpus embedding time.

## Snapshot federation (runtime query, not default eval)

Production `omnisem query` may federate mapped imported snapshots:

- evidence origin is explicit (`local_index` vs `snapshot`);
- ranking across databases uses RRF (see ADR-0016), not raw multi-DB BM25;
- local exact `text_hash` duplicates suppress snapshot copies;
- snapshot freshness is `unknown`;
- unhealthy snapshot payloads are skipped with bounded warnings.

Targeted integration tests cover federation; they must not contaminate reference lexical metrics.
