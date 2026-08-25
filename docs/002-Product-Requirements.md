# 002 — Product Requirements

## Vision

The concrete, checkable requirements behind the vision in `000-Executive-Vision.md`,
organized by the Workspace → Repo Channel → Session Thread model (Part 3 of the founding
brief).

## Goals

**Workspace**: holds Skills library, connector directory (MCP servers, model providers,
bridges), and — as of Phase 3 — user accounts and roles (`cid-core/src/auth`,
`cid-core/src/governance`).

**Repo Channel**: connects one local git repo; auto-detects `AGENTS.md`
(`cid-core/src/context/mod.rs`); pins enabled MCP servers (least-privilege subset of the
Workspace registry); shows live Session status.

**Session Thread**: one unit of work. Requires a Isolation (worktree default / shared
clone, `cid-core/src/git/mod.rs`) and an Autonomy Level (Manual / Co-Pilot / Autonomous,
`cid-core/src/api/types.rs::AutonomyLevel`) at creation. Chat stream interleaves human
messages, agent messages, inline diff/plan/tool-call cards, and — since Phase 4 —
Confidence score cards (`src/components/confidence/ConfidenceCard.tsx`).

**Golden path (Flow 1)**: connect repo → new Session → Planner proposes plan → human
approves → Implementer executes with per-tool approval → diff accumulates → per-hunk
review → Reviewer pass → merge/PR. Exercised end-to-end by `tests/e2e/flow1.spec.ts`.

## Non-Goals

See `000-Executive-Vision.md`'s Non-Goals — restated in full there rather than duplicated.

## Architecture

See `004-System-Architecture.md`.

## Data Structures

Core domain types live in `cid-core/src/api/types.rs`: `Workspace`, `RepoChannel`,
`Session`, `ChatMessage`, `SessionPlan`, `SessionReview`, `AutonomyLevel`, `IsolationMode`.

## Tradeoffs

Single-user-local through Phase 2, multi-user via local accounts (not SSO) from Phase 3 —
see ADR 0013. This keeps the product buildable without a server component CID doesn't
otherwise need, at the cost of not being enterprise-identity-ready without further work
(explicitly named as a Phase 4+ reconsideration point, not built speculatively).

## Failure Modes

A Session created with Autonomous autonomy but no Workspace governance policy configured
is refused, not silently downgraded — verified by
`creating_an_autonomous_session_is_refused_by_default_policy` in
`cid-core/tests/api_integration.rs`.

## Security

See `031-Security.md`.

## Testing

`cid-core/tests/api_integration.rs` exercises the full Workspace → Repo → Session
lifecycle over real HTTP against a running Core — not handler unit tests, the actual wire
contract every shell uses.

## Implementation Order

Phase 0: single Workspace, single Repo Channel at a time, Co-Pilot only. Phase 1: plan
gate, ACP, multi-provider. Phase 2: web shell, subagents, sandboxing. Phase 3: accounts,
governance, mobile. Phase 4: Confidence Engine, role profiles, Decisions/Deployment,
CLI/TUI. See `041-Roadmap.md`.

## Acceptance Criteria

Flow 1 (golden path) passes as an automated E2E test. Autonomous Sessions are refused
without Workspace policy permitting them. Plan approval is enforced in Core (verified by
`co_pilot_session_is_gated_until_a_plan_is_approved`), not merely suggested by the UI.

## AI Coding Rules

New product requirements go here, cross-referenced to the file(s) implementing them and
the test(s) proving they work — a requirement without a test is a wish, not a
requirement.
