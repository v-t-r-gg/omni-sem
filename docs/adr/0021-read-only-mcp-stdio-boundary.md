# ADR-0021: Read-only MCP STDIO boundary

Status: accepted

## Decision

Expose Milestone 5 through `omnisem mcp`, using `rmcp` 3.1.0 with server, macros, and standard-I/O features only. MCP SDK and schema types live in the CLI adapter. A synchronous `McpContextService` in core owns search, hydration, strict resource resolution, and safe persisted status.

The adapter uses a Tokio runtime for protocol I/O, a four-permit semaphore, and `spawn_blocking` for synchronous SQLite/provider calls. Each application request opens the database read-only, runs no migration, and performs no mutation. Stdout is protocol-only; stderr is diagnostic-only. STDIO EOF is the shutdown boundary.

MCP retrieval uses an explicit `Mcp` audience. `NeverReturnToMcp` and `RequireExplicitQuery` candidates are removed before ranking, exact-scan accounting, fusion, deduplication, and packing. Hydration repeats eligibility and sensitivity checks. Strict `omnisem://` resource identifiers never fall back to filesystem paths.

The `mcp` feature is enabled by default because this is the technical-MVP client boundary. Disabling default features removes the SDK/runtime graph while preserving lexical CLI operation; invoking `omnisem mcp` then returns `MCP_FEATURE_DISABLED`.

## Consequences

The official SDK provides negotiation and protocol/schema behavior without a hand-written JSON-RPC implementation. Tokio does not enter the storage, retrieval, or provider contracts. There is no HTTP transport, write tool, root management, resource enumeration of the corpus, provider discovery, or daemon lifecycle. A future daemon may replace direct database access without changing the application service contract.

Snapshot format 1 remains lexical-only. Schema version 4 is sufficient, so no migration is introduced.
