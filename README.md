# Omni-Sem

Omni-Sem is a local-first, read-only semantic indexing service for AI agents. It is currently in its project-foundation phase: the repository contains domain, parser, persistence, and evaluation contracts, but does not yet index user files.

The first operational pipeline will be:

```text
Approved Markdown → immutable revisions → structure-aware segments → SQLite FTS5 → bounded retrieval → read-only MCP
```

## Development

Install stable Rust 1.85 or newer, then run:

```bash
./scripts/check.sh
cargo run -p omnisem-cli -- --help
```

No root is added and no source file is read automatically. See [development](docs/development.md), [architecture](docs/architecture.md), and [security](SECURITY.md) for the current contracts.

## Status

Pre-alpha, Milestone 0. Daemon, watcher, MCP transport, embeddings, graph functionality, and source-file mutation are deliberately absent.

## License

Licensed under Apache License 2.0. See [LICENSE](LICENSE).

