# Architecture

## Foundation boundary

The workspace has two crates because application logic must be reusable independently of terminal process concerns:

- `omnisem-core`: storage-independent domain types, discovery, parsers, and the project-owned SQLite migration runner.
- `omnisem-cli`: argument parsing, process output, and future dependency wiring.

Domain types do not model SQLite rows. Protocol types will also remain adapters rather than domain authority. Synchronous logic is the default until real asynchronous I/O exists.

## Implemented input pipeline (Milestone 1 slice)

```text
Approved root
    → safe ignore-aware discovery
    → supported-file classification (Markdown | plain text)
    → deterministic Markdown or plain-text parsing
    → validated parser output (segments, anchors, ordinals)
```

This slice does **not** yet persist revisions, build FTS indexes, run retrieval, expose MCP, watch the filesystem, or run a daemon.

## Planned full data flow

```text
approved root → discovery → stable read/hash → parser → transaction
                                                ├─ revision
                                                ├─ segments
                                                ├─ FTS
                                                └─ current pointer
```

A revision becomes current only after baseline indexing succeeds. Retrieval only uses active revisions and always returns source, revision, segment, anchor, text, and ranking evidence. The graph, embeddings, daemon, watcher, IPC, and MCP transport are not foundation components.

## Discovery policy

- roots must be explicit and are canonicalized before traversal;
- `.gitignore` and `.git/info/exclude` inside the root are honored by default;
- parent-directory ignore files outside the root are not applied;
- host-global gitignore is not applied;
- hidden paths are ignored by default;
- Omni-Sem exclude patterns are authoritative when they conflict with keep decisions;
- include patterns, when set, narrow the surviving set;
- symlinks are not followed by default and never escape the approved root;
- devices, sockets, FIFOs, and other special files are skipped;
- default maximum file size is 10 MiB;
- discovery uses metadata only and does not read file contents.

## Parser identities

| Identity | Role |
|---|---|
| `markdown-v1` | Structure-aware Markdown segments |
| `plain-text-v1` | Deterministic UTF-8 fallback; not language-aware |

Structured parsers are registered before the fallback. Duplicate parser identities are rejected. Invalid UTF-8 fails the parse without partial persistent output (persistence is not yet wired).

## Sensitivity vs exclusion

- **Exclusion** controls whether a path is discovered/indexed.
- **Sensitivity tags** on a root mark patterns whose indexed content must later be hidden from MCP (`NeverReturnToMcp`) or require an explicit query opt-in (`RequireExplicitQuery`).

Sensitivity configuration exists in the domain model now; retrieval filtering is deferred until MCP/query surfaces exist.

## Dependency choices

`clap` validates CLI help/version. `serde` / `serde_json` define portable contracts. `thiserror` preserves typed errors. `uuid` supplies opaque IDs. `rusqlite` (bundled) owns explicit SQL and future FTS5. `ignore` + `globset` implement gitignore-aware discovery. `pulldown-cmark` streams Markdown events into segments. `tempfile` is a dev-only fixture helper.

No async runtime, ORM, network stack, model runtime, or MCP transport is included yet.

## Schema

See [schema](schema.md), the checked-in SQL migration, and ADRs for invariants and evolution policy. Timestamp columns and sensitivity storage are deferred to the schema-alignment slice; domain types already carry `Timestamp` and `SensitivityTag`.
