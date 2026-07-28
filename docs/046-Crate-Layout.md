# 046 — Crate Layout (Phase 6)

## Vision

A reader new to the repository should be able to tell, from this file alone, what each
crate/workspace member is for and why it's separate from the others — without reading
every `Cargo.toml`.

## Goals

The workspace (`Cargo.toml`) has three members:

| Crate | What it is | Why it's separate |
|---|---|---|
| `cid-core` | The Rust/Tokio daemon — every real subsystem (git, PTY, MCP client, ACP host, model routing, context engine, auth, governance, sandbox, confidence engine, repo health, observability) and the JSON-RPC API surface (`src/api/router.rs`) that exposes them. Ships as both a library (`cid_core`, used by tests and `cid-tui`) and a binary (`cid-core`, the headless daemon `main.rs` starts). | Part 15's "One Core, Many Surfaces": every shell (desktop, web, CLI/TUI) is a thin client over this one process's JSON-RPC API. Keeping it a library-plus-binary, not binary-only, is what lets `cid-tui` and the integration test suite drive it in-process without spawning a subprocess. |
| `cid-tui` | A `ratatui`-based terminal client (`src/main.rs`, `api.rs`, `app.rs`, `events.rs`, `ui.rs`) — mission list, chat, tool-call/plan approval, over the same HTTP+WebSocket JSON-RPC API the desktop/web shells use. | The CLI-first persona (Part 34 of the Phase 5 prompt): a developer who wants to drive a Mission without a GUI. It is a client, not a fork of Core's logic — it holds no business logic beyond rendering and input handling. |
| `src-tauri` (package name `cid`) | The Tauri v2 desktop shell — bundles `cid-core` as a sidecar/embedded process and serves the `src/` React frontend as its webview content. | Phase 0's desktop deliverable (Part 15). Kept as its own crate because Tauri's build tooling (`tauri build`, icon/bundle config) expects a dedicated crate at a fixed location, not because it holds meaningfully different logic from Core. |

Outside the Rust workspace: `src/` (the React/TypeScript/Vite frontend, shared by the
desktop shell and the Phase 2 web shell — same bundle, different host process) and
`docs/` (this file's own directory).

## Non-Goals

This is not a build-order or dependency-graph document — `cargo tree` and
`cargo metadata` are the authoritative source for that and go stale-proof by
construction; this file explains *why* the boundaries are where they are, which those
tools cannot.

## Architecture

```
cid-core (lib + bin)  ──┬── used directly by ──> cid-tui (bin)
        │               │
        │               └── embedded/sidecar in ──> src-tauri (bin, "cid")
        │
        └── JSON-RPC 2.0 over HTTP+WS ──> src/ (React) ──> served by src-tauri's webview,
                                                             or by Core's headless mode (Phase 2 web shell)
```

`cid-core`'s internal module boundaries (`src/*/mod.rs`, 30+ modules as of Phase 6) are
intentionally *not* separate crates — Part 5's "roles, not an org chart" principle
applies here too: one compiling unit with clear module seams is easier to keep coherent
than 30 crates with their own version numbers and inter-crate API surfaces, for a system
this interconnected (nearly every module reaches into `persistence` and several reach
into `model` and `event_tx`).

## Tradeoffs

Splitting `cid-core` into smaller crates (e.g., a standalone `cid-git`, `cid-mcp`) would
give clearer compile-time boundaries and faster incremental builds for isolated changes,
at the cost of version-syncing multiple crates and more `pub` surface area between them.
Not done in Phase 0–6: no measured compile-time pain has justified it, and Part 0's own
operating rule ("a working Phase N beats a scaffolded everything") argues against
preemptive splitting.

## Failure Modes

A workspace member added without updating this file is the direct failure mode this doc
exists to prevent — the same category of drift ADR 0009's numbering collision was.

## Security

N/A — this is a structural document.

## Testing

`cargo metadata --format-version1 | jq '.workspace_members'` (or simply `cat
Cargo.toml`) is the ground truth this file must never contradict; re-verified as part of
writing this document (Phase 6).

## Implementation Order

N/A — descriptive, not sequenced work.

## Acceptance Criteria

Every entry in the root `Cargo.toml`'s `[workspace] members` list has a row above.

## AI Coding Rules

When adding a new workspace member, add a row here in the same commit — do not let this
file drift the way the ADR numbering did (see `042-ADRs.md`'s own note on that).
