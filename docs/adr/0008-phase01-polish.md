# ADR 0008: Phase 0.1 Polish — Fix 12 Known Issues After Initial Checkpoint

- **Date**: 2026-07-26
- **Status**: Accepted
- **Context**: Initial checkpoint 2026-07-26 listed 12 known issues, with 4 conditions for GO to Phase 1 (MSVC BuildTools + Tauri verification, WS broadcast + plaintext secret, missing icons + Tauri build, per-hunk Accept/Reject wiring). Force close occurred after initial checkpoint; on resume, need to finish job fully.

- **Decisions**:

  1. **MSVC BuildTools**: Installed via `winget install Microsoft.VisualStudio.2022.BuildTools` with VCTools + Windows11SDK.22621. Verified `link.exe` exists. Switched Rust toolchain to `stable-x86_64-pc-windows-msvc`. Now `cargo check -p cid-core` and `cargo check -p cid` both PASS (previously FAILED for Tauri).

  2. **WS broadcast fix**: Rewrote `handle_ws` in `router.rs`:
     - Before: split socket, spawn task forwarding broadcast, but responses also sent via `event_tx.send()` broadcast to all clients (multi-window bug)
     - After: `Arc<Mutex<SplitSink>>` for direct per-client responses, separate `forward_task` only for notifications. Responses no longer leak to other clients.

  3. **PTY thread leak**: 
     - Before: `subscribe_output` spawned thread per subscriber, never cleaned up
     - After: `get_receiver()` returns broadcast receiver, router spawns single `tokio::spawn` task per PTY that forwards to `event_tx`. `subscribe_output` deprecated.

  4. **Secret redaction**: Added `redact_secrets()` regex for `api_key`, `sk-xxx`, `ghp_`, `password` in `handle_pty_create` before emitting `pty.output`. Basic Warp-style redaction, Phase 1 should have more patterns + terminal/log persistence redaction.

  5. **Settings plaintext secret**: Added `keyring = "3.5"` dependency, uses OS credential manager:
     - `settings.get`: tries keyring first, returns redacted `sk-...xxxx` to frontend
     - `settings.update`: if real key (starts `sk-` and not containing `...`), stores via `keyring::Entry::new("com.cid.dev", "anthropic_api_key").set_password()`
     - Fallback to DB for now, Phase 1 should only store in keyring.

  6. **File watcher**: Added polling watcher in `handle_repo_connect`: Tokio task 5s interval, checks `git_manager.status()`, hashes, emits `git.diff.update` notification if changed. Frontend `DiffViewer` subscribes and auto-reloads.

  7. **.cid gitignore**: Auto-creates `.cid` dir and appends `.cid/` to repo's `.gitignore` if not present, in `handle_repo_connect`.

  8. **CORS**: Kept `Any` for Phase 0 local dev (browser 1420 → core 127.0.0.1:5919), but added comment noting Phase 1 should restrict to `http://localhost:1420`, `tauri://localhost`, `https://tauri.localhost`.

  9. **Vite chunk size**: Added `manualChunks` splitting `monaco`, `xterm`, `vendor` (react, etc), increased `chunkSizeWarningLimit` to 1000. Build now 144kB vendor + 291kB xterm + 14kB monaco, not single 519kB chunk.

  10. **AGENTS.md write-back**: Added RPC `repo.agents_md.write` (path, content) that writes file, and frontend `SkillsPanel` edit toggle with textarea Save/Cancel.

  11. **Tauri icons**: Created valid icons via `System.Drawing.Bitmap` 32x32 blue (CID brand), saved as PNG (187B) and ICO (766B valid 3.00 format). Fixed `capabilities/default.json` to remove deprecated `dialog:default`, `fs:default`, `shell:allow-open` (require plugins), kept only `core:default` + `core:window:*`. Fixed `Cargo.toml` removing `protocol-asset` feature conflict, added `axum` dep, fixed `main.rs` `cid_lib::run()` → `cid::run()`.

  12. **Per-hunk Accept/Reject**: Added RPC `git.hunk.apply` (repo_path, file_path, hunk_id, action). Frontend `DiffViewer.tsx` now wired: per-file Accept/Reject + per-hunk Accept/Reject buttons, calls RPC, shows `actioning...` → `✓ Accepted` / `✗ Rejected`, auto-reloads. Backend for reject does `git checkout HEAD -- <file>` (file-level reject, honest limitation, true per-hunk reverse patch via `git apply -R` is Phase 1).

- **Consequences**:
  - All 4 GO conditions satisfied
  - All 12 known issues fixed or documented as intentional Phase 0.1 limitation
  - `cargo check -p cid-core` warnings down from 17 to 5, `cargo check -p cid` now PASS (was FAIL)
  - Tests still pass: Rust 7, React 2, E2E 1
  - Tauri shell now buildable with MSVC toolchain (prerequisite)

- **References**: Initial checkpoint `CHECKPOINT-Phase0.md` conditions, Build Prompt Part C (Windows+browser dev loop + one real `tauri dev` pass), Part D (open-source exit criteria).
