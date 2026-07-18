# ADR-0005: Read-only MCP

- Status: Accepted
- Date: 2026-07-18

## Context and decision

Agents need evidence, not filesystem authority. Future MCP adapters expose indexed search, known resources, and status only; they never mutate source files, run commands, accept arbitrary paths, or execute retrieved text.

## Alternatives and consequences

General filesystem tools offer flexibility but destroy the narrow approval boundary. Read-only indexed identifiers reduce risk and client coupling, leaving editing to another explicitly authorized system.

