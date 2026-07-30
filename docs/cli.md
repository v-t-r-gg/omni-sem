# CLI reference (Milestone 1 operational indexing)

Global option:

```bash
omnisem --data-root PATH <command>
```

`--data-root` redirects configuration and database layout for tests and isolated installs. Production installs omit it and use platform directories.

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | success |
| 2 | invalid input |
| 3 | configuration failure |
| 4 | filesystem failure |
| 5 | database failure |
| 6 | partial indexing failure |
| 7 | protocol failure |
| 70 | internal error |

## Commands

### `omnisem init [--json]`

Creates configuration and data directories, writes a default config when missing, opens the database, and applies migrations. Never adds roots or indexes files. Idempotent.

### `omnisem root add PATH [--name NAME] [--json]`

Approves a directory root after existence and directory checks. Canonicalizes the path, assigns a stable root ID, writes safe include/exclude defaults, and persists the root row. Does not read source contents.

### `omnisem root list [--json]`

Lists approved roots with include/exclude rules, symlink policy, sensitivity-tag counts, and indexed-file counts when available.

### `omnisem root suggest [--json]`

Metadata-only suggestion near the current directory. Bounded by depth, entry count, duration, and candidate count. Never approves or indexes.

### `omnisem root remove ROOT_ID [--json]`

Removes the root from configuration and transactionally deletes derived database rows and active FTS entries. Source files are never modified.

### `omnisem index [--root ROOT_ID] [--json]`

Discovers and indexes enabled roots (or one root). Emits counters for additions, modifications, unchanged files, deletions, skips, failures, and segments. Returns exit code 6 when some documents fail.

### `omnisem status [--json]`

Reports configuration path, database path, schema version, counters, last scan timestamps, database size, and sensitivity-tag count.

### `omnisem changes [--since 7d|12h|30m|90s] [--root ROOT_ID] [--json]`

Reports additions, modifications, and deletions from stored source/revision records without printing source text.

### `omnisem query QUERY [options]`

Lexical retrieval over active FTS rows.

Options:

- `--mode lexical|auto|semantic|hybrid` (`semantic`/`hybrid` unavailable)
- `--root ROOT_ID`
- `--file-type markdown|plain_text`
- `--limit N`
- `--token-budget N`
- `--budget NAME` (mutually exclusive with `--limit` / `--token-budget`)
- `--include-sensitive`
- `--explain`
- `--json`

Public BM25 scores are higher-is-better (`1/(1+raw_bm25)`). Raw BM25 remains in JSON signals (lower-is-better).

### `omnisem eval [--corpus PATH] [--mode lexical] [--json]`

Runs the production indexer and retriever against an evaluation bundle in an isolated temporary data root. Default bundle is the repository `evals/` directory when present.
