# ADR-0011: Use the `ignore` crate for discovery

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Milestone 1 discovery must honor nested `.gitignore` rules, avoid parent ignore files outside the approved root, skip hidden paths by default, and stay deterministic. Use the `ignore` crate (with `globset` for Omni-Sem include/exclude patterns) rather than a hand-rolled walker or plain `walkdir`.

## Alternatives and consequences

`walkdir` alone is smaller but does not implement gitignore semantics. A custom ignore parser would duplicate a well-tested stack and add maintenance risk. `ignore` pulls `walkdir`, `globset`, and regex automata transitively and is pure Rust with no native code. Host-global gitignore is disabled for portable results; Omni-Sem excludes remain authoritative.
