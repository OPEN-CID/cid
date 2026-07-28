# 042 — ADRs

## Vision

Index every Architecture Decision Record in `docs/adr/`, so a reader can find the
reasoning behind a specific technical choice without grepping.

## Goals

| ADR | Decision |
|---|---|
| 0001 | JSON-RPC 2.0 as the transport, over HTTP + WebSocket |
| 0002 | `git2-rs` as the default git backend (writes included); `gix` opportunistic for hot read paths only |
| 0003 | `portable-pty` for cross-platform native PTY |
| 0004 | SQLite schema approach (additive migrations, no framework) |
| 0005 | MCP client scope for Phase 0 (stdio framing documented as an honest stub) |
| 0006 | Editor strategy: CodeMirror 6 + Monaco, not a native engine — CodeMirror half never built; Monaco-only shipped (see the ADR's own superseding note) |
| 0007 | Anthropic-only for Phase 0 |
| 0008 | Phase 0.1 polish fixes (WS broadcast, secret redaction, icons, per-hunk UI) |
| 0009 | ACP host design |
| 0010 | Mobile technology bake-off: Tauri v2 Mobile selected |
| 0011 | Windows sandbox boundary: Job Objects don't confine the filesystem; two-layer real design |
| 0012 | Core access control: bearer token mandatory for non-loopback binds |
| 0013 | Local auth model: Argon2id accounts, no SSO, explicit limitations |
| 0014 | CLI/TUI shell: HTTP polling + existing WS for approvals |
| 0015 | Multi-provider routing design |
| 0016 | Dev Container: built, scoped to the browser+Core loop only (not Tauri) |

**Corrected during this documentation pass**: two ADRs were both numbered 0009
(`0009-acp-host.md` and `0009-multi-provider-routing.md`) — a real numbering collision
from earlier phases. Renumbered the multi-provider-routing ADR to 0015 (no other file
referenced it by name, so this was a safe rename).

## Non-Goals

Duplicating each ADR's content here — this is an index; read the linked file for the
full decision, alternatives considered, and consequences.

## Architecture

N/A.

## Tradeoffs

N/A.

## Failure Modes

The 0009 numbering collision (see Goals) was found and fixed during this documentation
pass — a minor documentation hygiene issue, not a functional one, but real.

## Security

ADRs 0011, 0012, 0013 are security-relevant — see `031-Security.md` for the consolidated
security view built from all three.

## Testing

N/A.

## Implementation Order

Chronological by ADR number (with the one noted collision).

## Acceptance Criteria

Every ADR file in `docs/adr/` has a corresponding row in this table.

## AI Coding Rules

When adding a new ADR, check the highest existing number first — the 0009 collision in
this codebase's own history is the direct consequence of not doing so.
