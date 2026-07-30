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

Unsafe code is forbidden in workspace crates. SQLite remains the only intentional native boundary.
