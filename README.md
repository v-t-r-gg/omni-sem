# Omni-Sem

Omni-Sem is a local-first, read-only semantic indexing service for AI agents.

## Operational now (Milestone 5)

```text
omnisem init
omnisem root add|list|suggest|remove
omnisem index [--since [REVISION]]
omnisem query
omnisem eval
omnisem snapshot export|import|list|inspect|remove
omnisem status [--serve] [--port N]
omnisem doctor
omnisem changes
omnisem mcp
```

Capabilities:

- deterministic Markdown / plain-text indexing into immutable revisions + active FTS5;
- lexical, semantic, hybrid, and automatic retrieval with bounded packing and comparative evaluation;
- optional Git-aware incremental indexing (`index --since`) using the **same discovery security policy** as full scans;
- portable sensitive derived-data snapshots with validation, explicit root mapping, lifecycle commands, and **federated lexical retrieval** (RRF) after complete mapping;
- local loopback read-only status HTTP view with method-aware GET/HEAD contracts.
- optional explicitly configured Ollama embedding materialization with digest-isolated spaces and a model-aware vector cache.
- read-only MCP over STDIO for bounded search, context hydration, and persisted status.

Embeddings are disabled by default and fresh installs make no network requests. Enabling Ollama permits requests only to the configured HTTP(S) endpoint during `index`, semantic/hybrid queries, auto queries that attempt hybrid, semantic/hybrid evaluation, and provider checks in `doctor`; Omni-Sem never pulls a model. Query text is sent only to that explicit provider and query vectors are transient. Lexical query and evaluation remain provider-inert. Embedding failure leaves lexical revisions and FTS valid, and a later `index` backfills unchanged active segments. Vectors are derived and rebuildable.

Imported snapshots are queryable only after every snapshot root is explicitly mapped to an enabled local root. Local exact text-hash duplicates win over snapshot evidence. Snapshot freshness is always `unknown`. Snapshot payloads contain derived text and must be treated as sensitive.

Semantic retrieval scans compatible active local vectors exactly, with a 50,000-vector safety bound. Hybrid retrieval combines local lexical, each eligible snapshot lexical list, and local semantic evidence in one RRF pass; BM25 and cosine values are never compared directly. Snapshot format 1 remains lexical-only.

`omnisem mcp` exposes only approved indexed evidence. It cannot index, manage roots, read arbitrary paths, execute source text, or mutate the database. MCP evidence is explicitly marked untrusted, and content tagged `NeverReturnToMcp` or `RequireExplicitQuery` is excluded before ranking. See [MCP client setup](docs/mcp-client-setup.md).

## Not yet implemented (Milestone 6+)

- filesystem watcher / persistent daemon
- ANN/vector extensions and portable snapshot vectors
- in-process embedding providers
- automatic Git hook installation

## Development

```bash
./scripts/check.sh
```

See [docs/development.md](docs/development.md), [docs/cli.md](docs/cli.md), and [docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD](docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD).

## License

Apache License 2.0. See [LICENSE](LICENSE).
