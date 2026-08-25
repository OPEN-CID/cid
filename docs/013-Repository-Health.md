# 013 — Repository Health

## Vision

The Action History panel (Part 13) — a transparent, filterable, exportable log of every
tool call, terminal command, file edit, and approval decision a Session's agents made —
plus the health signals (stale docs, test coverage gaps) the Phase 4 graphs surface.

## Goals

- **History panel**: chronological, filterable (by actor/type/approval-status) log,
  exportable as Markdown/JSON (`src/components/history/HistoryPanel.tsx`).
- **Stale doc detection** (Phase 4): `semantic_engine.docs.stale`
  (`006-Repository-Digital-Twin.md`).
- **Test coverage visibility** (Phase 4): `semantic_engine.test_impact.entries` — which
  symbols have no covering test at all is derivable from comparing this against the full
  symbol list, though no dedicated "uncovered symbols" RPC method exists yet (a real,
  named gap, not silently claimed as built).

## Non-Goals

A full "Test Health dashboard" with duplicate-test detection via AST similarity — named
in the original founding-brief vision (`cid_project_blueprint.md`) but not built in
Phases 0–4; the test-impact graph gives coverage visibility, not duplicate detection.
Named here explicitly as unbuilt rather than silently assumed.

## Architecture

History entries are persisted per-Session (`ChatMessage` with `tool_calls`,
`SessionReview`, `ConfidenceScore`, `DeploymentRecord` — all queryable per-Session and
surfaced in the thread/History panel together).

## Data Structures

`ToolCall` (`api/types.rs`) carries actor, action, target, timestamp, and result status
per entry.

## Traits / Interfaces

RPC: `message.list` (the underlying data History renders), `semantic_engine.docs.stale`,
`semantic_engine.test_impact.entries`.

## Storage Layout

SQLite `messages` table, `tool_calls` embedded as JSON per message.

## Performance Targets

Not separately benchmarked — bounded by message list size per Session, which is small at
current usage scale.

## Tradeoffs

No duplicate-test AST-similarity detection (see Non-Goals) — a real scope gap relative to
the original blueprint's ambition, named honestly rather than glossed over.

## Failure Modes

N/A beyond what's covered in `007-Context-Engine.md` and `006-Repository-Digital-Twin.md`
for the underlying data sources.

## Security

History is per-Session and inherits the same access boundary as the Session itself — no
separate access control layer, a reasonable scope for a single-Workspace-at-a-time
product.

## Testing

Covered indirectly through `message.list` tests in `api_integration.rs` and the
Confidence/Review/Deployment tests that populate History-visible data.

## Implementation Order

History panel (Phase 0) → stale-doc/test-impact visibility (Phase 4). Duplicate-test
detection remains unbuilt.

## Acceptance Criteria

Every tool call a Session's agents make is visible in the History panel with actor,
action, and result — the transparency baseline Cline established, per the founding
brief's Part 13.

## AI Coding Rules

If you build duplicate-test detection, update this document's Non-Goals section to move
it into Goals — don't leave a stale "not built" claim next to a feature that now exists.
