# ADR-0008: Provenance data is surfaced, not only stored

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Revision timestamps, match signals, and source anchors are only useful if agents can see them. Expose match explanation and freshness status directly in retrieval output instead of treating provenance as internal-only storage.

## Alternatives and consequences

Keeping provenance internal preserves a smaller response schema but forces users to trust scores without evidence. Surfacing it reuses data the ingestion and retrieval pipelines already compute; CLI and JSON schemas grow slightly and become part of the stable retrieval contract once Milestone 2 lands.
