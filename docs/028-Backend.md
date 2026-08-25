# 028 — Backend

## Vision

Consolidated view of Core's backend surface — this document indexes the per-subsystem
docs rather than duplicating them, since `004-System-Architecture.md` already covers the
shape and every subsystem has its own document.

## Goals

Core's managers, and where each is documented:

| Manager | Doc |
|---|---|
| `git::GitManager` | `016-Git-Engine.md` |
| `pty::PtyManager` | (Terminal — see Testing below; no dedicated doc, small surface) |
| `mcp::McpManager`, `mcp_tasks::McpTasksManager` | `023-MCP.md` |
| `model::ModelManager` | `009-Model-Scheduler.md` |
| `roles::RoleRunner` | `008-Agent-Operating-System.md`, `002-Product-Requirements.md` |
| `context_engine`, `semantic_engine` | `007-Context-Engine.md` |
| `confidence::ConfidenceEngine` | `014-Patch-Verification.md` |
| `role_profiles::RoleProfileManager` | `008-Agent-Operating-System.md` |
| `auth::AuthManager` | `031-Security.md` |
| `governance::GovernanceManager` | `017-Workspace-Manager.md` |
| `forges`, `trackers`, `github` | `023-MCP.md` sibling — see below |
| `decisions::DeploymentLog`, ADR listing | `013-Repository-Health.md` |
| `sandbox::SandboxManager` | `031-Security.md`, ADR 0011 |
| `access::AccessPolicy` | `031-Security.md`, ADR 0012 |
| `persistence::Persistence` | `021-Storage.md`, `035-Database.md` |

**Terminal (PTY)**: `pty::PtyManager` wraps `portable-pty` for a real, native PTY per
Session (ConPTY on Windows, Unix PTY on macOS/Linux), streamed over WebSocket as
`pty.output` notifications. Terminal output passes through `redact::redact_secrets`
before being persisted or streamed (`031-Security.md`).

**Forge bridges** (GitHub, GitLab, Bitbucket): `github::GitHubManager`
(Phase 1) and `forges::ForgeManager` (Phase 3, GitLab/Bitbucket) share the same
issue→Session trigger and PR/MR status-sync workflow, normalized to `ForgeIssue`/
`ForgeChangeRequest` shapes for the two newer providers.

**Tracker linkage** (Jira, Linear): `trackers::TrackerManager` — Session ↔ ticket linkage
only, deliberately not a tracker replacement (Part 1's non-goal).

## Non-Goals

Duplicating subsystem detail already covered elsewhere — this document is an index.

## Architecture

See `004-System-Architecture.md`.

## Failure Modes / Security / Testing

See each linked subsystem document.

## Implementation Order

See `041-Roadmap.md` for the full phase-by-phase build order.

## Acceptance Criteria

Every manager listed above is constructed in `Core::new`/`Core::new_in_memory` and
exposed through `AppState` — verified by the fact that `cid-core` compiles and its 302
unit tests + 56 integration tests pass against the real, fully-wired `Core`.

## AI Coding Rules

When adding a new manager, add a row to this table pointing at wherever its detailed doc
lives — this document exists specifically so a reader can find the right doc without
grepping the codebase first.
