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
- resource exhaustion from large files, requests, or unbounded work;
- sensitive indexed content leaking through MCP once that surface exists.

## Required controls

Roots require explicit approval and canonical containment checks. Symlink following defaults off. Discovery rejects devices, sockets, pipes, oversized files, root escapes, and documented secret filename patterns. Nested `.gitignore` rules are honored by default; Omni-Sem excludes remain authoritative. Sensitivity tags gate future retrieval visibility without deleting indexed data. Revision promotion is transactional when indexing lands. Retrieved text is labeled untrusted, never executed, and never treated as instructions. Logs exclude text and credentials. Future IPC is user-scoped, authenticated, versioned, bounded, and timeout-controlled.

## Implemented surface

This repository implements discovery and parsers only. No network, MCP, daemon, or source mutation is present. Discovery and parse failures produce typed errors or skip reasons rather than partial persistent revisions (persistence is not yet wired).
