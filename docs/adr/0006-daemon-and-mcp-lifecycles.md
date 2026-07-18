# ADR-0006: Separate daemon and MCP lifecycles

- Status: Accepted (deferred implementation)
- Date: 2026-07-18

## Context and decision

Continuous indexing and shared model resources must eventually survive individual MCP clients. After static retrieval is proven, use a persistent writer daemon and lightweight STDIO bridge over authenticated user-scoped IPC.

## Alternatives and consequences

One process per client is simpler but duplicates models and couples indexing uptime to clients. Building the daemon now would be premature. Separation later adds authentication, protocol negotiation, restart recovery, and concurrency testing; none is implemented in Milestone 0.

