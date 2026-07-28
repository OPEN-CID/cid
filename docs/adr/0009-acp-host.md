# ADR 0009: ACP Host – External Editor Handoff (Phase 1)

- **Date**: 2026-07-26
- **Status**: Accepted
- **Context**: 
  - CID needs to interoperate with best-in-class editors instead of out-building them (ADR 0006). 
  - **Agent Client Protocol (ACP)** created by Zed Industries Aug 2025, co-developed with JetBrains Oct 2025, Apache-licensed, JSON-RPC 2.0 over stdio, adopted by 25+ agents and 10+ editor surfaces, is the standard for agent↔editor-surface integration (analogous to LSP for language servers). 
  - Distinct from MCP (tool/data), A2A (agent-to-agent), and Stripe/OpenAI Agentic Commerce Protocol. Internally we spell out `AgentClientProtocol` in code comments to avoid collision.
  - CID should be an **ACP host (Agent Client Protocol)**: a Mission's worktree/agent session can be handed off to external ACP-compatible editor (Zed, JetBrains IDEs) and returned. Non-ACP editors (VSCode, Cursor) should also be supported via folder open for UX completeness.
  - Requirements per Part 11 (editor strategy) and Part 15 (cross-platform host, JSON-RPC over WebSocket): CID Core exposes local API, spawns external editor with worktree path, tracks handoff lifecycle HandedOff -> InExternalEditor -> Returned/Failed without blocking.

- **Decision**:
  - Created module `cid-core/src/acp/mod.rs`:
    - Struct `AcpHostManager` with `Arc<RwLock<HashMap<String, AcpHandoff>>>` for thread-safe tracking (std::sync::RwLock chosen for sync `list_handoffs()` + async `handoff()` ergonomics).
    - Types reused from `api/types.rs`: `AcpEditor`, `AcpEditorType`, `AcpHandoff`, `AcpHandoffStatus`.
    - **Editor detection** (`list_editors()`):
      - Probes PATH via split_paths + Windows PATHEXT handling (.exe, .cmd, .bat) – implements which/where logic manually to avoid shell dependency.
      - Checks common install locations per OS:
        - Zed: `zed` in PATH, `C:\Program Files\Zed\zed.exe`, `%LOCALAPPDATA%\Zed\...`, `/Applications/Zed.app/Contents/MacOS/zed`, `~/.local/bin/zed`, `/usr/local/bin/zed`, etc.
        - JetBrains: `idea`, `pycharm`, `webstorm` in PATH or Toolbox scripts `%LOCALAPPDATA%\JetBrains\Toolbox\scripts\*.cmd`, `~/.local/share/JetBrains/Toolbox/scripts/*`, `/Applications/IntelliJ IDEA.app/...`
        - VSCode: `code`, `C:\Program Files\Microsoft VS Code\...`, `/Applications/Visual Studio Code.app/...`
        - Cursor: `cursor`, `%LOCALAPPDATA%\Programs\cursor\...`, `/Applications/Cursor.app/...`
      - For each candidate, checks file existence, marks `available`, attempts `--version` with 2s timeout via `std::sync::mpsc` + spawned thread (avoids hanging on GUI apps). Trims to first line, 120 chars.
      - `supports_acp` true for Zed (AcpEditorType::Zed) and JetBrains (co-developed ACP), false for VSCode/Cursor but still allow handoff.
    - **Handoff** (`handoff(mission_id, editor_id, worktree_path) -> AcpHandoff`):
      - Validates inputs, checks worktree path existence (warn if missing, still allows for testing).
      - Finds editor by id from detection list, bails if not available.
      - Creates `AcpHandoff` with uuid v4, status `HandedOff`, then spawns editor via `tokio::process::Command` (non-blocking per requirement):
        - Handles platform quirks: `.app` bundle via `open -a <app> <path>` on macOS, `.cmd/.bat` via `cmd /C` on Windows.
        - Stdio nulled to detach, drop Child handle (does not kill on drop).
      - On success sets status `InExternalEditor`, stores in map, returns. On failure stores `Failed` and returns error.
      - Future improvement: background task waiting for child exit to auto-mark Returned – intentionally not done for Phase 1 because `open -a` and `code` spawn wrappers that exit immediately.
    - **Take back** (`take_back(handoff_id) -> AcpHandoff`):
      - Sets status `Returned`, `returned_at = Utc::now()`, idempotent. Does not forcibly kill external editor (Phase 1 safety).
    - **List** (`list_handoffs()` and `list_handoffs_for_mission()`, `get_handoff()`, `remove_handoff()`):
      - Clones values under RwLock read.

  - Integrated into `cid-core/src/lib.rs`:
    - `pub mod acp;`
    - `Core` now has `acp_manager: Arc<AcpHostManager>` and `app_state()` propagates to `AppState`.
  - `api/router.rs` `AppState` extended with `acp_manager` (and also `github_manager`, `context_engine_manager` from parallel Phase 1 work).
  - Added stub modules for `autonomy` and `skills` (empty dirs previously caused compile failure) to keep crate compiling.

- **Alternatives considered**:
  - Use `which` crate for PATH probing – adds dependency, less control over Windows PATHEXT, custom impl keeps zero extra deps and matches spec "via which/where".
  - Use `tokio::sync::RwLock` for handoffs map – would require async `list_handoffs()`; `std::sync::RwLock` allows sync API as spec suggests `list_handoffs() -> Vec<...>` without async, still safe because lock not held across await.
  - Store child `tokio::process::Child` in map and auto-wait – would allow auto-return detection but breaks for `open -a` and VSCode's launcher wrapper that exits immediately; deferred to Phase 2 with editor-specific wait strategies.
  - Implement full ACP JSON-RPC host protocol (stdio transport) in this phase – Phase 1 scope per task is only spawning + tracking; full ACP session relay (JSON-RPC over stdio between CID and external editor) is Phase 2, after Core has stable mission context to expose via ACP.
  - Attempt to kill external editor on `take_back` – rejected for safety (user may still be editing); flag for optional kill can be added later.

- **Consequences**:
  - CID can now detect Zed, JetBrains IDEs, VSCode, Cursor across Windows/macOS/Linux via PATH + common locations.
  - Mission worktree can be handed off to external editor with non-blocking spawn, lifecycle tracked in `Arc<RwLock<HashMap>>`.
  - `supports_acp` correctly true for Zed and JetBrains (co-developers of ACP), false for VSCode/Cursor (still usable via folder open).
  - Module compiles (`cargo check -p cid-core` warnings only, no errors) and is usable from `lib.rs` (`Core::new_in_memory().acp_manager.list_editors()` etc).
  - Tests: 5 acp-specific tests + 4 existing pass (9 total for `cargo test -p cid-core acp`).
  - Future work:
    - RPC endpoints `acp.editors.list`, `acp.handoff`, `acp.take_back`, `acp.handoffs.list` in `api/router.rs` (currently manager is in AppState but not yet exposed via JSON-RPC – easy addition).
    - Full ACP JSON-RPC host implementation: proxy Mission messages to external editor's ACP client, handle `fs_read`, `terminal`, etc.
    - Optional child process tracking with platform-specific wait (e.g., `--wait` flag for VSCode: `code --wait <path>` blocks until window closed, enabling auto-return).
    - Version detection for JetBrains Toolbox wrappers (`idea --version` opens IDE; need to parse `product-info.json` from install dir instead).

- **References**: 
  - Part 11 (editor strategy: Monaco/CodeMirror inline + ACP host for pop-out)
  - Part 15 (cross-platform host, ACP JSON-RPC over stdio, 25+ agents, 10+ surfaces)
  - Zed 1.0 Apr 29 2026, ACP created Aug 2025, JetBrains co-dev Oct 2025, ACP Registry Jan 28 2026
  - Warp, Herdr, Cursor, Devin Desktop precedents for worktree-per-agent + external editor pop-out
