# Security policy

## Supported versions

Omni-Sem is pre-alpha and has no supported release. Security reports are still welcome through a private security advisory in the project repository; do not include private corpus content in a public issue.

## Security boundary

Omni-Sem will read only explicitly approved roots. Source documents are immutable and untrusted; derived indexes may contain sensitive text and require the same care as the corpus. The product never claims filename exclusions detect every secret.

Discovery canonicalizes roots, rejects root escapes and special files, defaults to not following symlinks, honors workspace ignore rules, applies Omni-Sem excludes authoritatively, and enforces a maximum file size. Sensitivity tags are configuration for later retrieval gating; they do not replace exclusion. The CLI still does not mutate sources, open network sockets, or serve MCP.
