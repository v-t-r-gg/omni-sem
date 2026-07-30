# Omni-Sem

Omni-Sem is a local-first, read-only semantic indexing service for AI agents.

## Operational now (Milestone 1)

```text
omnisem init
omnisem root add|list|suggest|remove
omnisem index
omnisem status
omnisem changes
```

These commands provide explicit configuration, approved-root lifecycle, deterministic Markdown/plain-text indexing into immutable revisions, active-only FTS5 maintenance, and operational status/history reporting.

## Not yet implemented

- `omnisem query` / lexical ranking / context packing
- `omnisem eval`
- MCP
- embeddings
- `omnisem index --since`
- snapshots, daemon, watcher, IPC, graph features

## Quick start

```bash
cargo run -p omnisem-cli -- init
cargo run -p omnisem-cli -- root add ./docs --name docs
cargo run -p omnisem-cli -- index
cargo run -p omnisem-cli -- status
```

Use `--data-root PATH` to isolate configuration and database files.

## Development

```bash
./scripts/check.sh
```

See [docs/development.md](docs/development.md), [docs/cli.md](docs/cli.md), [docs/architecture.md](docs/architecture.md), and the [blueprint](docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD).

## License

Apache License 2.0. See [LICENSE](LICENSE).
