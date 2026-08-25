# 006 — Repository Digital Twin

## Vision

Documents the **scoped resolution** of the "Repository Digital Twin" proposal (12 graph
types, live twin, all AI edits operate on the twin first) evaluated in Phase 4's own
"what changed, and why" table. See `CID-Phase4-Build-Prompt.md`'s Part 0 for the original
resolution text.

## Goals

The proposal is ~90% already covered by the existing Semantic Context Engine
(`007-Context-Engine.md`): embeddings, symbol/dependency graph, ownership/git-history
overlays. Phase 4 added the two genuinely new pieces:

- **Test-impact graph** — which tests exercise which symbols, incrementally rebuildable.
  `cid-core/src/semantic_engine/graphs.rs::TestImpactGraph`.
- **Documentation graph** — which docs reference which symbols, and which docs are stale
  because the symbol they describe no longer exists.
  `cid-core/src/semantic_engine/graphs.rs::DocGraph`.

"Session history" and "AI memory" from the original proposal are treated as the Session
Thread/History data that already exists (`013-Repository-Health.md`,
`cid-core/src/persistence`), not a new graph type. "Runtime graph" stays optional/deferred,
carrying the same tag it had in the original proposal.

## Non-Goals

A separate 12-graph subsystem. Building one would duplicate ~90% of what
`semantic_engine` already does and reintroduce the "full knowledge graph as a day-one
requirement" pattern the founding brief's v3.0 already scoped down for good reason (see
Part 2's "what changed" table).

## Architecture

Both graphs live inside `SemanticEngine`'s `RepoIndex` (`semantic_engine/mod.rs`),
rebuilt on `enable()` alongside the Tantivy scan, refreshed incrementally on `index_file`.
See `007-Context-Engine.md` for the full engine architecture; this document covers only
the two Phase 4 additions.

## Data Structures

```rust
pub struct TestImpactGraph {
    symbol_to_tests: HashMap<String, HashSet<String>>,
    test_to_symbols: HashMap<String, HashSet<String>>,
}
pub struct DocGraph {
    doc_to_symbols: HashMap<String, HashSet<String>>,  // unfiltered mentions
    symbol_to_docs: HashMap<String, HashSet<String>>,
}
```

`DocGraph` stores mentions **unconditionally**, not filtered against known symbols at
write time — staleness detection needs to compare "what a doc mentioned" against "what
currently exists," so pre-filtering at write time would make every doc trivially
non-stale by construction. This was a real bug found and fixed during Phase 4
implementation (see Failure Modes).

## Storage Layout

In-memory only, rebuilt from a full repository scan (`build_graphs` in
`semantic_engine/mod.rs`) plus incremental per-file updates. Not persisted to disk
separately — the underlying Tantivy index and SQLite persist other engine state, but
these two graphs are cheap enough to rebuild that persistence wasn't judged worth the
complexity.

## Performance Targets

Rebuild happens in the same background scan as the Tantivy index (`enable()`'s
`tokio::spawn`), so it doesn't block the RPC response. No separate budget measured beyond
the general indexing benchmark in `004-System-Architecture.md`.

## Tradeoffs

Test-impact detection uses a crude identifier-extraction heuristic
(`extract_identifier_like_tokens`) rather than full call-graph analysis — correct enough
to find "this test calls this function" but not "this test transitively exercises this
function via three layers of indirection." Accepted as proportionate to what Part 7 asks
for ("which tests exercise which code," not perfect coverage analysis).

## Failure Modes

**Found and fixed during implementation, not merely anticipated:** the original
`TestImpactGraph::build` fed test files' own *defined* symbols (e.g., `fn it_adds()`)
into the graph instead of the identifiers they *reference* (a call to `add_numbers`) —
so it could never detect real coverage against actual Tree-sitter output, though unit
tests with unrealistic fixture data passed. Caught by an end-to-end integration test
(`test_impact_and_doc_graphs_populate_after_enabling_the_semantic_engine`) against real
parsed Rust source, not by the unit tests alone. See
`cid-core/src/semantic_engine/graphs.rs` module doc for the full account.

## Security

No new attack surface — read-only analysis over already-trusted repo content.

## Testing

17 unit tests in `semantic_engine/graphs.rs`, plus the end-to-end integration test above
proving real Tree-sitter output populates both graphs correctly.

## Implementation Order

Built in Phase 4 as an extension of Phase 2's Semantic Context Engine, per this
document's own scoping decision.

## Acceptance Criteria

`semantic_engine.test_impact.for_symbol` and `semantic_engine.docs.stale` RPC methods
return real, verified data against a live-scanned repository — not fixture-only
correctness.

## AI Coding Rules

If you touch `TestImpactGraph::build`, re-run
`test_impact_and_doc_graphs_populate_after_enabling_the_semantic_engine` specifically —
it is the test that catches the class of bug (definitions vs. references) that shipped
once already.
