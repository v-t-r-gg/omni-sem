# Omni-Sem

Omni-Sem is a local-first, read-only semantic indexing service for AI agents.

The repository currently implements the first Milestone 1 **input pipeline** slice under Development Blueprint v0.3:

```text
Approved root → ignore-aware discovery → Markdown | plain-text parse → validated segments
```

It does **not** yet index into SQLite, build FTS, retrieve context, or expose MCP. The CLI foundation command remains `status` only.

## Development

Install stable Rust 1.85 or newer, then run:

```bash
./scripts/check.sh
cargo run -p omnisem-cli -- --help
```

No root is added and no source file is read automatically by the CLI. Library discovery and parsers are available for the next indexing slice. See [development](docs/development.md), [architecture](docs/architecture.md), [blueprint v0.3](docs/OMNI_SEM_DEVELOPMENT_BLUEPRINT_v0.3.MD), and [security](SECURITY.md).

## Status

Pre-alpha, Milestone 1 input-pipeline slice. Configuration CLI, immutable revision persistence, FTS5, retrieval, MCP, daemon, watcher, and embeddings are deliberately absent.

## License

Licensed under Apache License 2.0. See [LICENSE](LICENSE).
