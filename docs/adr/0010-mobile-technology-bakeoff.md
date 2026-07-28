# ADR 0010 — Mobile Shell Technology Bake-off

**Status**: Decided  
**Date**: 2026-07-26  
**Deciders**: CID Architecture  
**Replaces**: None  

## Context

Phase 2 of CID calls for a mobile companion shell. The mobile app is explicitly scoped to approval/monitoring only (per Appendix A Part 1's non-goal: "not full editing"), with these capabilities:

- Review diffs
- Approve/deny plan and tool-call requests
- Check Mission status
- Voice input
- Push notifications
- Read-only terminal

The mobile client must hit the same Core JSON-RPC 2.0 API (over WebSocket or HTTP) that the desktop and web shells use. No separate mobile backend.

Three approaches were evaluated.

---

## Options

### Option A: Tauri v2 Mobile

Build the mobile shell using Tauri v2's mobile targets (iOS/Android), reusing the existing React/TypeScript frontend directly.

**Pros**:
- Maximum code reuse — same React bundle, same API client (`src/lib/api.ts`), same component library (shadcn/ui, Tailwind)
- All Rust Core capabilities available via Tauri IPC without serialization overhead
- Same build pipeline — `npm run tauri:build` with mobile targets
- Full native capabilities: push notifications, biometric auth, background tasks via Tauri plugins
- Single ecosystem to maintain for desktop + mobile
- WebView rendering is production-quality on both iOS (WKWebView) and Android (Chromium WebView)

**Cons**:
- Tauri v2 mobile is explicitly marked as "not yet first-class" by its own team (as of mid-2026)
- Certain mobile-specific UI patterns (pull-to-refresh, swipe gestures, native tab bars) need manual polyfills or native bridging
- WebView app startup time is slower than fully native (~1-3s cold start)
- Larger binary size (includes WebView rendering engine overhead)
- Mobile API surface (camera, GPS, contacts, etc.) requires Tauri plugin maturity that may lag behind React Native
- Debugging across the Rust-WebView boundary on mobile is immature tooling

### Option B: React Native

Build a dedicated React Native client with the same API contract pointing at Core's JSON-RPC 2.0 endpoint over WebSocket.

**Pros**:
- Mature ecosystem with huge plugin/component library (Expo, React Navigation, push notifications)
- Native-feeling UI patterns first-class: swipe, pull-to-refresh, tab bars, safe areas
- Fast development with Expo/Expo Go for rapid iteration
- Smaller app size (no WebView engine overhead, true native compilation)
- Large talent pool and community support
- Native performance for UI rendering (JS → native bridge)
- OTA updates via Expo EAS

**Cons**:
- Second UI codebase to maintain — all components must be rewritten in React Native
- No shared component code with desktop/web React app (different primitives: `<View>` vs `<div>`, native styling vs Tailwind)
- The API client (`src/lib/api.ts`) and types can be shared, but rendering cannot
- Separate build pipeline (`eas build` / `react-native run-ios`) from Tauri
- Two different UI frameworks to keep aligned on features
- State management (zustand) can be shared, but hooks tied to DOM components cannot
- WebSocket handling on mobile (background/foreground lifecycle) is more complex than on desktop

### Option C: Thin Native Client

Build a minimal Swift/Kotlin native client with a simple REST/WS client hitting the Core API, rendering only the Mission list, approval cards, and status — no complex UI framework at all.

**Pros**:
- Smallest binary size, fastest startup, most native feel
- Least dependency overhead
- Best battery life and background behavior

**Cons**:
- Third UI codebase to maintain (Swift + SwiftUI for iOS, Kotlin + Jetpack Compose for Android)
- Maximum fragmentation — different codebases for iOS, Android, macOS desktop, Windows desktop, and web
- Slowest to develop new features — every feature ships 3x (iOS, Android, React web)
- Smallest feature surface realistically maintainable
- Requires mobile-specific expertise on the team for Swift and Kotlin

---

## Comparison Table

| Criterion | Tauri v2 Mobile | React Native | Thin Native |
|---|---|---|---|
| **Code reuse with desktop/web** | Max (same React bundle, all components shared) | Medium (API client + types shared, UI rewritten) | Low (nothing shared except API contract) |
| **Development velocity** | High (one UI codebase) | Medium (two React flavors) | Low (three platforms, three languages) |
| **Native UX quality** | Good (WebView, requires polyfills) | Excellent (native components) | Excellent (true native) |
| **App size** | Larger (WebView + Rust) | Medium (JS engine) | Smallest (native) |
| **Startup time** | ~1-3s (WebView cold) | ~0.5-1s | ~0.3-0.5s |
| **Ecosystem maturity** | Immature (Tauri mobile is beta) | Very mature (Expo, libraries) | Mature but fragmented |
| **Maintenance burden** | Low (one build pipeline) | Medium (two pipelines) | High (N platforms × N pipelines) |
| **Push notifications** | Supported via plugin | First-class in Expo | Native (APNs/FCM directly) |
| **Background sync** | Limited (WebView suspended) | Good (native background tasks) | Best (full native control) |
| **Talent availability** | Niche (Tauri mobile devs rare) | Abundant (React Native devs) | Requires Swift + Kotlin devs |

---

## Decision

**Chosen: Option A — Tauri v2 Mobile**

Rationale:

1. **Code reuse is the dominant factor.** Phase 2 is building a mobile shell — an approval/monitoring companion, not a full IDE. The risk of maintaining two complete UI codebases (React web + React Native) for a monitoring companion outweighs the native UX benefits of React Native. When the mobile surface is approval cards, a chat stream, and a read-only terminal — all widgets the existing React app already renders — rewriting them in React Native is pure duplication.

2. **The API contract is the same.** Both Tauri mobile and the existing desktop shell talk to the same Core JSON-RPC 2.0 API. Tauri mobile can reuse the existing `api.ts` client directly (WebSocket from the WebView to Core) without any adapter layer. React Native would need the same client rewritten for React Native's networking primitives.

3. **Phase 3 can revisit.** If Tauri v2 mobile matures by Phase 3 and meets the needs, great — no change needed. If it doesn't, or if users demand a more native feel, the JSON-RPC 2.0 API contract means switching to React Native or a thin native client is a frontend-only change with zero backend work. The architecture deliberately decouples shell from Core for exactly this reason (Appendix A Part 15).

4. **Real-world precedent.** Several tools in CID's competitive set use WebView-based mobile companions successfully — including Devin Desktop's mobile companion and opencode's web-in-mobile-shell pattern. A WebView shell for approval/monitoring is a proven, not speculative, approach.

5. **Single build pipeline.** `tauri build --target aarch64-apple-ios` and `tauri build --target aarch64-linux-android` extend the existing CI pipeline rather than adding a new one.

---

## Consequences

### Positive
- One UI codebase for desktop, web, and mobile
- Zero migration cost if the mobile scope stays within monitoring/approval
- Shared state management (zustand), shared API client, shared components
- Single CI pipeline with mobile target additions

### Negative
- Mobile UX will feel "web-like" rather than platform-native
- Complex gesture-based interactions may need polyfills
- Tauri v2 mobile plugin ecosystem may have gaps
- Cold start time (1-3s) may feel slow for quick approval checks

### Mitigations
- PWA fallback: the web shell (served by Core in headless mode) can also work as a PWA on mobile as a stopgap
- Monitor Tauri v2 mobile plugin maturity; if critical plugins (push notifications) are unstable, evaluate React Native for those specific integrations while keeping the main UI in Tauri
- Set explicit UX expectations: this is a companion, not the primary development surface

### Future Considerations
- If mobile scope expands beyond monitoring (e.g., full editor, full terminal input), re-evaluate React Native in Phase 4
- The JSON-RPC 2.0 API contract means the mobile shell can be swapped without touching Core
- Consider Capacitor.js as an incremental step from WebView to more native capabilities without a full rewrite

---

## References

- Appendix A Part 15: Cross-Platform Architecture (One Core, Many Surfaces)
- Appendix A Part 2: Competitive Landscape (Devin Desktop mobile, opencode headless server)
- Tauri v2 mobile docs: https://v2.tauri.app/start/mobile/
- Phased Build Plan: Part 22 (Phase 2 mobile bake-off, Phase 3 mobile companion app)