# Omni-Sem

Omni-Sem is a local-first, read-only semantic indexing service for AI agents.

## Operational now (Milestone 3)

```text
omnisem init
omnisem root add|list|suggest|remove
omnisem index [--since [REVISION]]
omnisem query
omnisem eval
omnisem snapshot export|import
omnisem status [--serve] [--port N]
omnisem changes
```

Capabilities:

- deterministic Markdown / plain-text indexing into immutable revisions + active FTS5;
- lexical retrieval with packing and evaluation;
- optional Git-aware incremental indexing (`index --since`);
- portable sensitive derived-data snapshots (explicit root mapping required);
- local loopback read-only status HTTP view.

## Not yet implemented

- embeddings / semantic / hybrid retrieval
- MCP
- filesystem watcher / persistent daemon
- automatic Git hook installation

## Development

```bash
./scripts/check.sh
```

See [docs/development.md](docs/development.md), [docs/cli.md](docs/cli.md), and [docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD](docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD).

## License

Apache License 2.0. See [LICENSE](LICENSE).
