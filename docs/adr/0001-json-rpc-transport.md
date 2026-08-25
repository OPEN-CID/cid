# ADR 0001: JSON-RPC 2.0 over WebSocket as Core IPC

- **Date**: 2026-07-26
- **Status**: Accepted
- **Context**: Need a local IPC between Core (Rust) and all shells (Tauri desktop Phase 0, web Phase 2, mobile Phase 2-3, headless Phase1). Earlier draft left it open as "JSON-RPC or gRPC". Build Prompt Part 15 mandates single consistent protocol style, and competitive landscape shows MCP and ACP both use JSON-RPC 2.0 over stdio/WS.
- **Decision**: Use JSON-RPC 2.0 over WebSocket (with HTTP POST fallback at `/api/rpc`) for all Core↔Shell communication. WS allows server→client streaming notifications (session.message.delta, pty.output, git.diff.update) without polling. Axum 0.7 with WS feature for server, native WebSocket API on frontend.
- **Alternatives**:
  - gRPC: high performance but heavier setup across languages, mismatched with MCP/ACP already using JSON-RPC
  - Tauri invoke only: ties Core to Tauri, prevents browser dev loop and future web/headless shells
  - REST only: no streaming, need SSE or polling for deltas
- **Consequences**:
  - Frontend can run in plain browser via `npm run dev` talking to standalone Core — faster iteration than rebuilding Tauri binary per change, as Build Prompt Part C requires
  - One wire format to maintain, matches MCP 2026-07-28 and ACP (Agent Client Protocol)
  - WS broadcast approach in router.rs is simple but broadcasts responses to all clients; Phase1 should add proper per-client sink handling with Arc<Mutex<SplitSink>>
  - HTTP fallback at `/api/rpc` allows curl debugging
- **References**: Build Prompt Parts 15, 18; competitive table: opencode serve uses OpenAPI+SDK, but Junie/Cursor agents already use WS for streaming.
