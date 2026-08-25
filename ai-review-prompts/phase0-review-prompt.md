# Phase 0 review — golden path, desktop shell, Core basics

Source of truth for what Phase 0 was supposed to be: `docs/CHECKPOINT-Phase0.md` and
`docs/CHECKPOINT-Phase0-Final.md` (the second one records fixes applied after the first
checkpoint — read both, in order).

## Claims to verify

1. **Core is a real Rust/Tokio daemon exposing JSON-RPC 2.0 over HTTP + WebSocket.**
   Check: `cid-core/src/main.rs` (binary entrypoint), `cid-core/src/api/router.rs`
   (`create_router`, routes `/health`, `/api/rpc`, `/ws`, and — added later, Phase 6 —
   `/metrics`). Run `cargo run -p cid-core -- --port 5919` and `curl
   http://127.0.0.1:5919/health`.
2. **Sessions run in an isolated git worktree by default, or a shared clone.** Check:
   `cid-core/src/api/types.rs`'s `IsolationMode` enum, `cid-core/src/git/mod.rs`'s worktree
   functions, `cid-core/tests/worktree_property.rs` (11 property tests).
3. **A real native PTY per Session, not xterm.js-only emulation.** Check:
   `cid-core/src/pty/mod.rs` (`portable-pty`-backed), `pty.create`/`pty.write`/
   `pty.resize`/`pty.kill`/`pty.list` RPC methods in `router.rs`.
4. **Per-hunk diff accept/reject, not just whole-file.** Check: `git.hunk.apply` RPC
   (`handle_git_hunk_apply` in `router.rs`), `src/components/diff/DiffViewer.tsx`'s
   `handleHunkAction`. Known, documented limitation: hunk *reject* currently does a
   whole-file `git checkout HEAD --`, not a true per-hunk reverse patch — check the
   comment in `DiffViewer.tsx` near line 184 still says this, and decide whether it's
   still accurate.
5. **A basic MCP client** (add server via UI, stdio/HTTP transports, tool calls render as
   inline cards). Check: `cid-core/src/mcp/mod.rs`, `mcp.server.*`/`mcp.tools.list`/
   `mcp.tool.call` RPC methods, `src/components/mcp/McpPanel.tsx`.
6. **`AGENTS.md` auto-detection on repo connect.** Check: `cid-core/src/context/mod.rs`'s
   `detect_agents_md`, called from `handle_repo_connect` in `router.rs`.
7. **SQLite persistence for sessions/messages/settings**, now using WAL journal mode
   (added during Release validation — see Failure Modes below). Check:
   `cid-core/src/persistence/mod.rs`'s `init_schema` and the `pragma_update` calls in
   `Persistence::new`.
8. **Tauri v2 desktop shell compiles.** Check: `src-tauri/Cargo.toml`, `cargo check -p
   cid`. Note per `docs/048-Platform-Verification.md`: this has **not** been verified via
   a real `tauri build` + click-through pass in this release cycle — only `cargo check`.
   Confirm that gap is still real, or whether it's been closed since this was written.

## Known-fixed issues from the original Phase 0 checkpoint (verify they're still fixed, not regressed)

Per `docs/CHECKPOINT-Phase0-Final.md`: MSVC toolchain requirement documented, WS response
broadcast fixed to per-client, PTY thread-per-subscriber leak fixed (`get_receiver`
pattern in `pty/mod.rs`, with the old leaking method kept only as `#[deprecated]`),
secrets moved off plaintext SQLite storage, secret redaction added
(`cid-core/src/redact/mod.rs`), CORS tightened from `Any` to an explicit allow-list
(`create_router`'s `CorsLayer`), Tauri icons present, per-hunk accept/reject wired.
Each of these was a *found bug*, not a stub — check it's genuinely still fixed, since
regressions are exactly what a large, many-phase codebase is prone to.

## Failure modes already found and fixed in later passes (context, not new findings)

A SQLite journal-mode crash-resistance gap was found and fixed during Release
validation (not Phase 0 itself): the default rollback-journal mode left the database in
an inconsistent state after a process was killed abruptly and immediately restarted
against the same file, causing a `FOREIGN KEY constraint failed` error on the next write
even though the referenced row existed and was visible to reads. Fixed by switching to
WAL mode. See `cid-core/src/persistence/mod.rs::file_backed_databases_use_wal_journal_mode`
for the regression test. Verify this test exists and passes.
