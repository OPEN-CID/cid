# 030 — Enterprise

## Vision

Air-gapped operation and enterprise hardening — named in Part 15 as designed-to-be-
possible-later, explicitly not a Phase 0–5 deliverable.

## Goals

What exists today that an enterprise deployment would build on: local accounts and
Workspace roles (`031-Security.md`, ADR 0013), Workspace governance policy
(`017-Workspace-Manager.md`), and Core's local-first design (no required external
service beyond the model providers the user chooses to enable).

What does not exist: SSO/OIDC integration, air-gapped model bundling beyond "connect to
an already-running local runtime" (`009-Model-Scheduler.md`'s local-runtime detection),
compliance certifications, or a dedicated enterprise admin console beyond the governance
RPC surface.

## Non-Goals

Building any of the above speculatively. Every phase checkpoint through Phase 5 states
honestly that no real demand signal for enterprise hardening has been gathered — "not
yet" is treated as a complete, legitimate answer, per the Phase 4/5 build prompts' own
explicit instruction.

## Architecture

N/A beyond what's already documented in `017-Workspace-Manager.md` and `031-Security.md`.

## Failure Modes / Security / Testing

See `031-Security.md` for what the current local-auth model does and does not protect
against (ADR 0013's own honest limitations section) — the realistic starting point for
any future enterprise-hardening work, not a green field.

## Implementation Order

Not scheduled.

## Acceptance Criteria

N/A.

## AI Coding Rules

Do not build SSO/OIDC or air-gapped hardening without real demand evidence and a
dedicated ADR — this is the same discipline `029-Cloud.md` applies, for the same reason.
