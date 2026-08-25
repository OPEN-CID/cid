# CID — Collaborative Intelligent Development

> A chat-native, multi-agent software engineering platform — Slack-shaped session control for shipping code with AI agents.

CID organizes work as **Workspaces → Repo Channels → Session Threads**, the same mental
model as Slack/Teams. A Session ("Build OAuth," "Fix #245") runs in an isolated git
worktree (default) or a shared clone, with a Planner → Implementer → Reviewer loop,
per-tool approval at Manual/Co-Pilot autonomy or a governed, allow-listed Autonomous
mode, inline diff review, a real terminal, and MCP tool access — all inside the thread,
not a separate app you alt-tab into.

**One Core, many surfaces**: a single Rust/Tokio daemon (`cid-core`) exposes everything
over JSON-RPC 2.0 (HTTP + WebSocket). The desktop app (Tauri v2), the browser (same React
bundle, served headless), the CLI/TUI (`cid-tui`), and a mobile approval/monitoring shell
are all thin clients over that one API — there is no separate backend to keep in sync.

## What's in v1.0

**Agent system** — Planner/Implementer/Reviewer as prompt+model+tool-permission
configurations (not a fixed cast of independent agents), plus ad hoc subagents for
scoped parallel work. Three autonomy levels per Session: Manual (you drive), Co-Pilot
(every tool call shown and approved, the default), Autonomous (runs a governed,
per-repo command allow-list without per-step approval; a human still reviews the final
diff). A **vibe-coding preset** skips the Planner's ceremony for quick, low-stakes
changes — the plan is auto-approved so the Implementer starts immediately, while
Co-Pilot's per-tool-call approval, the diff viewer, and History stay exactly as they are.

**Confidence Engine** — 9-signal patch scoring (symbol resolution, static analysis, type
validation, architecture-rule validation, test impact, duplicate detection, dependency
impact, semantic similarity, existing-reuse) surfaced inline before you approve a change.

**Git, diff & terminal** — `git2-rs`-backed worktrees, per-hunk accept/reject (not just
whole-file), atomic auto-commits per logical change, a real native PTY per Session with
default secret redaction in both the live view and stored history.

**Context & code intelligence** — opt-in Tree-sitter structural indexing, `AGENTS.md`
(Linux Foundation/AAIF standard) and `SKILL.md` (Anthropic Agent Skills) layered
Workspace → Repo Channel → Session Thread with nearest-scope-wins resolution, a
test-impact graph and a documentation graph once the semantic engine is enabled, plus
hybrid (BM25 + embedding) search via Tantivy.

**Model routing** — Anthropic, OpenAI, and Google natively, one generic
OpenAI-compatible endpoint slot (covers OpenRouter, Groq, self-hosted vLLM, and most
others), hardware-gated local-model detection (Ollama/LM Studio/`llama.cpp --server`),
and per-role (Planner/Implementer/Reviewer) model overrides.

**Security & governance** — least-privilege MCP servers scoped per Repo Channel, OS-native
credential storage for secrets (never plaintext in SQLite), a two-layer Autonomous-mode
sandbox (command allow-list + path policy on every platform; kernel-level confinement via
`sandbox-exec` on macOS and `bubblewrap` on Linux — Windows Job Objects do **not** confine
the filesystem, documented honestly rather than glossed over), multi-user local auth
(Argon2id, role hierarchy Viewer<Reviewer<Developer<Admin<Owner), and workspace-level
governance (who can enable Autonomous mode, on which repos, with spend caps).

**Integrations** — GitHub, GitLab, Bitbucket (issue/PR bridges), Jira and Linear
(ticket linkage, not a project-tracker replacement), Slack and Microsoft Teams bridges.
CID does not integrate with deployment providers (no "Deploy to X" buttons) — that's a
deliberate, permanent scope boundary, not a gap; see `docs/041-Roadmap.md`.

**Surfaces** — desktop (Tauri v2, macOS/Windows), web (same bundle, headless Core), a
mobile companion app (approval/monitoring, not full editing), and a CLI/TUI
(`cid-tui`, `ratatui`-based) for chat, session status, and tool-call/plan approval from a
terminal.

**Repository Health & observability** — a signal-based dashboard over the repo's own
test suite (test presence per module, duplicate-test detection — not instrumented line
coverage, named as a real gap rather than faked), Prometheus-style `/metrics`, and a
local, secret-redacted crash log with a tested no-code-leakage guarantee.

**Autonomous-mode command controls** — a per-repo settings panel over the command
allow-list: toggle any pattern between auto-run and ask-first (e.g. `git commit`
auto-approved, `git push`/PR-opening commands always asked for), add custom patterns,
edit denied paths.

## Architecture

```
                    ┌───────────────────────────┐
                    │   CID Core (Rust, Tokio)  │
                    │  git · PTY · MCP client   │
                    │  ACP host · model router  │
                    │  context engine · SQLite  │
                    └─────────────┬─────────────┘
                                  │ JSON-RPC 2.0 over HTTP + WS
        ┌──────────────┬─────────┼─────────┬──────────────┐
        │              │         │         │              │
   Desktop Shell    Web Shell  Mobile Shell   CLI/TUI    Headless
  (Tauri v2)        (browser)  (approval/    (cid-tui)   (CI, remote
                                 monitoring)               trigger)
```

## Setup

### Prerequisites

- Node.js 18+
- Rust (stable), with MSVC Build Tools on Windows
- Git

### Quick start — browser + standalone Core

```powershell
npm install
cargo run -p cid-core -- --port 5919      # in one shell
npm run dev                                # in another; opens http://localhost:1420
```

Core exposes `ws://127.0.0.1:5919/ws` (JSON-RPC 2.0), `http://127.0.0.1:5919/api/rpc`
(HTTP fallback), `http://127.0.0.1:5919/health`, and `http://127.0.0.1:5919/metrics`
(Prometheus text format).

### Desktop app

```powershell
npm run tauri:dev
npm run tauri:build
```

### CLI/TUI

```powershell
cargo run -p cid-tui -- --host 127.0.0.1 --port 5919
```

### Dev Container

A `.devcontainer/devcontainer.json` gets a contributor a working Rust + Node toolchain
on any OS with zero manual setup, scoped to the browser+Core dev loop (see ADR 0016 for
why it doesn't cover the Tauri desktop build).

See `CONTRIBUTING.md` for the full contributor setup path, verified end-to-end on a
clean checkout.

### Running in production

The steps above are for local development. For running `cid-core` for a real team —
TLS, a persistent service, database backups, upgrades, monitoring — see
`docs/052-Production-Deployment.md`. A `Dockerfile`/`docker-compose.yml` are provided
as one path to a running instance; building from source is the other.

## API contract

`cid-core/src/api/types.rs` and `cid-core/src/api/router.rs` are the authoritative
source for the full JSON-RPC method list — it is large (workspace/repo/session/message,
git, PTY, MCP, file, skills, settings, model, autonomy allow-lists, auth, governance,
forges, trackers, confidence scoring, decisions/ADRs, repo health, observability, and
more) and grows with the product; rather than duplicate it here and let it drift, read
the router's `match method.as_str()` block directly.

## Testing

```powershell
cargo test --workspace --exclude cid --all-features   # Rust: unit + integration + fuzz + property + perf
npm run test                                            # frontend unit tests (vitest)
npx playwright install && npm run test:e2e              # E2E, against a real Core + real dev server
npx tsc --noEmit                                         # frontend typecheck
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs all of the above on every PR.

## Docs

- `docs/000-*.md` through `docs/047-*.md` — the full design/spec doc tree (vision, goals,
  non-goals, architecture, tradeoffs, failure modes, security, testing, acceptance
  criteria per subsystem), per `CID-Doc-Template.md`.
- `docs/adr/` — Architecture Decision Records, one per non-trivial engineering choice.
- `docs/CHECKPOINT-Phase0.md` through `docs/CHECKPOINT-Phase6.md` — what was built, what
  was deferred, known issues, and test status at each phase boundary.
- `docs/041-Roadmap.md` — what v1.0 includes and what's deliberately not in it yet.
- `docs/045-Dependency-Audit.md` — the stack table, re-verified against its current state
  rather than assumed unchanged since project inception.
- `docs/046-Crate-Layout.md` — what each workspace crate is for.
- `ai-review-prompts/` — the phase build prompts, rewritten to describe what's actually
  in the codebase today rather than the original aspirational brief, so another AI model
  (or a human) can use them to audit for gaps between spec and implementation.

## Open source

MIT license. `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`,
`.github/CODEOWNERS`, `.github/pull_request_template.md`, `.github/ISSUE_TEMPLATE/`.

## What's not in v1.0, and why

A native rendering engine (Monaco/CodeMirror instead — Zed took ~5 years to build one
with dedicated funding and Tree-sitter's own creators), enterprise/air-gapped hardening,
and a hosted "CID Cloud" are all deliberately deferred pending real usage evidence, not
cut. See `docs/041-Roadmap.md` for what evidence would change each of those.
