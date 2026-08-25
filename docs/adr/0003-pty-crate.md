# ADR 0003: portable-pty for native PTY

- **Date**: 2026-07-26
- **Status**: Accepted
- **Context**: Need real PTY per Session, native cross-platform (not xterm.js-only client-side emulation). Build Prompt Part B suggests `portable-pty` or equivalent with Windows ConPTY and Unix support. Warp, Herdr, cmux all use native PTY with real process behind it.
- **Decision**: Use `portable-pty` 0.8 crate. On Windows it uses ConPTY, on Unix it uses forkpty. Reader thread spawns per PTY that reads from master and broadcasts via `broadcast::channel`. Frontend renders only via `xterm.js`, with `FitAddon` for resize. Resize handled via `pty.resize` RPC.
- **Alternatives**:
  - `pty-process` or manual `unsafe` PTY handling: more control but more platform-specific code to maintain
  - `tokio-pty-process`: tokio-centric but less Windows-tested than portable-pty
  - `winpty` (deprecated): older Windows PTY, superseded by ConPTY
- **Consequences**:
  - `create_pty` takes workdir from session's worktree_path or repo path — terminals start in correct repo context
  - Output streaming via WS notification `pty.output` — simple but may need backpressure handling for high throughput Phase2+
  - `subscribe_output` spawns thread per subscriber — for Phase0 single client per PTY it's fine; Phase1 with multi-surface may need shared broadcast receiver instead of thread-per-subscriber
  - On Windows, shell is `%COMSPEC%` (cmd.exe) by default; on Unix `$SHELL` or `/bin/bash -l`. Could allow user-configurable shell in settings Phase1+
  - Security: no sandboxing Phase0; Phase2+ should run Autonomous-mode commands inside restricted job object / sandbox-exec / namespaced process scoped to worktree dir per Part 14
- **References**: Build Prompt Parts 9, 13, 18; Warp's PTY-based arch validation.
