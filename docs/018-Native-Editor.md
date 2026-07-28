# 018 — Native Editor

## Vision

State plainly, per this document's own template instruction: **CID does not have a
native rendering engine, and does not build one in Phases 0–4.** This document exists so
a reader doesn't have to infer that from silence.

## Goals

None — there is no native editor to build goals around. CID uses Monaco (full pane),
documented in `012-Semantic-Editing.md`. **Correction (2026-07-27,
`050-Gold-Standard-Review.md` F5):** an inline CodeMirror 6 editor was originally planned
alongside Monaco but never built — there is no `codemirror` dependency in this
repository. Monaco alone is the real, shipped editor strategy.

## Non-Goals

Building a from-scratch, GPU-rendered editor (rope buffer, Tree-sitter, SCIP, incremental
parser, `wgpu` renderer) — the exact ask the original v1.0 brief made, and the exact thing
Zed's own history argues against attempting casually: Zed — built by Tree-sitter's own
creators, with $32M in dedicated funding — took roughly five years to reach 1.0 doing
precisely this. That evidence doesn't compress on request.

## Architecture

N/A — no native rendering architecture exists. See `012-Semantic-Editing.md` for the
actual editor architecture (Monaco + ACP pop-out).

## Tradeoffs

Deferring to proven components costs CID a differentiated rendering story; it buys a
working product in Phases 0–4 instead of a multi-year editor project with nothing else
shipped in the meantime.

## Failure Modes

N/A.

## Security

N/A.

## Testing

N/A.

## Implementation Order

Not scheduled. Revisit only if Phase 5+ profiling produces real evidence that Monaco
rendering is an actual bottleneck (Part 22's own "reconsider, don't assume" framing for
anything in this bucket). As of Phase 5's checkpoint, no such evidence exists — see
`041-Roadmap.md`.

## Acceptance Criteria

N/A — there is nothing to accept.

## AI Coding Rules

Do not write speculative architecture for a native editor in this document or anywhere
else. If Phase 5+ profiling ever produces real evidence justifying reconsideration, that
evidence — not enthusiasm — is what reopens this question, and this document should be
rewritten from that evidence, not extended preemptively.
