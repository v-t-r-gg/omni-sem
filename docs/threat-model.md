# Threat model

## Assets and trust boundaries

Source content and the derived database are sensitive. Filesystem contents, Markdown text, MCP callers, and future model output are untrusted. Local configuration and the executable are trusted only within the user's account boundary.

## Principal threats

- path traversal, symlink escape, race conditions, and special-file reads;
- accidentally indexing secrets or overly broad roots;
- stale or partial revisions appearing in retrieval;
- prompt injection in retrieved evidence;
- protocol injection through standard output;
- source text leakage through logs, errors, exports, or future network providers;
- resource exhaustion from large files, requests, or unbounded work.

## Required controls

Roots require explicit approval and canonical containment checks. Symlink following defaults off. Discovery must reject devices, sockets, pipes, oversized files, root escapes, and documented secret filename patterns. Revision promotion is transactional. Retrieved text is labeled untrusted, never executed, and never treated as instructions. Logs exclude text and credentials. Future IPC is user-scoped, authenticated, versioned, bounded, and timeout-controlled.

Milestone 0 has no source-reading or network behavior, reducing the implemented attack surface to CLI argument and local SQLite schema initialization contracts.

