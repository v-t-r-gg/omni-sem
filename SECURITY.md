# Security policy

## Supported versions

Omni-Sem is pre-alpha and has no supported release. Security reports are still welcome through a private security advisory in the project repository; do not include private corpus content in a public issue.

## Security boundary

Omni-Sem will read only explicitly approved roots. Source documents are immutable and untrusted; derived indexes may contain sensitive text and require the same care as the corpus. The product never claims filename exclusions detect every secret.

Milestone 0 performs no discovery, indexing, networking, model invocation, MCP serving, or source mutation. Future implementations must canonicalize paths, reject root escapes and special files, default to not following symlinks, impose size limits, and keep protocol output separate from logs.

