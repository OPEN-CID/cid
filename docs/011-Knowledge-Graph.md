# 011 — Knowledge Graph

## Vision

"What else touches this" queries over a repository's real structure — symbols, imports,
dependencies, tests, docs — without a general-purpose graph database.

## Goals

- **Dependency graph** (Phase 2): `DependencyNode`/`DependencyEdge` built from import
  extraction (`semantic_engine::extract_dependencies`), queryable via
  `semantic_engine.dependency_graph`.
- **Test-impact graph** (Phase 4): symbol ↔ covering-test edges
  (`006-Repository-Digital-Twin.md`).
- **Documentation graph** (Phase 4): symbol ↔ referencing-doc edges, plus staleness
  detection.
- **Structural symbol index** (Phase 1): file → symbols, the foundation every other graph
  here is built from (`analyzer::CodeAnalyzer::analyze_directory`).

## Non-Goals

A general-purpose graph database (Neo4j-style) or `petgraph`-based in-memory graph engine
as a separate subsystem — the founding brief named `petgraph` in its original stack table
but every graph CID actually needs (dependency, test-impact, doc) is well-served by plain
`HashMap<String, HashSet<String>>` bidirectional maps, which is what's actually
implemented. Simpler, no new dependency, same query capability at this codebase's scale.

## Architecture

All graphs share one input: `CodeAnalyzer::analyze_directory`'s Tree-sitter symbol
extraction (`analyzer/mod.rs`). Each graph (dependency, test-impact, doc) is a
purpose-built index over that same symbol data, not a shared generic graph structure —
each has different query shapes (dependency: file→file; test-impact: symbol→test;
doc: symbol→doc) that don't benefit from forcing a common representation.

## Data Structures

`DependencyNode`, `DependencyEdge` (`api/types.rs`); `TestImpactGraph`, `DocGraph`
(`semantic_engine/graphs.rs`); `FileIndex`, `CodeSymbol` (`analyzer` output, the common
input).

## Traits / Interfaces

RPC: `code.{analyze_file,analyze_directory,search_symbols,get_imports}`,
`semantic_engine.dependency_graph`, `semantic_engine.test_impact.*`,
`semantic_engine.docs.*`.

## Storage Layout

In-memory, rebuilt from a repository scan; not persisted separately (the Tantivy index
persists search state, but graph structure is cheap to rebuild).

## Performance Targets

Bundled into the same repository-scan benchmark as `007-Context-Engine.md` — 57.7ms for
a 200-file repo in this environment.

## Tradeoffs

Choosing plain hash maps over `petgraph` trades generic graph algorithms (shortest path,
centrality) for simplicity — accepted because CID's actual queries ("what tests cover
this," "what docs reference this") are direct lookups, not graph traversal problems that
would benefit from a real graph library.

## Failure Modes

See `006-Repository-Digital-Twin.md`'s Failure Modes — the definitions-vs-references bug
found in `TestImpactGraph::build` is the concrete example of what goes wrong when a graph
is built from the wrong signal.

## Security

Read-only over already-trusted repo content.

## Testing

Covered by `analyzer/mod.rs`, `semantic_engine/graphs.rs` (16 tests), and
`semantic_engine/mod.rs`'s dependency-graph tests.

## Implementation Order

Structural symbol index (Phase 1) → dependency graph (Phase 2) → test-impact/doc graphs
(Phase 4).

## Acceptance Criteria

A known call/import relationship in a real (non-fixture) repository is discoverable via
`semantic_engine.dependency_graph` and `code.get_imports`.

## AI Coding Rules

Before adding a `petgraph` dependency for a new query need, check whether a direct
`HashMap` lookup already solves it — this codebase deliberately avoided that dependency
once already for good reason (see Tradeoffs).
