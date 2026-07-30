# Omni-Sem

Omni-Sem is a local-first, read-only semantic indexing service for AI agents.

## Operational now (Milestone 2)

```text
omnisem init
omnisem root add|list|suggest|remove
omnisem index
omnisem status
omnisem changes
omnisem query
omnisem eval
```

Deterministic local **lexical** search is operational:

```text
query → safe FTS5 MATCH → active-segment BM25 ranking
    → sensitivity filtering → duplicate suppression
    → freshness inspection → token-budget packing → CLI output
```

Evaluation runs against isolated temporary indexes and never mutates the user’s ordinary index.

## Not yet implemented

- embeddings / semantic / hybrid retrieval
- MCP
- `omnisem index --since`
- snapshots, daemon, watcher, IPC
- graph features

## Quick start

```bash
cargo run -p omnisem-cli -- init
cargo run -p omnisem-cli -- root add ./docs --name docs
cargo run -p omnisem-cli -- index
cargo run -p omnisem-cli -- query "storage architecture" --explain
cargo run -p omnisem-cli -- eval --json
```

Use `--data-root PATH` to isolate configuration and database files.

## Development

```bash
./scripts/check.sh
```

See [docs/development.md](docs/development.md), [docs/cli.md](docs/cli.md), [docs/architecture.md](docs/architecture.md), and the [blueprint](docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD).

## License

Apache License 2.0. See [LICENSE](LICENSE).
