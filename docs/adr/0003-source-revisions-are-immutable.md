# ADR-0003: Source revisions are immutable

- Status: Accepted
- Date: 2026-07-18

## Context and decision

Retrieval must be reproducible and never expose partially updated evidence. Store each observed content/parser state as an immutable revision and transactionally promote it only after baseline indexing succeeds.

## Alternatives and consequences

In-place replacement is simpler but loses lineage and makes failures expose mixed state. Immutable revisions improve stale-result prevention and parser migration reasoning, at the cost of future retention policy and storage growth.

