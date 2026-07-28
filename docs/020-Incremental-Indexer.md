# 020 — Incremental Indexer

## Vision

Refresh index state on file change without a full repository rescan — the concrete
mechanism behind Part 7's "refreshed incrementally on file change" requirement.

## Goals

- **Structural Context Engine**: filesystem watcher (`notify` crate,
  `context_engine/mod.rs`) triggers per-file re-index on change.
- **Semantic Engine**: `semantic_engine.index_file` RPC re-indexes one file's Tantivy
  chunks (replace-on-change, no stale chunks left behind — verified by
  `re_indexing_a_file_replaces_its_chunks`), and incrementally updates the test-impact or
  doc graph if the changed file is a test or doc (`006-Repository-Digital-Twin.md`).
- **Git status**: a 5-second polling watcher (`handle_repo_connect`'s background task)
  emits `git.diff.update` notifications on real status change, not continuous unconditional
  polling from the client.

## Non-Goals

A fully event-driven, zero-polling architecture for git status — a 5-second poll is
simple, robust, and cheap enough (2.99ms per `git status` call on a small repo) not to
need the added complexity of a native filesystem-event-to-git-status pipeline.

## Architecture

Two independent incremental mechanisms, not a unified "incremental indexer" subsystem:
filesystem-watch-triggered re-index (Context Engine) and explicit `index_file`
RPC-triggered re-index (Semantic Engine, called by whichever shell's editor just saved a
file).

## Data Structures

See `007-Context-Engine.md` and `015-Test-Impact-Analysis.md` for the structures being
incrementally updated.

## Traits / Interfaces

RPC: `context_engine.file_index`, `semantic_engine.index_file`.

## Storage Layout

See `007-Context-Engine.md` — Tantivy index persisted on disk, structural/graph indices
in-memory.

## Performance Targets

A single-file re-index is proportionally cheaper than a full scan — not separately
benchmarked, but bounded by the same per-file chunking cost measured in the full-scan
benchmark (007).

## Tradeoffs

No debouncing on the filesystem watcher — a file saved rapidly multiple times in
succession (an editor's autosave, for instance) could trigger multiple re-index passes.
Not currently a measured problem at typical usage scale; named as a potential future
tuning point rather than a current defect.

## Failure Modes

`index_file`'s incremental update correctly removes stale word-index entries for the
file's old chunks before adding new ones (verified by
`re_indexing_a_file_replaces_its_chunks`) — a naive implementation that only added new
chunks without removing old ones would leak stale search results, which this test
specifically guards against.

## Security

Read-only over trusted repo content.

## Testing

Covered by `semantic_engine/mod.rs`'s `test_index_file_and_search` and the graph
incremental-update tests in `graphs.rs` (`incremental_update_replaces_a_test_files_
previous_edges`, `updating_a_doc_replaces_its_previous_edges`).

## Implementation Order

Filesystem watcher (Phase 1) → Tantivy incremental re-index (Phase 2) → graph incremental
updates (Phase 4).

## Acceptance Criteria

Editing a file and re-indexing it does not leave stale search results or stale graph
edges from the file's previous content.

## AI Coding Rules

Any new incrementally-updated index must have a test proving the *old* state is removed
on update, not just that the new state is added — the specific failure mode every
existing incremental-update test in this codebase guards against.
