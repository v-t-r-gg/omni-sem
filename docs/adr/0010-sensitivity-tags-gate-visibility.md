# ADR-0010: Sensitivity tags gate visibility, not indexing

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Introduce `SensitivityTag` / `SensitivityScope` so content may be indexed for local use while remaining hidden from MCP or ordinary retrieval unless explicitly permitted. Exclusion patterns alone decide whether content is indexed.

## Alternatives and consequences

Using exclude patterns for both indexing and visibility forces users to drop sensitive notes from local search entirely. Sensitivity adds a retrieval-path filter that must be tested when MCP and query surfaces land. It does not weaken read-only or local-first guarantees; it only narrows which already-approved indexed content may surface.
