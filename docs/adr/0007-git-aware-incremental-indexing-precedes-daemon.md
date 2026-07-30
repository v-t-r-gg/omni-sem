# ADR-0007: Git-aware incremental indexing precedes the daemon

- Status: Accepted
- Date: 2026-07-30

## Context and decision

Users need fresher indexes before a persistent daemon and filesystem watcher exist. Ship `omnisem index --since` (git-diff-based incremental indexing) as the interim freshness mechanism. The daemon and watcher remain required for non-git roots and true real-time freshness.

## Alternatives and consequences

Building the watcher first would close more freshness gaps but is a larger Milestone 6 surface (lifecycle, IPC, coalescing). Deferring all incremental work until the daemon would leave git-managed workspaces reindexing fully on every run. Freshness indicators must still surface remaining staleness rather than silently serving outdated revisions.
