# Dependency assessment

| Crate | Concrete purpose | Simplest alternative | Main impact | License/platform |
|---|---|---|---|---|
| `clap` | Typed CLI, help, version, validation | manual `std::env::args` | compile time and proc macros | MIT/Apache-2.0; portable Rust |
| `serde`, `serde_json` | stable JSON boundary contracts | manual encoding | proc macros and transitive crates | MIT/Apache-2.0; portable Rust |
| `thiserror` | typed error display/source plumbing | manual trait implementations | one proc macro | MIT/Apache-2.0; portable Rust |
| `uuid` | opaque identifiers independent of paths/rows | project counter/string IDs | randomness support | MIT/Apache-2.0; portable Rust |
| `rusqlite` + bundled SQLite | explicit transactions, schema, future FTS5 | SQLite FFI or flat files | largest build/binary impact; compiles C SQLite | MIT; common desktop/server targets |
| `ignore` | recursive discovery with `.gitignore` semantics | `walkdir` + custom ignore parsing | pulls `walkdir`, regex automata; pure Rust | MIT/Apache-2.0; portable Rust |
| `globset` | Omni-Sem include/exclude pattern matching | manual glob implementation | shared with `ignore` stack; pure Rust | MIT/Apache-2.0; portable Rust |
| `pulldown-cmark` | deterministic Markdown event streaming | `comrak` or hand-rolled splitter | modest pure-Rust parser; default features off | MIT; portable Rust |
| `tempfile` (dev) | isolated discovery fixtures | manual temp dirs | test-only | MIT/Apache-2.0; portable Rust |

The lockfile pins transitive versions. CI should add `cargo audit` and a license policy tool once their availability and update policy are stable; neither is required locally to avoid a hidden tool dependency. Unsafe code is forbidden in workspace code, while SQLite's audited native boundary remains inside its dependency.

Decision records: ADR-0011 (`ignore`), ADR-0012 (`pulldown-cmark`).
