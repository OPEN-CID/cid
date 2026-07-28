# 032 — Performance

## Vision

Performance targets as budgets to validate after real measurement, never as
pre-measurement guarantees (Part 17's explicit discipline, corrected from the founding
brief's earlier draft that asserted numbers before any code existed).

## Goals

Part 17's stated budgets: <150MB idle memory with optional features off, <2s cold start,
git status/diff feeling instant on repos under ~50k files.

## Non-Goals

Treating any number in this document as a guarantee for the shipped desktop app under
real disk-backed load at scale — every measurement below is real but taken in-process
against an in-memory database, which is explicitly weaker evidence than a packaged binary
under real usage (stated plainly, not hidden).

## Architecture

`cid-core/tests/performance_budget.rs` — 5 tests, each printing its real measured value
(`println!`) as well as asserting it clears a generous threshold above the actual budget
(the assertion catches "10x slower," not "exceeds the exact budget by 5%" — deliberately
loose given CI-runner variance).

## Benchmarks

Real numbers from this environment, reproduced from `004-System-Architecture.md`:

| Measurement | Result |
|---|---|
| Cold start to first `/health` response | 12.5ms |
| `Core::new_in_memory` construction | <500ms |
| `git status`, 50-file repo | 2.99ms |
| Repository scan, 200 files (Tantivy indexing) | 57.7ms |
| 100 concurrent RPC calls | 26ms, 100/100 succeeded |

## Tradeoffs

No memory-usage benchmark exists in the automated suite — the <150MB idle target is
stated in Part 17 but not mechanically verified by a test in this codebase, a real gap
named honestly rather than claimed as covered.

## Failure Modes

A CI runner slower than this development environment could, in principle, cause the
generously-thresholded assertions to fail even though nothing regressed — mitigated by
the thresholds being set well above the actual measured values specifically to absorb
that variance.

## Security

N/A.

## Testing

See Benchmarks above; `cargo test -p cid-core --test performance_budget -- --nocapture`
prints the real numbers.

## Implementation Order

Built in Phase 3 as part of Part 21's cross-platform/performance testing floor.

## Acceptance Criteria

Every number in the Benchmarks table is reproducible by running the named test command
in this repository, not asserted from memory.

## AI Coding Rules

If you add a memory-usage benchmark (closing the gap named in Tradeoffs), update this
document's Benchmarks table with the real measured number — do not estimate one.
