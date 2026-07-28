# 019 — Code Parser

## Vision

Turn source files into ASTs and symbol tables via Tree-sitter — incremental, fast,
tolerant of syntax errors — feeding every downstream analysis (Context Engine,
Confidence Engine, test-impact/doc graphs) from one shared extraction layer.

## Goals

`CodeAnalyzer` (`cid-core/src/analyzer/mod.rs`) parses Rust, TypeScript, JavaScript,
Python, Go, and JSON via per-language Tree-sitter grammars, extracting `FileIndex`
(symbols, imports, language, size) per file, recursively over a directory.

## Non-Goals

A universal, language-agnostic AST representation — each language's grammar produces its
own tree; `CodeAnalyzer` extracts a common `CodeSymbol` shape (name, kind, location,
parent) from each, but doesn't attempt to unify the underlying grammars themselves.

## Architecture

`analyze_directory` recursively walks a path, dispatching each file by extension to the
matching Tree-sitter grammar, producing `FileIndex` entries consumed by
`context_engine`, `semantic_engine`, and `confidence`.

## Data Structures

`FileIndex { path, language, symbols: Vec<CodeSymbol>, imports, last_modified, size }`,
`CodeSymbol { name, kind: SymbolKind, file_path, line, column, end_line, end_column,
parent, imports }` (`api/types.rs`).

## Traits / Interfaces

RPC: `code.{analyze_file,analyze_directory,search_symbols,get_imports}`.

## Storage Layout

No persistence of its own — `FileIndex` results are consumed and re-derived by callers
(Context Engine caches a version for its own use; Confidence Engine re-scans per patch).

## Performance Targets

Bundled into the repository-scan benchmarks referenced across `007-Context-Engine.md`
and `015-Test-Impact-Analysis.md` — 57.7ms for 200 files in this environment.

## Research

Tree-sitter's incremental, error-tolerant parsing (Max Brunsfeld et al., originally for
Atom, now the basis of GitHub's code navigation and Zed's own editor core) is real,
citable prior art directly relevant to this choice — not a CID-specific invention. Chosen
over ANTLR/PEG-based alternatives specifically because Tree-sitter grammars are
incremental and tolerant of syntax errors mid-edit, which a `TODO`-riddled work-in-
progress file needs (Part 2 of the founding brief).

## Tradeoffs

`analyzer::analyze_directory` recurses without a depth or skip-list guard for
`.git`/`node_modules`/`target` — unlike `semantic_engine`'s own repository scan, which
does skip those directories explicitly (`SKIP_DIRS` in `semantic_engine/mod.rs`). This is
a real inconsistency: the Confidence Engine, which calls `analyzer::analyze_directory`
directly, will walk into `target/` or `node_modules/` on a real project, unlike the
Semantic Engine's scan. Named here as a known gap rather than silently accepted.

## Failure Modes

A file that fails to parse is skipped with a logged warning, not a hard failure of the
whole directory scan — Tree-sitter's own error recovery means most real-world
work-in-progress code still produces a usable partial AST.

## Security

Read-only over trusted repo content.

## Testing

Exercised indirectly through every module that consumes `FileIndex` — `analyzer/mod.rs`
has its own unit tests for symbol extraction correctness per language.

## Implementation Order

Built in Phase 1 as the Structural Context Engine's foundation; unchanged in shape
through Phase 4, though consumers multiplied (Confidence Engine, test-impact/doc graphs
in Phase 4).

## Acceptance Criteria

A known symbol in a real source file is extracted with correct name, kind, and location.

## AI Coding Rules

If you fix the `SKIP_DIRS` inconsistency noted in Tradeoffs, update this document and
verify the Confidence Engine's own tests still pass — some may currently rely on
`target`/`node_modules` being walked (unlikely, but check).
