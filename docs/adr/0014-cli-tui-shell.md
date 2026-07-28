# ADR 0014 — CLI/TUI shell: HTTP polling plus a WebSocket approval channel

**Status:** Accepted
**Context:** Phase 4, Part A (CLI/TUI shell)

## Context

Part A calls for "a new thin client of Core's existing JSON-RPC API — likely a Rust
TUI (e.g., via `ratatui`) — letting a developer drive Missions, review diffs, approve/deny
tool calls, and chat with the agent entirely from a terminal. No new Core functionality
required; this is a UI-only addition on top of a complete API." This closes a real gap:
Phases 0–3 shipped desktop, web, and mobile shells, but nothing served the CLI/TUI-first
developer persona Aider, opencode, and Claude Code itself already serve.

## Decision

**`cid-tui`**, a new workspace crate, built on `ratatui` + `crossterm`. Two data paths:

1. **State (Missions, repo channels, chat history): plain HTTP polling** of `/api/rpc`,
   on a fixed interval (1.5s). Simpler to reason about inside a synchronous terminal
   render loop than a persistent WebSocket, and the TUI's own refresh cadence already
   bounds staleness to something a human typing in a terminal won't notice.
2. **Pending tool-call approvals: the existing `/ws` WebSocket.** This is the one place
   polling genuinely doesn't work — Core has no "list pending approvals" query; a pending
   approval is *only* ever announced as a `mission.tool_call.request` push notification,
   the same one the desktop and web shells already consume. A background task listens on
   `/ws` and forwards decoded events into the render loop over an `mpsc` channel.

This was found, not assumed: the first implementation used HTTP polling exclusively, which
compiled and ran but could never populate the approval list — there was nothing to poll.
Adding the WS listener uses Core's *existing* API surface, not new functionality; it does
not change the "no new Core surface" constraint.

Layout mirrors the desktop/web shells' shape (Part 19) at terminal scale: a Mission list,
a thread pane, a composer, and an approvals strip that appears only when something is
actually pending. Keybindings: `j`/`k` or arrows to navigate, `Tab` to switch panes, `i`
to type, `Enter` to send or select, `a`/`d` to approve/deny, `q` to quit.

## Consequences

- **Verified against a live Core**, not just unit tests: connected to a running Core over
  HTTP, fetched a real repo channel and Mission, and rendered them correctly in the
  terminal frame buffer.
- Approval visibility is scoped to whichever Mission is currently selected — switching
  Missions clears the pending-approval list rather than tracking all Missions
  simultaneously. A developer running many parallel autonomous Missions and wanting a
  single "everything waiting on me" view would need a future enhancement; this is a
  reasonable v1 scope, not a silent gap (this document says so).
- If the WS connection drops, approval visibility degrades but the rest of the TUI (state
  browsing, sending messages) keeps working over HTTP — a lost push channel isn't a lost
  app.
- No new Core RPC methods, matching Part A's stated constraint exactly.
