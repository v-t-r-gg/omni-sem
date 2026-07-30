# ADR-0017: Compensating atomicity for snapshot import

- Status: Accepted
- Date: 2026-07-30

## Context

Filesystem copy and SQLite registration cannot share one ACID transaction. A naïve “copy then insert” can leave an orphaned managed payload if registration fails, or a registered row pointing at a missing file if rename fails.

## Decision

Import order:

1. Validate source tree, manifest, and source payload (read-only checks).
2. Copy into a unique temporary file under the managed `snapshots/` directory.
3. Restrict permissions and re-validate the managed copy.
4. Transactionally insert `snapshots` + `snapshot_root_maps` rows pointing at the temp path.
5. On commit success, rename the temp file to the final managed name and update `payload_path`.
6. On any failure before successful rename, delete the temp file and roll back or delete registration rows.
7. Never overwrite an existing final managed payload path.
8. Never leave a visible partial snapshot ID in list/inspect as queryable when the payload is missing.

Removal prefers DB deregistration first, then managed payload delete with a clear orphan error if filesystem delete fails after DB removal.

## Consequences

Callers observe either a fully registered snapshot with a healthy payload path or no registration. Operators can recover orphan files under `snapshots/` by deleting unknown `.tmp` or unreferenced `.sqlite3` files after `snapshot list` reports unhealthy payloads.
