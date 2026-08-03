# Dependency assessment

| Crate | Purpose | Alternative | Impact | License |
|---|---|---|---|---|
| `clap` | CLI parsing | manual args | proc macros | MIT/Apache-2.0 |
| `serde` / `serde_json` | contracts | manual encoding | proc macros | MIT/Apache-2.0 |
| `toml` | configuration | hand-rolled TOML | pure Rust | MIT/Apache-2.0 |
| `directories` | platform paths | hard-coded paths | pure Rust | MIT/Apache-2.0 |
| `thiserror` | typed errors | manual Display | proc macro | MIT/Apache-2.0 |
| `uuid` | opaque IDs | counters | pure Rust | MIT/Apache-2.0 |
| `rusqlite` + bundled SQLite | transactions, FTS5 | external SQLite | compiles C SQLite | MIT |
| `ignore` / `globset` | discovery | walkdir + custom ignore | pure Rust | Unlicense/MIT |
| `pulldown-cmark` | Markdown events | comrak | pure Rust, default features off | MIT |
| `blake3` | content hashing | sha2 | pure Rust (+SIMD) | Apache-2.0/CC0 |
| `tempfile` (dev) | fixtures | manual temps | test-only | MIT/Apache-2.0 |
| `url` 2.5.8 | strict endpoint parsing | hand URI parsing | default features retained; pure Rust plus IDNA/ICU | MIT/Apache-2.0 |
| `ureq` 3.3.0 (optional) | blocking Ollama HTTP | reqwest-blocking, minreq | defaults off, `rustls` only; no native TLS | MIT/Apache-2.0 |
| `rmcp` 3.1.0 (optional) | official MCP server/protocol and STDIO | hand-written JSON-RPC, community SDK | defaults off; `macros`, `server`, `transport-io` | Apache-2.0 |
| `tokio` 1.53.1 (optional) | MCP protocol executor and blocking dispatch | async-std, custom executor | defaults off; `io-std`, `macros`, `rt-multi-thread`, `sync` | MIT |
| `schemars` 1.2.2 (optional) | MCP tool input JSON Schemas | manual schemas | defaults off; `derive` | MIT |

`ureq` is exposed by default feature `embeddings-ollama`; `--no-default-features` removes it and configured Ollama returns `EMBEDDING_FEATURE_DISABLED` while lexical operations work. `reqwest` was rejected for its larger async graph; `minreq` for its smaller ecosystem/policy surface. `url` avoids security-sensitive hand parsing. Rustls supplies TLS; no new direct dependency adds native code. The agent explicitly disables environment proxies and redirects. Compile impact is the Rustls and URL/IDNA graph; binary-size impact was not separately measured.

Unsafe code is forbidden in workspace crates. Bundled SQLite remains the intentional native boundary.
Milestone 4B adds no runtime dependency. Exact search uses `rusqlite` and safe vector decoding already present; fusion uses the standard library. This avoids native ANN/vector-extension packaging and keeps no-default-feature lexical builds unchanged.

## Milestone 5 MCP dependency boundary

`rmcp` 3.1.0 is the current official Rust SDK selected instead of hand-written protocol negotiation; community SDKs were rejected in favor of the protocol project's maintained implementation. Its unused client, HTTP, authentication, TLS, and child-process transport features are disabled. The server uses the SDK's known-version negotiation, preferring protocol `2026-07-28`, and exposes only tool/resource capabilities. It is pure Rust and adds no native library or TLS/proxy/redirect behavior because only process STDIO is enabled.

Tokio is isolated to `omnisem-cli` MCP transport. `spawn_blocking` bridges the synchronous core and provider contracts under a four-request concurrency bound. Async-std and a custom executor were rejected because `rmcp` already uses Tokio. Tokio is pure Rust and cross-platform; no network driver feature is enabled.

Schemars derives bounded input schemas consumed by `rmcp`; manual JSON Schema was rejected because it can drift from Rust request types. It is compile-time/proc-macro heavy but has no native runtime component. All three dependencies are behind the default-enabled `mcp` feature. `--no-default-features` removes them and retains normal lexical operations plus a clear `MCP_FEATURE_DISABLED` result.

The workspace MSRV increases from Rust 1.85 to 1.88, required by `rmcp` 3.1.0. In separate clean release target directories on the development host, `embeddings-ollama` alone built in 1m44s and produced an 11,253,872-byte binary; all features built in 1m57s and produced 14,810,080 bytes. The measured MCP increment is 3,556,208 bytes (3.4 MiB) and 13 seconds for this environment. No new dependency enables native code, TLS, environment proxies, redirects, client transports, or HTTP servers.
