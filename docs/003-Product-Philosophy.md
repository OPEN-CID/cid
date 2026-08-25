# 003 — Product Philosophy

## Vision

The operating principles that produced the actual architecture, restated here as
philosophy rather than requirements — the "why" behind decisions documented elsewhere as
"what."

## Goals

**A working phase beats a scaffolded everything.** Every phase in this project's real
history shipped something runnable and tested before moving on — 392 passing tests as of
Phase 4, not a directory of stubs. When Part 0's own rule 1 was violated (the Confidence
Engine built but never wired into `lib.rs` in an earlier session), the next audit caught
it and fixed it rather than letting the gap compound.

**No placeholder code presented as done.** A `TODO` mid-implementation is fine; reporting
something complete when it isn't is not. This project's own history has two concrete
counterexamples that got caught: the Phase 2 sandbox boundary test that asserted a
tautology (`passed || !passed`) instead of a real guarantee, and the dead Confidence
Engine module. Both are documented honestly in their respective checkpoints
(`docs/CHECKPOINT-Phase2.md`) rather than smoothed over.

**Ambiguity gets a documented default, not a blocking question.** Every non-obvious
choice — git backend (`git2-rs` over `gitoxide`, ADR 0002), the minimal local-auth model
(ADR 0013), HTTP polling plus a WS approval channel for the TUI (ADR 0014) — is recorded
as an ADR with what was chosen, what was given up, and why.

**Human-in-the-loop is enforced, not suggested.** The plan-approval gate lives in Core
(`cid-core/src/roles/mod.rs`), checked before `session.send_message` does anything —
not a UI convention a client could bypass.

## Non-Goals

Chasing novelty for its own sake. Every technology choice in `018-Native-Editor.md` and
elsewhere defers to a proven component unless real profiling evidence justifies building
one from scratch — evidence that, as of Phase 4, does not yet exist.

## Architecture

N/A — philosophy document.

## Tradeoffs

This philosophy trades "impressive on first read" for "actually works when you run it."
The founding brief's own critique of its v1.0 predecessor — a 12-agent org chart, a
from-scratch GPU editor, a full knowledge graph, all as day-one requirements — is the
clearest illustration of what this philosophy exists to prevent.

## Failure Modes

Optimizing for the appearance of completeness (a long checkpoint, a big feature list)
over verified reality. The corrective is always the same: run `cargo test --workspace`
and `npm test`, read the actual diff, and trust that over any summary — including this
one.

## Security

Security-critical code (the sandbox boundary, the plan-approval gate, the access token
check) gets a dedicated review pass and its own test proving the specific guarantee holds
— not just a unit test that the function returns without panicking. See ADR 0011 for the
concrete example of what happens when this discipline slips (a tautological test) and how
it was corrected.

## Testing

See `037-Testing.md`.

## Implementation Order

N/A — this is a constant across all phases, not a phased deliverable.

## Acceptance Criteria

N/A — philosophy, not a checkable requirement.

## AI Coding Rules

- A stub is acceptable mid-task; reporting a stub as done is not.
- When you find a gap in earlier work — dead code, a bug, an untested claim — fix it and
  say so plainly in the next checkpoint, the way this project's Phase 2 and Phase 4
  checkpoints do. Do not quietly paper over it.
- Verify before claiming. "Tests pass" means you ran them in this session, not that they
  probably would.
