# 001 — Competitive Analysis

## Vision

Understand where CID sits relative to the tools developers already use, so design
decisions build on what the market has already proven rather than re-deriving it.

## Goals

- Identify what each adjacent tool validates about CID's own bets (worktree isolation,
  plan-approval gates, headless server mode, ACP as an editor-integration standard).
- Name CID's actual white space honestly, not aspirationally.

## Non-Goals

- Continuous competitive tracking — this is a point-in-time analysis (dated July 2026 in
  the founding brief) informing initial design, not a maintained market-intelligence feed.

## Architecture

Not applicable — this is a research document, not a running system.

## Research

The founding brief's Part 2 (reproduced in full in each `CID-PhaseN-Build-Prompt.md`
under Appendix A) is the authoritative, dated competitive analysis this project was built
against. Rather than duplicate it here, the summary below extracts what it means for
CID's actual architecture; see Appendix A Part 2 for the full sourced table.

**IDE-first with chat bolted on** (Cursor, Devin Desktop, Kiro, Junie): validate
plan-approval gates (Kiro's `requirements.md`/`design.md`/`tasks.md`, Junie's Plan Mode),
premium-planner/cheap-implementer model splits (Junie's own stated advice), and ACP as a
real cross-editor standard (Junie rebuilt on it; Devin Desktop shipped ACP support day
one).

**Terminal-first single-session agents** (Aider, opencode, Claude Code): validate
headless server mode (`opencode serve` is the direct precedent for Core's `--host`/`--port`
CLI and the `cid-tui` shell) and atomic per-change commits (Aider's pattern, mirrored in
`cid-core/src/roles/mod.rs`'s Implementer discipline).

**Agent multiplexers** (Herdr, cmux, amux, dmux, workmux, and a dozen others): validate
the worktree-per-unit-of-work mechanic hard — but per Part 1's own analysis, this category
is now crowded at the session-manager layer. CID's differentiation is not the worktree
mechanic (table stakes) but the fuller platform around it: chat/Mission model, embedded
editor, cross-platform reach, model routing, governance.

**Fully autonomous cloud agents** (Devin Cloud, Copilot's coding agent): validate the
issue→Mission trigger pattern, mirrored in `cid-core/src/github/mod.rs` and
`cid-core/src/forges/mod.rs`.

## Open Source Comparison

| Tool | What CID took from it | Where in CID |
|---|---|---|
| opencode | Headless server mode | `cid-core/src/main.rs`'s `--host`/`--port`/`--auth-token` flags |
| Aider | Atomic per-change commits, architect/editor model split | `cid-core/src/roles/mod.rs`, per-role provider config |
| Cline | Every tool call shown, human approves | Co-Pilot autonomy's per-call approval in `cid-core/src/model/mod.rs` |
| Zed / JetBrains (ACP) | Agent-editor integration as a standard, not a bespoke protocol | `cid-core/src/acp/mod.rs` |
| Kiro / Junie | Plan-before-code, human-editable plan gate | `cid-core/src/roles/mod.rs`'s plan-approval enforcement |
| Warp | Feature-flag-everything, terminal→full-ADE slider | Part 17's "heavy features off by default" |

## Tradeoffs

Naming this table risks the analysis going stale within months in a fast-moving market.
Accepted deliberately: the value here is in what each row justified about CID's own
architecture, which doesn't expire even if the specific competitor's feature set changes.

## Failure Modes

Treating this document as current beyond its stated date. Anyone using it to make a new
decision should re-verify the specific claim, not cite this document as ongoing truth —
Part 24 of the founding brief models this discipline directly (it re-verified v2.0's own
claims and found several needed correction).

## Security

N/A — research document.

## Testing

N/A — research document.

## Implementation Order

N/A — informed Phase 0 design before any code existed.

## Acceptance Criteria

Every row in the Open Source Comparison table names a real file that exists in this
repository implementing the pattern credited to it.

## AI Coding Rules

Do not add a new competitor row without checking it's still accurate as of the date
you're reading this — this document is explicitly dated, not continuously maintained.
