# Changelog

All notable changes will be documented here. The format follows Keep a Changelog; the project does not yet promise semantic-version compatibility.

## Unreleased

### Added

- Authoritative Development Blueprint v0.3.
- ADR-0007 through ADR-0012 (git-aware incremental indexing, surfaced provenance, personal-knowledge interoperability, sensitivity tags, `ignore` discovery, `pulldown-cmark` Markdown).
- Domain contracts for plain-text classification, serializable timestamps, and sensitivity tags.
- Safe ignore-aware discovery for approved roots (`omnisem_core::discovery`).
- Deterministic `markdown-v1` and `plain-text-v1` parsers behind the existing parser registry.

### Changed

- Package repository metadata now points at `https://github.com/v-t-r-gg/omni-sem`.
- Architecture, schema, threat-model, development, vision, and dependency documentation updated for the input-pipeline slice.

### Foundation (prior)

- Establish the Rust workspace, domain and parser contracts, SQLite migration scaffold, evaluation formats, CI, and architecture documentation.
