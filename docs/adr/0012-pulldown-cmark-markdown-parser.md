# ADR-0012: Use `pulldown-cmark` for Markdown segmentation

- Status: Accepted
- Date: 2026-07-30

## Context and decision

The Markdown parser must emit deterministic structural segments (frontmatter, titles, headings, paragraphs, lists, blockquotes, fenced code, links, tables) with stable anchors. Use `pulldown-cmark` event streaming rather than `comrak`.

## Alternatives and consequences

`comrak` offers a richer AST and more GFM extensions, at higher dependency weight and AST complexity than this slice needs. Manual regex splitting is brittle for nested structure. `pulldown-cmark` is pure Rust, default features minimized, and maps cleanly onto ordered segments. Parser identity is `markdown-v1` so the library can change later if output semantics are preserved or the version is bumped.
