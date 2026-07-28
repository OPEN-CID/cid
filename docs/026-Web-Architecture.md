# 026 — Web Architecture

## Vision

The same React bundle as the desktop shell, served by Core running in headless server
mode, reached over the same JSON-RPC API — no separate web backend to build or keep in
sync (Phase 2).

## Goals

- `ConnectionBanner`, `HealthDashboard`, `AccessControlPanel` (`src/components/WebShell.tsx`)
  — connection status with exponential backoff, live health/client-count display, and
  access-control visibility that reads Core's *real* `/health` state (`auth_required`,
  `loopback_only`) rather than local-only UI toggles.
- Explicit origin allow-list CORS (`access::AccessPolicy`) rather than `Any` — the fix for
  a real, found gap (see Failure Modes).

## Non-Goals

A separate web-specific backend or API surface — the Web Shell is a client of the exact
same `/api/rpc` and `/ws` endpoints the desktop and mobile shells use.

## Architecture

See `004-System-Architecture.md`. The Web Shell's distinguishing characteristic is simply
that it's reached over a network rather than loopback-only by default, which is what
makes `access::AccessPolicy` (`031-Security.md`) load-bearing for this shell specifically.

## Data Structures

Same shared types as every shell (`src/lib/api.ts`).

## Traits / Interfaces

No Web-Shell-specific RPC methods — everything it calls is shared with the desktop shell.

## Storage Layout

N/A — stateless client of Core's storage.

## Performance Targets

Same cold-start/idle-memory budgets as Core itself (`004-System-Architecture.md`); the
Web Shell's own bundle size is managed via Vite's `manualChunks` splitting (monaco,
vendor, xterm as separate chunks — `docs/CHECKPOINT-Phase0-Final.md`'s Condition fix for
the original >500kB single-chunk warning).

## Tradeoffs

CORS defaults to a fixed allow-list (`localhost:1420`, `127.0.0.1:1420`,
`tauri://localhost`, `https://tauri.localhost`) covering the local desktop/web dev loop,
extensible via `--allow-origin` — not `Any`. Trades convenience (any origin "just works"
in dev) for the correctness a real Web Shell deployment needs.

## Failure Modes

**Found and fixed in the Phase 2 re-verification**: the original `AccessControlPanel`
kept "allow remote"/origin toggles as local React state that wrote to nothing and
enforced nothing, while CORS was `allow_origin(Any)` — any web page the user visited
could have driven the RPC surface from their browser. Rewritten to read Core's real
`/health` fields and CORS moved to an explicit allow-list backed by
`access::AccessPolicy`. See `docs/CHECKPOINT-Phase2.md` and ADR 0012 for the full account.

## Security

See ADR 0012 and `031-Security.md` — a non-loopback Core bind requires a bearer token,
enforced at the transport layer, not merely displayed as a setting.

## Testing

Covered by the same E2E suite as the desktop shell (`tests/e2e/`), since the Web Shell
and desktop shell render the same React tree; access-control specifically covered by
`cid-core/tests/api_integration.rs`'s `protected_core_*` tests.

## Implementation Order

Scaffolded in Phase 2, corrected (real access control, real CORS) in the Phase 2
re-verification pass — a concrete example of this project's "audit and fix, don't just
build" discipline (`003-Product-Philosophy.md`).

## Acceptance Criteria

A Core bound to a non-loopback address without a token refuses to start
(`remote_bind_without_a_token_is_refused_at_startup`); one bound with a token rejects
unauthenticated requests (`protected_core_rejects_rpc_without_a_token`).

## AI Coding Rules

Never revert CORS to `Any` for convenience — this was a real, found security gap in this
project's own history, not a hypothetical one.
