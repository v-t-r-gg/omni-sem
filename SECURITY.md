# Security policy

## Supported versions

Omni-Sem is pre-alpha and has no supported release. Security reports are still welcome through a private security advisory in the project repository; do not include private corpus content in a public issue.

## Security boundary

Omni-Sem reads only explicitly approved roots. Source documents remain immutable and untrusted. Derived indexes may contain sensitive text and require the same care as the corpus. Filename exclusions do not detect every secret.

Operational controls now include:

- explicit root approval and revocation with derived-data cleanup;
- canonical root containment and default no symlink following;
- special-file rejection and size-limited stable reads;
- restrictive configuration and database permissions on Unix;
- no source text in logs or standard command summaries;
- sensitivity tags persisted for later MCP/retrieval filtering.

The product still has no MCP server, network embedding providers, or daemon in this release.
