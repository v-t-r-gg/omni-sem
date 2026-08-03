# Threat model

## Assets and trust boundaries

Source content and the derived database are sensitive. Filesystem contents and future MCP callers are untrusted. Configuration and the executable are trusted within the user account.

Embedding network access is deny-by-default. Only explicit enabled Ollama configuration and an operation needing provider access (`index`, or model resolution in `doctor`) may contact the validated HTTP(S) endpoint. Redirects and environment proxies are disabled, credentials in URLs are rejected, paths and response sizes are fixed/bounded, and source text/provider bodies are excluded from diagnostics. Status reads persisted state only.

Cache rows are untrusted derived data: synchronization validates stored dimensions, byte length, finite values, nonzero norm, and existing L2 normalization before linking. Provider batches are validated in full before mutation, preventing partial malformed responses from creating spaces or cache entries.

## Principal threats

- path traversal, symlink escape, races, special-file reads;
- broad roots or secret files entering the index;
- weaker incremental indexing path that bypasses discovery policy;
- undecodable Git paths silently dropping deletions/renames;
- stale/partial revisions appearing active;
- source text leakage through logs, errors, status HTTP, or snapshot metadata display;
- malicious or corrupted snapshot trees (symlinks, oversized entries, forged counts);
- orphan managed payloads or path deletion outside the managed snapshots directory;
- resource exhaustion from large files, unbounded suggestion scans, or many federated snapshots;
- residual FTS rows after root revocation; snapshot evidence remaining after root disable/remove.

## Controls in this release

- explicit root approval and revocation;
- `init` and `root suggest` never index or auto-approve;
- canonical containment, default no symlink following, special-file skips;
- **shared** `validate_relative_path` for full discovery and Git incremental selection;
- non-UTF-8 Git paths abort incremental collection → full safe fallback (no partial deletes);
- size limits at discovery and stable read;
- metadata double-check during reads (`FILE_CHANGED_DURING_READ`);
- restrictive config/database permissions on Unix;
- transactional revision/FTS promotion and root cascade cleanup;
- snapshot tree inspection without following symlinks; integrity + relationship validation beyond checksum;
- compensating import atomicity; remove only managed payload paths under the snapshots directory;
- snapshot federation caps (snapshots/candidates) and read-only payload opens;
- errors and CLI/status output avoid source text and absolute managed paths;
- status HTTP: loopback bind, method enforcement, header limits, generic 500s, security headers;
- sensitivity tags gate retrieval visibility, including imported snapshot relative paths.
## Semantic query privacy

Semantic and hybrid retrieval send query text once to the explicitly configured embedding provider. Query text, hashes, and vectors are not persisted or cached. Errors are bounded and omit endpoint URLs, credentials, provider bodies, and query content. Lexical retrieval is provider-inert. Auto may make this request only after embeddings have been explicitly enabled.

Exact vector search decodes and validates persisted bytes, excludes corrupt candidates with a bounded warning, and refuses to scan more than 50,000 eligible active vectors. It retains at most 200 semantic candidates and admits at most 768 channel candidates to fusion.
