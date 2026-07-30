# SQLite schema proposal

Schema version 1 establishes `schema_metadata`, `roots`, `source_files`, `revisions`, and `segments`. Identifiers are opaque UUID text. Domain timestamps now use milliseconds since the Unix epoch (`Timestamp`); corresponding SQL columns and sensitivity-tag storage are planned for the next schema-alignment slice and are not present in migration `0001_initial.sql` yet. Include/exclude lists and segment metadata are JSON text to preserve a narrow schema while remaining inspectable.

Key invariants:

- one source identity per root-relative path;
- revision uniqueness includes source, content hash, parser ID, and parser version;
- segment anchors and ordinals are unique inside a revision;
- current revision promotion will be an explicit transaction in a later Milestone 1 slice;
- foreign keys are enabled by the connection boundary;
- future schema versions are rejected.

FTS is intentionally not created yet. A later Milestone 1 slice must verify FTS5 availability and specify how active-revision filtering and transactional replacement work before adding its migration.

The project-owned migration runner applies monotonic SQL migrations inside transactions. A failed migration is returned as a database error and is not marked complete. Downgrades are unsupported; rebuildable derived databases may be reconstructed when documented migrations cannot apply.
