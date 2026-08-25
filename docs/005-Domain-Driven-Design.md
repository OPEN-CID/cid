# 005 — Domain-Driven Design

## Vision

Name CID's core domain concepts once, precisely, so every doc and every module uses the
same vocabulary rather than drifting synonyms (a "Session" is never a "Session" is never
a "Task" elsewhere in the codebase).

## Goals

**Workspace** — top-level container; org/team scope. One default Workspace in Phase 0–2;
multi-user via `auth`/`governance` from Phase 3.

**Repo Channel** — one connected git repository; the primary navigation unit. Struct:
`RepoChannel` (`api/types.rs`).

**Session** — one unit of work; the direct analogue of a Slack thread. Owns a
`IsolationMode` (worktree/shared), an `AutonomyLevel`, a `SessionPlan`, zero or more
`SessionReview`s, `ChatMessage`s, `ConfidenceScore`s, `DeploymentRecord`s, and
`TrackerLink`s.

**SessionPlan** — the Planner's editable Requirements/Approach/Steps document; has a
`SessionPlanStatus` (Draft/Approved/Rejected). Approving a plan opens the Implementer
gate; editing an approved plan returns it to Draft (`roles/mod.rs`).

**SessionReview** — the Reviewer's pass over a Session's diff; has a `ReviewVerdict`
(Clean/CommentsOnly/ChangesRequested/NotRun) and a list of `ReviewFinding`s.

**RoleProfile** — a named, configurable prompt + model config + tool-permission set
(Phase 4), distinct from the three built-in roles (Planner/Implementer/Reviewer). Scoped
to a Workspace or a Repo Channel.

**Patch** (confidence domain) — the unit the Confidence Engine scores: a target file,
its new content, extracted symbol references, and an optional diff.

**Session** (auth domain) — an authenticated user's bearer token plus role, distinct from
`IsolationMode` (worktree/shared) — the same word means two different things in two
different domains, a known naming collision documented here rather than silently
tolerated.

## Non-Goals

A formal bounded-context diagram or event-storming artifact — the domain is small enough
(roughly a dozen core types) that a glossary suffices; a heavier DDD apparatus would be
over-engineering for this codebase's actual size.

## Architecture

Domain types are concentrated in `cid-core/src/api/types.rs` (the shared vocabulary every
manager and the router use) rather than duplicated per-module — a deliberate anti-drift
choice.

## Data Structures

See `cid-core/src/api/types.rs` for the canonical definitions of every noun above.

## Tradeoffs

Centralizing types in one `api/types.rs` file (1,100+ lines) trades module encapsulation
for a single source of truth that can't drift into three slightly-different `Session`
structs across modules — judged worth it at this codebase's size.

## Failure Modes

The `Session` name collision (auth vs. git session mode) is a real, current risk for
confusion in new code. Mitigated by this document naming it explicitly; not yet
mitigated by renaming either type, which would be a larger, separately-considered change.

## Security

N/A — vocabulary document.

## Testing

N/A — vocabulary document; the types themselves are exercised by every test that touches
them.

## Implementation Order

N/A — descriptive of the current, already-built domain model.

## Acceptance Criteria

Every noun in this glossary has exactly one struct definition in the codebase matching
it.

## AI Coding Rules

Before introducing a new domain noun, check this document and `api/types.rs` for an
existing one that already means what you're about to name — the `Session`/`IsolationMode`
collision above is the cautionary example of what happens when this isn't checked.
