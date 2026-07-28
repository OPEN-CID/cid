# 027 — Mobile

## Vision

Approval and monitoring from a phone, not a full editing surface — per Part 1's explicit
mobile non-goal: reviewing diffs, approving plan/tool-call requests, checking Mission
status, voice input, push-style notifications, a read-only terminal.

## Goals

`src/mobile/MobileApp.tsx` — Mission list (blocked-on-approval Missions surface first) →
tap into a Mission → approve/deny/comment on pending tool calls → read-only diff and
terminal tabs. Built on the Phase 2 bake-off decision (ADR 0010: Tauri v2 Mobile), so it
shares the exact React bundle and JSON-RPC contract every other shell uses — no separate
mobile backend.

- **Notifications**: Web Notification API, requested once, one push per pending
  approval, deduplicated by `tool_call_id`.
- **Voice input**: Web Speech API where the platform provides it —
  `useVoiceInput` reports `supported: false` rather than rendering a dead button when it
  doesn't (a deliberate honesty choice: a visible-but-broken control is worse than an
  absent one).
- **Shell selection**: `src/main.tsx`'s `isMobileShell()` — Tauri Android/iOS platform, or
  a narrow touch-only viewport, or an explicit `?mobile=1` override for testing. Chosen by
  platform/input, not window width alone, so a narrow desktop window still gets the full
  app.

## Non-Goals

File tree, full code editor, or code-writing from mobile — deliberately absent, per
Part 1's mobile non-goal and Part 19's mobile screen spec.

## Architecture

Same architecture as every shell (`004-System-Architecture.md`) — `MobileApp` is a
different React entry point over the identical `src/lib/api.ts` client.

## Data Structures

`Mission`, `Message`, `PendingApproval` (mobile-local TypeScript types in
`MobileApp.tsx`, mirroring the shared backend shapes).

## Traits / Interfaces

No mobile-specific RPC methods — `mission.list`, `message.list`,
`mission.approveTool`, `mission.sendMessage`, `git.diff` are all shared.

## Storage Layout

N/A — stateless client.

## Performance Targets

Not independently benchmarked; bounded by the same Core-side budgets as any other shell.

## Tradeoffs

Approval push notifications use the in-browser/in-webview Notification API, not a real
push service (APNs/FCM) requiring a backend relay — sufficient for "the app is open or
recently backgrounded," insufficient for "notify me while the app is fully closed." Named
as a real, deliberate scope limit (`docs/CHECKPOINT-Phase3.md`), not an oversight.

## Failure Modes

**Not verified on real iOS/Android hardware or the Tauri mobile runtime** — tested via
web-build responsive/touch-emulation and the `?mobile=1` override only. Named explicitly
in `docs/CHECKPOINT-Phase3.md` as the one place this project's "verify, don't just build"
discipline could not be fully applied in this environment, since no physical device or
mobile build toolchain was available. Push notifications and voice input specifically are
the areas most likely to behave differently on real hardware, per Part C of the Phase 3
build prompt's own warning.

The Phase 5 dependency audit (`045-Dependency-Audit.md`) found Tauri v2's own mobile
maturity has improved since ADR 0010's bake-off characterization — now described upstream
as shipping "first-class iOS and Android support" on an actively-patched 2.11.x line, with
a stable API for both platforms (some desktop plugins still not yet available on mobile).
This strengthens confidence in ADR 0010's choice; it does not substitute for the
still-missing real-device verification pass above.

## Security

Same access-control boundary as the Web Shell (`026-Web-Architecture.md`) — the mobile
shell is a network client like any other.

## Testing

No dedicated mobile E2E suite exists; covered indirectly by the shared component tests
where `MobileApp` reuses logic, and manually verified via the web-build emulation path
described above.

## Implementation Order

Bake-off decision (Phase 2, ADR 0010) → mobile companion app built (Phase 3).

## Acceptance Criteria

A Mission blocked on approval is visible and actionable from the mobile shell — verified
via the web-build path; not yet verified on a physical device.

## AI Coding Rules

Do not claim mobile is "fully verified" in any future checkpoint until a real device pass
happens — this document's own honesty about that gap should be preserved, not quietly
dropped in a later revision.
