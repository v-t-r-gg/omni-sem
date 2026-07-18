# Dependency assessment

| Crate | Concrete purpose | Simplest alternative | Main impact | License/platform |
|---|---|---|---|---|
| `clap` | Typed CLI, help, version, validation | manual `std::env::args` | compile time and proc macros | MIT/Apache-2.0; portable Rust |
| `serde`, `serde_json` | stable JSON boundary contracts | manual encoding | proc macros and transitive crates | MIT/Apache-2.0; portable Rust |
| `thiserror` | typed error display/source plumbing | manual trait implementations | one proc macro | MIT/Apache-2.0; portable Rust |
| `uuid` | opaque identifiers independent of paths/rows | project counter/string IDs | randomness support | MIT/Apache-2.0; portable Rust |
| `rusqlite` + bundled SQLite | explicit transactions, schema, future FTS5 | SQLite FFI or flat files | largest build/binary impact; compiles C SQLite | MIT; common desktop/server targets |

The lockfile pins transitive versions. CI should add `cargo audit` and a license policy tool once their availability and update policy are stable; neither is required locally in the foundation to avoid a hidden tool dependency. Unsafe code is forbidden in workspace code, while SQLite's audited native boundary remains inside its dependency.

