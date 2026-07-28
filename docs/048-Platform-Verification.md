# 048 — Platform Verification

## Vision

One place that states, per surface, what "verified" actually means in this codebase —
real automated test coverage vs. real-hardware pass vs. not yet exercised at all —
so nobody mistakes "the code exists" for "it's been run there." Written during Release
validation because scattering this across six checkpoint files made it hard to answer
"is X actually checked?" in one look.

## Goals

| Surface | What exists | How it's verified | Real gaps |
|---|---|---|---|
| **Core (`cid-core`)** | The Rust/Tokio daemon — every subsystem, the JSON-RPC API. | 317 unit tests, 62 integration tests (real HTTP against a spawned Core), 9 protocol-fuzz tests, 11 worktree property tests, 5 performance-budget tests. Runs in CI on Linux, Windows, and macOS (`test-rust-linux/windows/macos` jobs) — the only surface with genuine cross-OS CI coverage. | None significant — this is the most-tested part of the system by a wide margin. |
| **Desktop shell (Tauri v2)** | `src-tauri/` wraps the same React bundle in a native webview on macOS/Windows. | CI runs `cargo check -p cid` on Linux/Windows/macOS (compiles, doesn't crash to build) — **not** a real `tauri build` producing an installer, and not a real launch-and-click-through pass. | No CI job actually runs the packaged app. `npm run tauri:dev`/`tauri:build` have been run manually during earlier phases per their checkpoints, not repeated every release. This is the single biggest gap between "compiles" and "verified" in the whole matrix. |
| **Web shell (browser)** | Same React bundle, served by headless Core. | The most-exercised non-Core surface this session: 30 Playwright E2E tests against a real `cid-core --release` and a real Vite dev server (not mocked), covering the Flow 1 golden path, code analysis, MCP, and the full RPC surface via `health-check.spec.ts`. Both new Phase 6 panels (Repository Health, Autonomy) were manually verified with real screenshots against this actual repository. | Real LLM responses were never exercised in this environment — no Anthropic/OpenAI/Google API key was available, so the agent loop's "simulated" fallback path (Core's documented behavior with no configured key) is what every E2E run actually exercises, not a live model call. |
| **Mobile shell** | Approval/monitoring UI (`src/mobile/MobileApp.tsx`), platform-detected. | Exercised as a web build with touch/narrow-viewport emulation (Phase 3). **Never run on real iOS/Android hardware or the Tauri mobile runtime** — stated plainly in `CHECKPOINT-Phase3.md` and still true. Push notifications and voice input are the specific risk areas a real-device pass would need to cover first. | Real-device pass — not done in any phase through this release. |
| **CLI/TUI (`cid-tui`)** | `ratatui`-based terminal client. | 3 unit tests (`api::tests`, `events::tests`), included in every `test-rust-*` CI job since the Phase 5 CI-coverage fix. Not exercised via any scripted "run the binary and interact with it" test — `ratatui` UIs are notoriously hard to test end-to-end without a terminal emulator harness, and none was built. | No end-to-end interaction test; unit tests cover the API client and event-parsing logic only, not the rendered UI itself. Also missing a diff view (see `docs/041-Roadmap.md`'s v1.0 scope table). |
| **Headless Core (CI/remote trigger)** | `cid-core` with no shell, driven by a scripted client. | This is exactly what every integration test and the E2E suite already does — genuinely the best-covered "headless" scenario there is. | None beyond Core's own gaps above. |

## Non-Goals

Re-litigating whether each gap above should be closed before v1.0 — that's the Release
Report's job (disposition: fixed / tracked / accepted). This document only states what's
true today.

## Architecture

N/A — a status matrix, not a design document.

## Tradeoffs

N/A.

## Failure Modes

The single clearest lesson from this whole verification pass, restated one more time
because it's the pattern behind nearly every real bug found across Phases 4–6: **a
compiling, plausible-looking feature is not a verified one.** The Settings panel
(Phase 5), `repo_health.scan`'s string-literal false positive (Phase 6), and the SQLite
journal-mode crash-resistance gap (this Release pass) were all found by actually running
the thing against real data, not by code review or trusting an earlier checkpoint.

## Security

N/A — see `031-Security.md` for the consolidated security posture.

## Testing

This document *is* about testing — see the Goals table.

## Implementation Order

N/A.

## Acceptance Criteria

Every surface in Part 15's architecture diagram (Core, Desktop, Web, Mobile, CLI/TUI,
Headless) has a row above.

## AI Coding Rules

Before claiming a surface "works," check this table first — if its row says "not
verified on real hardware" or "not exercised end-to-end," a claim of full confidence
about that surface is not supported by anything in this repository yet.
