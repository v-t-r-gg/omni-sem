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

`ureq` is exposed by default feature `embeddings-ollama`; `--no-default-features` removes it and configured Ollama returns `EMBEDDING_FEATURE_DISABLED` while lexical operations work. `reqwest` was rejected for its larger async graph; `minreq` for its smaller ecosystem/policy surface. `url` avoids security-sensitive hand parsing. Rustls supplies TLS; no new direct dependency adds native code. The agent explicitly disables environment proxies and redirects. Compile impact is the Rustls and URL/IDNA graph; binary-size impact was not separately measured.

Unsafe code is forbidden in workspace crates. Bundled SQLite remains the intentional native boundary.
Milestone 4B adds no runtime dependency. Exact search uses `rusqlite` and safe vector decoding already present; fusion uses the standard library. This avoids native ANN/vector-extension packaging and keeps no-default-feature lexical builds unchanged.
