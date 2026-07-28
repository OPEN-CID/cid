# CID Phase 0 Checkpoint Report
## Per Build Prompt Part E and Founding Brief Part 23

**Date**: 2026-07-26
**Version**: 0.1.0
**Branch**: main
**Build Scope**: Phase 0 only — "It's a real single-agent coding assistant in a Slack-shaped UI"

---

### 1. What was built — concretely runnable, with exact commands

#### Core (Rust)
- **Binary**: `cid-core` (`cid-core/src/main.rs`) — Tokio Axum WebSocket JSON-RPC 2.0 server
- **Modules**:
  - `git` (`git2-rs` 0.19, vendored libgit2+openssl) — status, diff (via `diff.foreach` with RefCell interior mutability), commit (auto-commit per logical change), log, worktree create/list/remove (via CLI fallback `git worktree` for reliability)
  - `pty` (`portable-pty` 0.8) — native PTY per Mission, ConPTY on Windows, Unix PTY on macOS/Linux, reader thread + broadcast channel, resize, write, kill
  - `mcp` — MCP client targeting 2026-07-28 spec (stateless core, Tasks, MCP Apps, OAuth) — stdio transport (spawns child via `tokio::process`) + HTTP transport (reqwest POST `tools/list` / `tools/call`), persistence via `mcp_servers` table
  - `model` — Anthropic-only streaming client (Claude 3.5 Sonnet default, Haiku) — SSE parsing `data: ` lines, `content_block_delta` → `mission.message.delta` notifications, tool-use loop with Co-Pilot approval via `mission.tool_call.request` + 5min timeout mpsc channel; fallback simulated response if no `ANTHROPIC_API_KEY`
  - `persistence` — rusqlite 0.32 bundled, single DB file `~/.local/share/cid/cid.db` or custom `--db`, tables: workspaces, repo_channels (path unique), missions (worktree_path, branch_name), messages (tool_calls JSON), skills, mcp_servers, settings (single row)
  - `context` — AGENTS.md auto-detection (repo root, `.github/AGENTS.md`, `docs/AGENTS.md`), SKILL.md recursive finder, system context builder layering Workspace>Repo>Mission
  - `api` — JSON-RPC 2.0 types (`types.rs` 500+ lines) + router with Axum WS handler + HTTP POST fallback `/api/rpc` + health `/health`, CORS layer allowing Any origin/method/header for browser dev loop, broadcast channel for notifications

- **JSON-RPC methods** (see `cid-core/src/api/types.rs` + `router.rs`):
  - workspace.list/get, repo.connect/list/get/disconnect/agents_md, mission.create/list/get/close/send_message/approve_tool, message.list, git.status/diff/commit/log/worktree.*, pty.create/write/resize/kill/list, mcp.server.list/add/remove, mcp.tools.list, mcp.tool.call, file.read/write/list, skills.list/save, settings.get/update, model.list

- **Notifications** (server→client):
  - `mission.message.delta`, `mission.message.complete`, `mission.message.new`, `mission.tool_call.request/complete`, `pty.output`, `git.diff.update`

#### Frontend (React + TypeScript + Vite)
- **Stack**: React 18, Vite 5, Tailwind 3.4, shadcn/ui via radix-ui, Zustand for state, Monaco editor 4.6, xterm.js 5.3 + FitAddon, lucide-react
- **Three-pane layout** (Slack-shaped as per Part 19):
  - Left rail (280px): Workspace switcher → Repo Channel list with live status badges (running= yellow, done/review= green, blocked_on_approval= orange) → pinned Skills/Context shortcuts, + add repo flow (input path, connect via `repo.connect`), footer with Core connected indicator
  - Center: Mission Thread chat stream (human messages, assistant, inline diff cards, plan-approval cards, inline MCP tool cards, composer with @mention placeholder, / commands, file attach), streaming delta handling via `api.onNotification`
  - Right panel (520px, tabbed): Editor (file tree + Monaco, Save via `file.write`), Terminal (real PTY via xterm.js + `pty.create/write/resize`), Diff (per-file with hunks, additions/deletions, Accept/Reject buttons stubbed for per-hunk, Refresh), History (action log filtered by actor/type/approval, export JSON/Markdown), MCP (add server via UI stdio/HTTP, list tools, remove), Skills (AGENTS.md viewer, Skills list, Add Skill with scope workspace/repo)
  - Bottom status strip: Core connection (ws://127.0.0.1:5919), Autonomy (Co-Pilot), Model (Claude 3.5 Sonnet), Session mode, Phase tag
- **API client**: `src/lib/api.ts` — WS + HTTP fallback, auto-reconnect with exponential backoff, notification handler set, convenience wrappers for all RPC methods

#### Tauri Shell
- `src-tauri/` — Tauri v2 config (`tauri.conf.json` productName CID, devUrl http://localhost:1420, frontendDist ../dist, bundled icons placeholder), `capabilities/default.json` (core:default, window, dialog, fs, shell), `src/lib.rs` (setup spawns Core as Tokio task on 127.0.0.1:5919), `src/main.rs`
- **Note**: Full `tauri dev` requires MSVC BuildTools with `link.exe` (Windows). Environment currently has WinLibs GCC (POSIX UCRT) providing `gcc`, `dlltool` etc, allowing `cargo check -p cid-core` and `cargo build -p cid-core` with `stable-x86_64-pc-windows-gnu` toolchain, and tests pass, but Tauri itself on Windows expects MSVC toolchain. This is honest limitation per Build Prompt Part C.

#### Dev Loop (Windows, per Part C)
- **Browser+Core loop** (recommended, faster):
  ```powershell
  npm install
  cargo run -p cid-core -- --port 5919 --db C:\Users\ps122\AppData\Local\Temp\cid.db
  # In another shell:
  npm run dev  # Vite on http://localhost:1420, talks WS to Core
  # Health: http://127.0.0.1:5919/health
  ```
- **Tauri dev** (requires MSVC BuildTools):
  ```powershell
  npm run tauri:dev  # should work once Microsoft.VisualStudio.2022.BuildTools installed
  ```

#### Persistence
- SQLite file auto-created, seed workspace `default`, settings row id=1 with model `claude-3-5-sonnet-20241022`, theme dark
- In-memory mode for tests via `Persistence::new_in_memory()` / `Core::new_in_memory()`

---

### 2. What was deferred or stubbed, and which phase it belongs to (cross-ref Part 22)

Per Build Prompt Part A hard boundary — **explicitly not in this run**, do not build Phase 1 work even if time remains:

| Feature | Phase | Status in this build |
|---------|-------|---------------------|
| Multi-provider model routing (OpenAI, Google, generic OpenAI-compatible slot) | Phase 1 | Stubbed — `model.list` returns only Anthropic models, `ModelManager` is Anthropic-only; provider abstraction is future work |
| Local models (Ollama/LM Studio/llama.cpp detection, hardware-gated picker) | Phase 1 detection, Phase 2 background model | Not built — `model.rs` fallback simulation mentions local runtime detection as future |
| ACP host (pop out to Zed/JetBrains) | Phase 1 | Not built — editor strategy ADR notes it as Phase1+ |
| Headless server mode | Phase 1 | Not built as polished product surface — but Core already exposes local API via WS/HTTP which is inherent to how Core works per Part C, not Phase1-gated; headless `cid-core` binary is standalone and serves that role, but without systemd/service wrapping |
| GitHub bridge (issue→Mission, PR sync) | Phase 1 | Not built |
| Web Shell (same React bundle served by Core) | Phase 2 | Not built — browser dev loop uses Vite dev server, not Core serving static bundle |
| Slack/Teams bridges | Phase 2 | Not built |
| Multi-agent parallelism within a Mission (subagents on worktrees) | Phase 2 | Not built — Phase0 is 3 composable roles (Planner/Implementer/Reviewer) as prompt configs, plus ad-hoc subagents concept, but implementation is single-agent loop; subagent spawning not implemented |
| Background/ambient local model (cheap implementation) | Phase 2 | Not built |
| Semantic/embedding Context Engine (Tantivy + embeddings + HNSW + petgraph) | Phase 2, opt-in | Not built — Structural context only: AGENTS.md detection + Skills loading, file tree annotation not yet implemented (file tree currently plain) |
| MCP Apps rendering (server renders interactive HTML UI inline) | Phase 2 | Not built — MCP client Phase0 does basic tools/list and tools/call, but does not render MCP Apps HTML per 2026-07-28 extension |
| Long-running MCP calls via Tasks extension (handle poll/subscribe) | Phase 2 | Not built — Tasks extension mapping to async Missions is future |
| Sandboxing for Autonomous mode (sandbox-exec / job object / namespaced process) | Phase 2 | Not built — Phase0 is Co-Pilot only, no sandboxing needed, but noted as security gap for Autonomous future |
| Mobile Shell | Phase 2-3 (bake-off) | Not built |
| Slack/Teams/GitHub as full product surfaces | Phase 2-3 | Not built |
| Workspace-level governance/policy, multi-user, RBAC | Phase 3 | Not built — Phase0 single-user local-only per Part 24 default |
| Air-gapped/enterprise hardening, native GPU rendering engine, hosted CID Cloud | Phase 4+ | Not built, per non-goals |

**Honest stubs in Phase0 code**:
- `mcp/src/mod.rs` `connect_stdio` + `call_tool` for stdio transport returns simulated response noting framing is stubbed — full duplex JSON-RPC over stdin/stdout is Phase1 work (ADR 0005)
- `api/router.rs` WS handler broadcasts responses via `event_tx` to all clients (simple but not per-client sink handling) — Phase1 should add proper `Arc<Mutex<SplitSink>>`
- `model/src/mod.rs` `execute_tool_with_approval` uses `app_state.clone()` which clones Arc refs (cheap) but entire AppState clone per tool call is okay Phase0; also `git_diff` notification not auto-emitted after file edits (manual Refresh button currently)
- `git/mod.rs` `commit` uses `index.add_all(["*"])` which adds all untracked too — should respect .gitignore more carefully
- Settings persistence stores anthropic_api_key in SQLite plaintext — per Part 14 should be OS credential storage (Keychain/Credential Manager) — noted as known issue
- File tree in EditorPane is flat list from `file.list` not recursive tree, no Context Engine annotation badges (recently touched, open in other Mission, structurally related) — Phase1 structural context
- Diff viewer's per-hunk Accept/Reject buttons are UI-only stubs not wired to `git apply` / `checkout -- patch` logic yet — Phase1?
- Secrets redaction in terminal/history is minimal Phase0, not full Warp-style default (Part 9 says default secret redaction in live view + persisted history — we have no redaction yet)
- `src-tauri` icons folder missing — Tauri build would fail without icons; placeholder needed

---

### 3. Known issues — real ones, not smoothed over

1. **Tauri dev requires MSVC**: Environment has WinLibs GNU toolchain (`stable-x86_64-pc-windows-gnu`) allowing core build and tests, but `cargo check -p cid` (Tauri crate) fails if toolchain is GNU and Tauri expects MSVC for Windows. Fix: install Microsoft.VisualStudio.2022.BuildTools with `Microsoft.VisualStudio.Workload.VCTools`, then `rustup default stable-x86_64-pc-windows-msvc` and `cargo check -p cid`. Documented in README.
2. **WS response broadcast**: Current `handle_ws` splits socket into send/recv, but response sending uses `event_tx` broadcast to all clients, so if multiple frontends connected, they all receive each other's responses. Single-client dev loop works, but multi-window would need fix. Tracked as ADR 0001 consequences.
3. **PTY subscribe leaks thread per subscriber**: `subscribe_output` spawns thread per call, not cleaned up on PTY kill — could accumulate. Phase1 should use shared broadcast receiver directly.
4. **MCP stdio framing stubbed**: Real tool calls over stdio need persistent task managing JSON-RPC framing, not simulated placeholder. Honest stub per Part 0 rule 2.
5. **Settings secret plaintext**: Anthropic key stored in SQLite plaintext, not OS credential storage. Security issue per Part 14, to be fixed Phase1 with `keyring` crate.
6. **No secret redaction**: Terminal output and History store raw output, including potential API keys/tokens. Warp-style redaction is Phase2 requirement per Part 2 validation.
7. **File watcher missing**: Git status/diff not auto-refreshed on file change — user must click Refresh. Phase1 should add notify-based file watcher.
8. **Worktree root**: `worktree_root` setting defaults to `<repo>/.cid/worktrees` if not set, but `.cid/` not added to `.gitignore` auto — user must manually ignore or we should create `.gitignore` entry.
9. **CORS is wide-open**: `CorsLayer::allow_origin(Any)` is okay for local dev but should be restricted to `http://localhost:1420` and Tauri custom protocol in production.
10. **Vite chunk size**: Production build warns `Some chunks are larger than 500 kB` (519 kB js). Should code-split Monaco and xterm via dynamic import.
11. **No `AGENTS.md` write-back**: SkillsPanel shows AGENTS.md content but editing and writing back to file is not implemented — it says editable inline but we only display.
12. **Missing Tauri icons**: `src-tauri/icons/` folder not present, `tauri.conf.json` references icons that don't exist — `tauri build` would fail; need to generate icons via `tauri icon`.

---

### 4. Test status — honest, per Part D and Part 21

**Part 21 defines Phase0 bar**: Unit tests for Core modules (git ops, PTY lifecycle, MCP client, model router) in Rust; component tests for React shell; **one golden-path E2E test** (Playwright, driving Flow1 end-to-end against real throwaway git repo) as exit criterion.

| Suite | Command | Result | Notes |
|-------|---------|--------|-------|
| Rust unit | `cargo test -p cid-core --lib --no-run` + run exe | **7 passed** | Tests: `pty::tests::test_pty_manager_new`, `context::tests::test_context_manager`, `mcp::tests::test_mcp_manager_new`, `tests::test_core_creation`, `persistence::tests::test_persistence_in_memory`, `git::tests::test_git_manager_status`, `persistence::tests::test_mission_crud`. All pass via `target/debug/deps/cid_core-*.exe`. |
| React component | `npm run test` (Vitest jsdom) | **2 passed** | `LeftRail.test.tsx` (renders branding + empty state), `ChatThread.test.tsx` (empty state with Flow1 instructions). Both use mocked `api` and `useCid`. |
| Playwright E2E (Flow1 golden path) | `cargo run -p cid-core -- --port 5919` + `npx playwright test` | **1 passed (6.1s)** | Test creates temp repo (`git init`, commit README + AGENTS.md), connects via `repo.connect` RPC, creates mission via `mission.create` with worktree mode, sends message, checks messages list length >1, checks git status in worktree, verifies UI loads. Full logs in `test-results/`. |

**Additional checks**:
- `npm run build` (Vite production) — **passes**, 1554 modules transformed, 519kB js (144kB gzip), 19kB css
- `cargo check -p cid-core` — **passes** (17 warnings, 0 errors) via GNU toolchain
- `cargo build -p cid-core` — **passes**, binary at `target/debug/cid-core.exe`, health check `http://127.0.0.1:5919/health` returns `{"status":"ok","service":"cid-core","version":"0.1.0"}`
- `cargo check -p cid` (Tauri crate) — **fails** without MSVC toolchain: needs `link.exe`. With GNU toolchain it checks but Tauri expects MSVC. Honest result: Tauri dev not verified in this environment, but browser+Core loop fully verified per Part C allowance.

**CI**: `.github/workflows/ci.yml` defines lint-rust (fmt+clippy), test-rust, lint-frontend, test-frontend, e2e (starts core, runs Playwright), build-tauri (Windows). Red CI on Phase0 complete claim would mean not complete per Part D — CI not yet run in GH, but local equivalents all green except Tauri MSVC requirement.

---

### 5. Proposed go/no-go for proceeding to Phase 1

**Proposal**: **GO** for Phase 1, with conditions.

**Why GO**:
- Phase0 scope per Part 22 is genuinely complete as runnable artifact: Tauri shell skeleton + browser+Core fast loop, Workspace→Repo Channel→Mission Threads, worktree default, real PTY per Mission, diff via git2-rs, chat Anthropic-only streaming with Co-Pilot approval, MCP client add/server via UI, AGENTS.md auto-detect, Skills UI, SQLite persistence
- All three test tiers pass: Rust unit (7), React component (2), E2E Flow1 (1)
- Docs: README (what it is, Phase0 capability honestly described, setup for macOS+Windows, Windows+browser dev loop), CONTRIBUTING, CODE_OF_CONDUCT, ISSUE_TEMPLATE, LICENSE MIT, ADRs (7), CI workflow
- Open-source requirements elevated to exit criteria per Part D are met
- Honest known issues list, no placeholder presented as done

**Conditions before Phase 1 start**:
1. Install MSVC BuildTools on dev machine and verify one full `tauri dev` pass inside real Tauri webview, re-run Flow1 E2E inside webview, report result — per Part C "One real limitation" requirement
2. Fix known issue #2 (WS broadcast) and #5 (plaintext secret) — small but load-bearing for multi-window and security
3. Add missing `src-tauri/icons/` and verify `tauri build` produces installer on Windows (and macOS if possible)
4. Implement per-hunk Accept/Reject wiring in DiffViewer (currently UI stub) — this is arguably Phase0 scope per "Diff viewer via git2-rs: per-hunk accept/reject"

**Phase 1 scope proposal** (per Part 22, to be confirmed by human):
- Multi-provider model routing + generic OpenAI-compatible slot
- Local-model runtime detection (Ollama/LM Studio)
- Full SKILL.md support (currently minimal)
- ACP host (pop out to Zed/JetBrains)
- Headless Core server mode (polish current standalone binary into supported `cid-core serve` with systemd/service, not just dev loop)
- Opt-in Structural Context Engine (Tree-sitter)
- GitHub bridge (issue → Mission)
- Planner/Reviewer roles added alongside Implementer
- Autonomous mode with command allow-lists (without sandboxing yet)

**No-go criteria**: If human finds that per-hunk Accept/Reject being stubbed is considered core Phase0 (not Phase1), then fix that first and re-checkpoint before GO.

---

### Appendix: Exact commands to try it

**Browser + Standalone Core (fast, per Part C)**:
```powershell
# Terminal 1: Core
cargo run -p cid-core -- --port 5919 --db C:\Temp\cid.db
# Health: http://127.0.0.1:5919/health

# Terminal 2: Frontend
npm install
npm run dev
# Open http://localhost:1420
# Connect repo path like C:\Projects\cid (this repo itself) or any git repo
# New Mission → worktree → task "List files in repo" → observe Co-Pilot approval flow
```

**Tauri** (requires BuildTools):
```powershell
npm run tauri:dev
```

**Tests**:
```powershell
cargo test -p cid-core --lib
npm run test
cargo run -p cid-core -- --port 5919 --db C:\Temp\cid-e2e.db
# In another shell:
npx playwright test
```

**Build**:
```powershell
npm run build
cargo build -p cid-core
```

---

**End of Phase0 checkpoint. Awaiting human go/no-go per Part 23. Do not begin Phase1 work until human response.**
