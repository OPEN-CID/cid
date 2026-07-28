# 039 — AI Implementation Rules

## Vision

The operating rules that produced this codebase — consolidated here from Appendix A
Part 0 and this project's own real history, for any AI agent (or human) implementing the
next milestone.

## Goals

1. **A working phase beats a scaffolded everything.** If a requirement conflicts with
   shipping something working and tested, flag the conflict with a proposed resolution —
   don't silently cut corners or silently attempt the full scope anyway.
2. **No placeholder code presented as done.** A `TODO` mid-task is fine; reporting it as
   complete is not. If something is deferred, say so and name which phase it belongs to.
3. **Everything is reviewable.** Every phase ends in a state a human can run and evaluate
   — not a description of what would happen if they did.
4. **Ambiguity gets a documented default (an ADR), not a blocking question** — unless
   proceeding under any assumption would be unsafe or make the work useless if wrong.
5. **Checkpoint at phase boundaries.** Report what was built, what was deferred (and to
   which phase), known issues, honest test status, and a go/no-go recommendation.
6. **Read the relevant `docs/0XX-*.md` file before implementing a milestone that touches
   it**, and update the doc if implementation reveals it was wrong — code and doc should
   not drift apart (the Doc Template's own instruction).
7. **Verify, don't assume.** Run the tests. Read the actual file. This project's own
   history has multiple real instances (documented across `031-Security.md`,
   `014-Patch-Verification.md`, `015-Test-Impact-Analysis.md`) of a claim that looked
   correct on first read turning out to be wrong once actually exercised.
8. **A security-critical path change needs a real integration test**, not just a unit
   test of the isolated function — see `031-Security.md`'s Failure Modes for why this
   specific discipline exists.
9. **Delegating routine, isolated work to a weaker/faster model is reasonable; delegating
   changes to existing, load-bearing modules is not** — this project's own history
   includes an external process using a free-tier model that silently rewrote a working,
   tested 444-line file into ~96 lines of code that didn't compile, referencing modules
   that don't exist. Caught immediately by the next `cargo test` run and restored from
   git. Scope any such delegation to new, isolated files a human or the primary agent can
   easily diff and verify — never to an existing module without review.

## Non-Goals

Prescribing a specific agent framework or model — these rules apply regardless of which
AI system is doing the implementing.

## Architecture

N/A.

## Tradeoffs

N/A.

## Failure Modes

See rule 9 above for the one concrete, real instance of an AI-delegation failure mode
this project encountered and the corrective it produced.

## Security

Rule 8 above.

## Testing

Every rule here maps to a real, named incident in this project's own development history
where following it (or having to recover from not having followed it) mattered — this
document is not a hypothetical best-practices list.

## Implementation Order

N/A — these rules are constant across all phases.

## Acceptance Criteria

N/A.

## AI Coding Rules

This document *is* the AI coding rules — apply it to itself: if a new failure mode is
discovered in a future phase, add it here with the same specificity as rule 9, not as a
vague generality.
