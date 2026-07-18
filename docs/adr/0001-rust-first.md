# ADR-0001: Rust-first runtime

- Status: Accepted
- Date: 2026-07-18

## Context and decision

The product needs a native, low-idle-overhead, durable runtime without a mandatory language service. Use stable Rust for every production component. Python is limited to optional offline analysis.

## Alternatives and consequences

Python would accelerate experiments but impose a runtime and packaging boundary; Go would simplify some builds but diverge from the requested ownership and ecosystem strategy. Rust requires deliberate implementation but provides explicit errors, safe concurrency foundations, and native distribution.

