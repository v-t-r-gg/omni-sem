# ADR-0019: Ollama transport and Cargo feature boundary

Status: accepted.

The boundary is synchronous because CLI and SQLite workflows are synchronous. Optional `ureq` 3.3.0 is behind default feature `embeddings-ollama`; no-default builds keep all lexical behavior and report `EMBEDDING_FEATURE_DISABLED` for Ollama.

Only `GET /api/tags` and `POST /api/embed` are used. `/api/embeddings` and `/api/pull` are never called. The Rustls agent has explicit connect/global timeouts, a 32 MiB response limit, no environment proxy, and no redirects. See `docs/dependencies.md`.
