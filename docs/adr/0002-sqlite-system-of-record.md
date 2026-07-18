# ADR-0002: SQLite system of record

- Status: Accepted
- Date: 2026-07-18

## Context and decision

Canonical metadata and rebuildable indexes need transactions and local inspection without a service. Use SQLite through `rusqlite`, bundled for consistent capabilities, with explicit SQL and a project-owned migration runner.

## Alternatives and consequences

Flat files are simpler but make atomic cross-record promotion and search difficult. An ORM adds abstraction without an observed maintenance need. External databases add installation and privacy costs. Bundling increases compilation and binary size, while avoiding platform SQLite/FTS variance. Vector search remains replaceable.

