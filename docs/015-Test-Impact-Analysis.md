# 015 — Test-Impact Analysis

## Vision

Which tests exercise which code, so a patch's risk can be judged by real coverage data
rather than guesswork — feeds directly into the Confidence Engine's Test Impact signal
(`014-Patch-Verification.md`).

## Goals

`TestImpactGraph` (`cid-core/src/semantic_engine/graphs.rs`) built from real test-file
content, not test files' own defined symbols — extracting call-shaped identifier
references from each test's body and matching them against known source symbols.
Incrementally updatable per file (`update_file`) so an edited test refreshes without a
full repository rescan.

## Non-Goals

Full call-graph transitive coverage analysis (a test that indirectly exercises a symbol
three layers of indirection away) — the graph finds direct references only, proportionate
to Part 7's "which tests exercise which code" ask.

## Architecture

See `006-Repository-Digital-Twin.md` for the full architecture (this document is the
test-impact half of that broader Phase 4 addition) and `011-Knowledge-Graph.md` for how
it relates to the other graphs.

## Data Structures

```rust
pub struct TestImpactGraph {
    symbol_to_tests: HashMap<String, HashSet<String>>,
    test_to_symbols: HashMap<String, HashSet<String>>,
}
```

Bidirectional so both "what tests cover this symbol" and "what does this test cover"
queries are direct lookups.

## Traits / Interfaces

RPC: `semantic_engine.test_impact.{for_symbol,for_symbols,entries}`.

## Storage Layout

In-memory, rebuilt on `semantic_engine.enable`, refreshed per-file via `index_file`.

## Performance Targets

Rebuilt in the same background scan as the rest of the Semantic Engine — 57.7ms for a
200-file repo in this environment (see `007-Context-Engine.md`).

## Tradeoffs

Identifier extraction is crude (a call-shaped-token scan, `extract_identifier_like_tokens`)
rather than a real reference-resolution pass — correct for the common case (a test calling
the function it tests) and proportionate to what's needed, not a substitute for a real
type-aware analyzer.

## Failure Modes

**The real, found-and-fixed bug**: the original implementation fed test files' own
*defined* symbols (`fn it_adds()`) into the graph instead of the identifiers they
*reference* (`add_numbers(1, 2)`) — so real coverage could never be detected, though unit
tests using unrealistic fixture data (symbol lists standing in for call references)
passed anyway. Caught by an end-to-end integration test against real Tree-sitter-parsed
Rust source, not by the unit tests. See `006-Repository-Digital-Twin.md`'s Failure Modes
for the full account and `graphs.rs`'s module doc for the fix.

## Security

Read-only over trusted repo content.

## Testing

`builds_symbol_to_test_mapping_from_real_test_content`,
`does_not_treat_a_tests_own_helper_as_a_covered_source_symbol`, and 14 other tests in
`graphs.rs`; end-to-end coverage in `api_integration.rs`'s
`test_impact_and_doc_graphs_populate_after_enabling_the_semantic_engine`.

## Implementation Order

Built in Phase 4 as one of the two genuinely new Repository Digital Twin pieces.

## Acceptance Criteria

A real function, called by a real test file, in a real (non-fixture) directory scanned by
the actual analyzer, is discoverable via `semantic_engine.test_impact.for_symbol` —
verified end-to-end, which is the specific thing the original bug would have failed.

## AI Coding Rules

Never feed a `FileIndex.symbols` list (definitions) into `TestImpactGraph::build`'s
content parameter — it must be the file's actual source text. This is the exact class of
mistake that shipped once; the type signature (`test_contents: &[(String, String)]`,
requiring real content) exists specifically to make that mistake harder to repeat.
