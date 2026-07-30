# ADR-0009: Interoperate with existing personal-knowledge structures

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Respect `.gitignore` and existing workspace conventions during discovery rather than requiring a parallel Omni-Sem-only exclusion model. Users should not restructure notes or projects to make the product work.

## Alternatives and consequences

A single Omni-Sem exclude list is simpler to reason about but raises setup friction. Discovery therefore depends on the already-selected `ignore` crate for gitignore-compatible rules. Omni-Sem-specific excludes remain available and take precedence where they conflict. Reading vault layouts as-is does not imply importing or rewriting those layouts.
