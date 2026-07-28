# 025 — Desktop Architecture

## Vision

The primary Phase 0 shell: Tauri v2 wrapping the same React bundle every other shell
uses, targeting macOS and Windows.

## Goals

`src-tauri/` — Tauri v2 app shell. Core runs as a sidecar/co-located process; the desktop
UI is the same `src/App.tsx` React tree as the Web Shell, with Tauri-specific native
capabilities (window chrome, native file pickers where used) layered on top via Tauri's
capability system.

## Non-Goals

A separate desktop-only codebase — the explicit anti-goal Part 15 exists to prevent
("one Core, many surfaces," not one app per platform).

## Architecture

See `004-System-Architecture.md`'s component diagram — Desktop Shell is one of four
clients of Core's JSON-RPC API, distinguished from the Web Shell only by its Tauri
wrapper and native capability grants.

## Data Structures

N/A beyond what the shared React app already defines (`src/lib/api.ts`'s client, shared
across shells).

## Traits / Interfaces

Tauri capability whitelist (`src-tauri/capabilities/default.json`) restricts what native
APIs the webview may call — `core:default` and `core:window:*` only, per the Phase 0
polish pass that removed deprecated/plugin-requiring permissions
(`docs/CHECKPOINT-Phase0-Final.md`).

## Storage Layout

Same SQLite/Tantivy storage as any other Core instance — the desktop shell doesn't
introduce separate local storage.

## Performance Targets

Part 17's <150MB idle, <2s cold start budgets apply to the full desktop app (Core +
Tauri webview), not independently re-measured in this documentation pass — the
in-process benchmarks in `004-System-Architecture.md` measure Core alone.

## Tradeoffs

Real `tauri dev`/`tauri build` passes require MSVC Build Tools on Windows (a genuine
environment dependency, not a CID design choice) — documented in
`docs/CHECKPOINT-Phase0-Final.md` as a condition that was met during Phase 0 hardening.

## Failure Modes

Icon/build-config issues (invalid ICO format, mismatched Tauri feature flags) were real,
found-and-fixed problems during Phase 0 — see `docs/CHECKPOINT-Phase0-Final.md`'s
Condition 1 and Condition 3 for the specific fixes.

## Security

Tauri's capability whitelist is the desktop-specific security boundary, restricting which
native APIs the webview can reach — separate from, and in addition to, Core's own
`access::AccessPolicy` (`031-Security.md`), since the desktop shell talks to a
loopback-bound Core by default and doesn't need the token-based access control a
remotely-reachable Core requires.

## Testing

E2E flows (`tests/e2e/flow1.spec.ts` etc.) run against the browser+standalone-Core dev
loop primarily; a real `tauri dev` pass is required before calling a phase done per the
Phase 0/1 prompts' own stated caveat, and was performed during Phase 0 hardening.

## Implementation Order

Scaffolded in Phase 0, hardened (icons, capability whitelist, MSVC toolchain) in the
Phase 0 polish pass; no structural change through Phase 4.

## Acceptance Criteria

`npm run tauri:dev` launches a working desktop app against a running Core, with the same
functional coverage as the Web Shell.

## AI Coding Rules

Any new native capability (file picker, notification, etc.) needs an explicit grant in
`src-tauri/capabilities/default.json` — Tauri v2 denies by default, unlike Tauri v1's
broader default allowlist.
