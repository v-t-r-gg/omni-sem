# Retrieval evaluation contract

Evaluation separates retrieval quality from answer-model quality. Versioned synthetic corpus records live in `evals/corpus.jsonl`; queries, judgments, and machine-readable benchmark results use the JSON Schemas in `evals/schema/`.

Required retrieval measures are Recall@5, Recall@10, MRR, nDCG, duplicate-result rate, stale-revision rate, misleading-result rate, source diversity, returned token count, and p50/p95 latency. Each query identifies required, acceptable, and misleading segment anchors and whether newest-revision or relationship behavior matters.

The starter corpus is deliberately small and tests overlapping terminology, contradiction, and freshness. It is a contract fixture, not the planned 50-file benchmark. Production retrieval comparisons will cover lexical, vector, fusion, deterministic expansion, inferred relationships, and optional reranking without assuming semantic search wins.

Benchmark execution should use Rust production retrieval code and emit one JSON object per line. Optional Python may analyze emitted files but never enters the runtime dependency chain.

