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

### `omnisem index [--root ROOT_ID] [--since [REVISION]] [--json]`

Discovers and indexes enabled roots (or one root). Emits counters for additions, modifications, unchanged files, deletions, skips, failures, and segments. Returns exit code 6 when some documents fail.

Git-aware incremental mode (`--since`) uses the same single-path discovery security policy as full scans. Unsupported or undecodable Git paths abandon incremental mutation for that root and perform a full safe discovery fallback with a structured reason. Incremental deletions apply only to paths Git explicitly reports.

### `omnisem status [--json] [--serve] [--port N]`

Reports configuration path, database path, schema version, counters, last scan timestamps, database size, and sensitivity-tag count.

With `--serve`, binds **loopback only** (`127.0.0.1`). Port `0` selects an ephemeral port.

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

Public lexical scores are higher-is-better (`public = -raw_bm25`). Raw FTS5 BM25 remains in JSON signals (lower-is-better, often negative). Scores are not probabilities and are not comparable across retrieval modes.

### `omnisem eval [--corpus PATH] [--mode lexical] [--json]`

Runs the production indexer and retriever against an evaluation bundle in an isolated temporary data root. Default bundle is the repository `evals/` directory when present.

### `omnisem snapshot export PATH`

Writes a directory snapshot (`MANIFEST.json` + `payload.sqlite3`). Contains derived indexed text; treat as sensitive. Refuses overwrite.

### `omnisem snapshot import PATH --map SNAP=LOCAL [--map ...]`

Validates tree, manifest compatibility, payload integrity/relationships/paths, and checksum. Requires every snapshot root mapped exactly once to an **enabled** local root ID. Compensating atomic registration under the managed snapshots directory. Does not approve paths. Queryability requires complete mapping and a healthy payload.

### `omnisem snapshot list|inspect SNAPSHOT_ID|remove SNAPSHOT_ID`

Lifecycle without exposing managed absolute paths, exporter machine paths, or source text. Remove deregisters mappings and deletes only the managed payload.

### Status HTTP (`omnisem status --serve`)

Supported methods and routes:

| Method | Paths |
|--------|--------|
| GET, HEAD | `/`, `/status.json`, `/healthz`, `/health`, `/api/status`, `/api/roots`, `/api/activity` |

- POST and other methods → `405` with `Allow: GET, HEAD`
- Unknown path → `404`
- Malformed request line → `400`
- Oversized headers → `431`
- One request per connection; no body processing; read timeout
- Security headers: CSP, nosniff, DENY frame, no-referrer, no-store
- Generic `500` bodies (no SQLite/filesystem strings)
- JSON includes registered/queryable/unhealthy snapshot counts

### Query provenance

JSON hits include `origin` (`local_index` or `snapshot` with ids). Human output marks snapshot results. Federation uses RRF when snapshots contribute; local BM25 path is unchanged when no snapshots are eligible.
