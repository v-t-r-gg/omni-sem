# Architecture

## Foundation boundary

The workspace has two crates because application logic must be reusable independently of terminal process concerns:

- `omnisem-core`: storage-independent domain and parser contracts plus the project-owned SQLite migration runner.
- `omnisem-cli`: argument parsing, process output, and future dependency wiring.

Domain types do not model SQLite rows. Protocol types will also remain adapters rather than domain authority. Synchronous logic is the default until real asynchronous I/O exists.

## Planned data flow

```text
approved root → discovery → stable read/hash → parser → transaction
                                                ├─ revision
                                                ├─ segments
                                                ├─ FTS
                                                └─ current pointer
```

A revision becomes current only after baseline indexing succeeds. Retrieval only uses active revisions and always returns source, revision, segment, anchor, text, and ranking evidence. The graph, embeddings, daemon, watcher, IPC, and MCP transport are not foundation components.

## Dependency choices

`clap` replaces hand-written argument parsing with validated help/version behavior. `serde` and `serde_json` define explicit portable contracts; manual JSON would be smaller but error-prone. `thiserror` preserves typed error matching without boilerplate. `uuid` supplies opaque IDs rather than authority-bearing paths or fragile row IDs. `rusqlite` offers explicit SQL and transaction control; direct SQLite FFI would add unsafe maintenance, while an ORM would obscure the intended schema. Its bundled SQLite feature gives reproducible FTS-capable builds at the cost of compile time and binary size.

No async runtime, Markdown parser, logging facade, ORM, network stack, model runtime, or parser framework is included yet. Their concrete behavior belongs to later milestones and requires a decision record when selected.

## Schema

See [schema](schema.md), the checked-in SQL migration, and ADRs for invariants and evolution policy.

