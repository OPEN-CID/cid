# 024 — Plugin SDK

## Vision

Assess whether CID needs a bespoke plugin/extension SDK beyond what MCP, `AGENTS.md`/
`SKILL.md`, and role profiles already provide.

## Goals

None currently — no dedicated plugin SDK exists or is scheduled. CID's actual
extensibility surfaces, all real and built:

- **MCP servers** (`023-MCP.md`) — the mechanism for connecting external tools.
- **Skills / `AGENTS.md`** (`010-Memory-System.md`) — the mechanism for customizing agent
  behavior and context per Workspace/Repo.
- **Role profiles** (`008-Agent-Operating-System.md`) — the mechanism for defining
  specialized agent configurations.

Between these three, most of what a "plugin SDK" would exist to provide (custom tools,
custom context, custom agent behavior) is already covered by standards-based, UI-editable
mechanisms rather than a bespoke code-plugin API.

## Non-Goals

A CID-specific plugin API requiring compiled extensions, WASM modules, or a separate
process-isolation story for third-party code. The founding brief names "pluggable skills
or extensions" as a vision-level aspiration (`cid_project_blueprint.md`) but no phase
prompt (0–5) scoped a concrete plugin SDK as a deliverable — this document exists to say
so plainly rather than leave the aspiration ambiguously unaddressed.

## Architecture

N/A — no plugin runtime exists.

## Tradeoffs

Not building a separate plugin SDK avoids a real security and maintenance burden (running
arbitrary third-party code safely) that MCP's client-server model already solves for the
"connect external capability" use case, and Skills/role-profiles already solve for the
"customize behavior" use case — without CID needing to invent and secure its own
extension-loading mechanism.

## Failure Modes

N/A.

## Security

N/A — no plugin execution surface exists to secure.

## Testing

N/A.

## Implementation Order

Not scheduled. Revisit only if a real use case emerges that MCP, Skills, and role
profiles together genuinely cannot serve — not before.

## Acceptance Criteria

N/A.

## AI Coding Rules

Before building a "plugin" for some new capability, check whether it's actually an MCP
server, a Skill, or a role profile in disguise — the three existing extensibility
mechanisms cover the overwhelming majority of real requests this shape of feature
produces.
