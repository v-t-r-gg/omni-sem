# Threat model

## Assets and trust boundaries

Source content and the derived database are sensitive. Filesystem contents and future MCP callers are untrusted. Configuration and the executable are trusted within the user account.

## Principal threats

- path traversal, symlink escape, races, special-file reads;
- broad roots or secret files entering the index;
- stale/partial revisions appearing active;
- source text leakage through logs, errors, or status output;
- resource exhaustion from large files or unbounded suggestion scans;
- residual FTS rows after root revocation.

## Controls in this release

- explicit root approval and revocation;
- `init` and `root suggest` never index or auto-approve;
- canonical containment, default no symlink following, special-file skips;
- size limits at discovery and stable read;
- metadata double-check during reads (`FILE_CHANGED_DURING_READ`);
- restrictive config/database permissions on Unix;
- transactional revision/FTS promotion and root cascade cleanup;
- errors and CLI output avoid source text;
- sensitivity tags are stored for later retrieval filtering, not treated as exclusions.
