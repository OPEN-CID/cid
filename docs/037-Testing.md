# 037 — Testing

## Vision

Scoped to what each phase actually needs (Part 21), not the full unit/integration/
property/benchmark/fuzz/snapshot/UI/E2E/cross-platform suite as a Phase 0 gate — but by
Phase 4, real coverage across every category Part 21 eventually asks for.

## Goals

Current totals (`cargo test --workspace`, `npm test`):

| Suite | Tests | What it covers |
|---|---|---|
| `cid-core` unit (`cargo test -p cid-core --lib`) | 302 | Every manager's own logic |
| `cid-core/tests/api_integration.rs` | 56 | Full RPC dispatch over real HTTP/WS |
| `cid-core/tests/protocol_fuzz.rs` | 9 (2 `proptest`) | JSON-RPC/MCP/ACP boundaries never 5xx or panic |
| `cid-core/tests/worktree_property.rs` | 11 (3 `proptest`) | Worktree lifecycle invariants |
| `cid-core/tests/performance_budget.rs` | 5 | Real measured numbers against Part 17 budgets |
| `cid-tui` | 3 | JSON-RPC client construction |
| `npm test` (Vitest) | 2 | React component smoke tests |
| `npm run build` / `npx tsc --noEmit` | — | Frontend compiles and typechecks clean |
| `tests/e2e/*.spec.ts` (Playwright) | 4 spec files | Flow 1 (golden path) end-to-end |

**392 total** across the Rust/TypeScript test runners as of this document.

## Non-Goals

100% line coverage as a target — Part 0's own discipline is "every non-trivial claim has
a test," not a coverage percentage pursued for its own sake.

## Architecture

Two Rust test tiers: `#[cfg(test)] mod tests` inside each module (fast, no I/O, the bulk
of the 302) and `cid-core/tests/*.rs` (real HTTP/WS against a spawned Core instance, no
mocking — `start_core()`'s helper in `api_integration.rs` is reused across all four
integration test files).

## Data Structures

N/A.

## Traits / Interfaces

`proptest!` macros in `protocol_fuzz.rs` and `worktree_property.rs` generate arbitrary
inputs (method names, params shapes, branch names) rather than hand-picked examples,
specifically to catch edge cases hand-picked examples would miss.

## Storage Layout

Every Rust integration/unit test uses `Persistence::new_in_memory()` or
`Core::new_in_memory()` — no test touches a real file-based SQLite DB or a real
network-reachable Core outside its own spawned instance.

## Performance Targets

See `032-Performance.md` — the `performance_budget.rs` suite *is* the performance
testing, not a separate concern.

## Tradeoffs

CI runs this full 392-test total as of the Phase 5 CI-extension fix documented in
`036-CI-CD.md` — `cargo test --workspace --exclude cid --all-features`, matching the
local command exactly.

## Failure Modes — what testing in this project has actually caught

This is not hypothetical: real bugs found by real tests during this project's own
development, not anticipated defensively —

- The sandbox boundary tautology (ADR 0011) — caught by rewriting the test to check
  ground-truth filesystem state instead of an exit code.
- The Confidence Engine's architecture-validation false-positive/false-negative pair
  (`014-Patch-Verification.md`) — caught by writing a test for the specific scenario
  ("an unrelated rule must not flag an unrelated patch") the original bug got backwards.
- The test-impact graph's definitions-vs-references bug (`015-Test-Impact-Analysis.md`)
  — caught by an end-to-end integration test against real Tree-sitter output, after unit
  tests with unrealistic fixtures passed.
- A background process (outside this project's own test suite) that corrupted
  `subagent/mod.rs` mid-session — caught immediately by the routine `cargo test` pass
  that followed, restored from git within minutes.

## Security

Security-critical paths get dedicated integration tests, not just unit tests — see
`031-Security.md`'s Testing section.

## Testing

This document is the testing document.

## Implementation Order

Phase 0: unit tests + one golden-path E2E. Phase 1: integration tests for the headless
API, Skills-resolution coverage. Phase 2: cross-shell E2E (partial), MCP Apps unit
coverage. Phase 3: protocol fuzzing, worktree property tests, performance budgets — Part
21's full Phase 3+ floor. Phase 4: Confidence Engine, role-profile enforcement, graph
tests added alongside their features, not as an afterthought pass.

## Acceptance Criteria

`cargo test --workspace` and `npm test` both exit 0, and every claim made in a checkpoint
report is traceable to a specific passing test named in this document or its linked
subsystem docs.

## AI Coding Rules

Write the test that would have caught the bug, not just a test that exercises the fixed
code — every "Failure Modes — real bugs found" entry above is precisely a test written
this way. When you find a class of bug, ask whether a similar bug could exist elsewhere
in the same pattern (e.g., after finding the test-impact graph's definitions-vs-references
bug, the same class of check was applied to `extract_symbols_from_content`'s `pub fn`
handling).
