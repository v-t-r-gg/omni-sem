# Development

## Toolchain

The workspace uses Rust edition 2024 and declares Rust 1.85 as its initial minimum supported version. The current CI checks stable Rust on Linux. This MSRV is provisional until dependency compatibility is exercised across releases.

Run all required checks with:

```bash
./scripts/check.sh
```

That command checks formatting, strict Clippy lints, all tests/features, documentation, and a debug build. Release builds and optional dependency audit commands are run separately during release preparation.

## Change policy

Keep source behavior deterministic and derived state rebuildable. Add typed validation at boundaries. Record meaningful dependency or architecture changes in an ADR with problem, alternatives, outcome, and consequences. Never use real private documents as fixtures.

## Exit-code contract

The future CLI reserves categories: 0 success, 2 invalid input, 3 configuration, 4 filesystem, 5 database, 6 partial indexing, 7 protocol, and 70 internal error. Milestone 0 exposes only Clap's input handling and successful shell commands; implementation and tests of stable mapping belong to operational commands.

