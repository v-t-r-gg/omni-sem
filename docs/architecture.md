# Architecture

## Crates

- `omnisem-core`: domain types, configuration, discovery, parsers, stable reads, hashing, SQLite persistence, indexing service.
- `omnisem-cli`: clap command surface, JSON/human output, exit-code mapping, process wiring.

Domain types remain independent of SQLite row shapes. Protocol adapters are still deferred.

## Operational pipeline (Milestone 1)

```text
Explicit local configuration
    → approved root lifecycle
    → safe ignore-aware discovery
    → stable bounded reads + BLAKE3 hashing
    → deterministic Markdown | plain-text parsing
    → immutable revision + segment persistence
    → transactional active-only FTS5 promotion
    → status and revision-history reporting
```

## Configuration

Platform directories come from the `directories` crate (`dev.OmniSem.omnisem`). Defaults:

- config: platform config dir + `config.toml`
- database: platform data dir + `index.sqlite3`
- logs: platform data dir + `logs/`

TOML uses `serde` with `deny_unknown_fields`. Home expansion (`~/`) happens only at the path boundary. Roots are never added automatically.

## Revision and FTS invariants

- Revisions are immutable.
- Unchanged content with the same parser ID/version skips reparse.
- Parser-version changes create a new revision projection for the same bytes.
- Parse or promotion failure leaves the prior current revision and its FTS rows active.
- Promotion deletes previous active FTS rows, inserts new ones, and updates the current pointer in one transaction.
- Successful full-root discovery may mark missing active files deleted and clear their FTS rows.
- Incomplete discovery failure does not run deletion inference.

## FTS design

`segments_fts` stores duplicated segment text plus unindexed identity metadata. Only active revisions are present. See ADR-0013.

## Security boundary

Explicit roots, canonical containment, default no symlink following, special-file skips, size limits on discovery and read, restrictive config/database permissions where supported, no source text in logs or CLI summaries, sensitivity tags persisted for later retrieval filtering.
