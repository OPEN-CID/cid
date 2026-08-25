# 043 — Sequence Diagrams

## Vision

The real request/response and event flows for CID's core interactions, as implemented —
not idealized versions.

## Goals

### Flow 1 — Golden path (Session creation through review)

```mermaid
sequenceDiagram
    participant U as User
    participant UI as Shell (any)
    participant Core
    participant Planner
    participant Implementer
    participant Reviewer

    U->>UI: New Session (repo, task, autonomy)
    UI->>Core: session.create
    Core->>Core: create_session + worktree (if worktree mode)
    Core->>Planner: generate_plan (background)
    Planner-->>Core: SessionPlan (Draft)
    Core-->>UI: session.plan.changed (WS)
    U->>UI: Edit + approve plan
    UI->>Core: session.plan.approve (session_token)
    Core->>Core: governance.can_approve_plan check
    Core-->>UI: SessionPlan (Approved, approved_by=user)
    U->>UI: Send message
    UI->>Core: session.send_message
    Core->>Core: role_runner.implementer_is_gated? (must be false)
    Core->>Implementer: process_message (tool-use loop)
    loop each tool call
        Implementer->>Core: execute_tool_with_approval
        Core-->>UI: session.tool_call.request (WS)
        U->>UI: Approve/Deny
        UI->>Core: session.approve_tool
        Core->>Implementer: execute_tool_direct_in (sandboxed if Autonomous)
    end
    U->>UI: Score confidence
    UI->>Core: confidence.score
    Core-->>UI: ConfidenceScore (9 signals)
    U->>UI: Close Session
    UI->>Core: session.close
    Core->>Reviewer: run_review (background)
    Reviewer-->>Core: SessionReview
    Core-->>UI: session.review.completed (WS)
```

### Flow 2 — Autonomous-mode tool call

```mermaid
sequenceDiagram
    participant Implementer
    participant Autonomy as AutonomyManager
    participant Governance
    participant Sandbox

    Implementer->>Autonomy: check_command(repo, command)
    alt allowed, no approval needed
        Autonomy-->>Implementer: PreApproved
        Implementer->>Sandbox: execute_sandboxed (path policy + kernel isolation)
        Sandbox-->>Implementer: Allowed/Blocked result
    else not on allow-list
        Autonomy-->>Implementer: Denied(reason)
        Implementer-->>Implementer: return denied, no execution
    end
```

Note: `Governance` is checked once at Session-creation time (can this Session even be
Autonomous), not per tool call — per-call gating is the Autonomy allow-list plus Sandbox.

## Non-Goals

Diagramming every RPC method — these two flows are the load-bearing ones; see
`034-API.md` for the full method inventory.

## Architecture

N/A — this document is the architecture-as-sequence view.

## Tradeoffs

N/A.

## Failure Modes

N/A — see the linked subsystem docs for failure-mode detail on each step.

## Security

The Autonomous-mode flow above is the literal security-critical path documented in
`031-Security.md` — this diagram is its concrete companion.

## Testing

Both flows are exercised end-to-end by real tests: Flow 1 by
`co_pilot_session_is_gated_until_a_plan_is_approved` plus
`tests/e2e/flow1.spec.ts`; Flow 2 by `autonomy_denies_a_command_outside_the_allowlist`
and the sandbox integration tests in `model/mod.rs`.

## Implementation Order

N/A.

## Acceptance Criteria

Both diagrams match the real code path as of this document's writing — verified by
cross-referencing against `roles/mod.rs`, `model/mod.rs`, and `autonomy/mod.rs` while
writing them.

## AI Coding Rules

If a flow changes (e.g., governance gets checked per-tool-call instead of at Session
creation), update the corresponding diagram in the same PR — a stale sequence diagram is
actively misleading, worse than none.
