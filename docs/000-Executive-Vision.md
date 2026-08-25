# 000 — Executive Vision

## Vision

CID (Collaborative Intelligent Development) is a chat-native, multi-agent software
engineering platform: Slack-shaped session control for shipping code with AI agents.
Workspaces, Repo Channels, and Session Threads replace the usual IDE-plus-chat-window
split with one navigation model, so a unit of work — a plan, its diffs, its terminal
output, its review — lives in one place instead of scattered across an editor tab, a
chat app, and a terminal window.

## Goals

- One coherent surface for human + AI collaboration on real code: chat, editor, terminal,
  diff review, and MCP tool access in a single Session thread, not five separate apps.
- Real, verified execution — every AI-authored change runs in an isolated git worktree,
  is reviewable per-hunk, and (as of Phase 4) carries a Confidence score with a
  plain-language explanation, not a bare "looks good."
- Human-in-the-loop by construction: outside Manual autonomy, nothing the Implementer
  does runs without an approved plan (enforced in Core, not the UI — see
  `cid-core/src/roles/mod.rs`); Autonomous mode is additionally gated by Workspace
  governance policy and a command allow-list.
- Reach across how developers actually work: desktop (Tauri), browser (Web Shell), phone
  (approval/monitoring), and terminal (`cid-tui`) all talk to the same Core over the same
  JSON-RPC contract.
- Team-ready without being enterprise-first: local accounts and Workspace roles (Phase 3)
  exist so a small team can share a Core, without requiring an identity provider or a
  hosted backend CID doesn't have.

## Non-Goals

- Out-featuring JetBrains on static analysis/refactoring tooling.
- Out-building Monaco/CodeMirror/Zed's GPUI on editor rendering — CID embeds proven
  editors and is an ACP host so a Session can pop out to a full external IDE
  (`cid-core/src/acp/mod.rs`) rather than reinventing one. Revisit only with real profiling
  evidence, which does not yet exist (see 041-Roadmap.md).
- Replacing Slack, Jira, Linear, or a project tracker — CID integrates with these
  (`cid-core/src/forges`, `cid-core/src/trackers`, `cid-core/src/slack_bridge`) rather than
  re-implementing them.
- Deployment orchestration. `cid-core/src/decisions/mod.rs`'s deployment record is a log —
  what/when/where — never an action CID performs. No provider SDK is a dependency.
- Air-gapped/enterprise hardening and a hosted "CID Cloud" — explicitly deferred pending
  real demand signal, not built speculatively.

## Architecture

See `004-System-Architecture.md` for the full picture. In one line: a single Rust/Tokio
Core exposes git, PTY, MCP, model routing, and persistence over JSON-RPC 2.0; four thin
shells (desktop, web, mobile, CLI/TUI) are clients of that one API.

## Tradeoffs

The founding brief (Appendix A across the phase prompts) is explicit about this: every
major technical choice here traded a more ambitious version of itself for one that
actually ships. A from-scratch native editor, a 12-agent org chart, and a full knowledge
graph on day one were all considered and rejected in favor of embedding proven components
(Monaco, three composable roles, a Tree-sitter structural index) and expanding
only where profiling or real usage justified it. The cost is that CID is not the most
novel system in any single dimension; the benefit is that what exists actually runs,
end-to-end, with 392 passing tests as of Phase 4.

## Failure Modes

- **Scope creep back toward the rejected v1 ambition** — the single biggest risk to this
  project's own history (see the "what changed, and why" table carried through every
  phase prompt). Mitigated by the non-goals above being restated, not softened, at each
  phase boundary.
- **Silent regression in what's already built** — mitigated by a real test suite (392
  tests across `cid-core`, `cid-tui`, and the frontend) run before every checkpoint, not
  after.
- **A checkpoint reporting something as done when it wasn't** — happened at least twice in
  this project's real history (the Phase 2 sandbox boundary test that could not fail; the
  unwired Confidence Engine found dead in Phase 4) and was caught by re-verification, not
  prevented by process. See 042-ADRs.md and the Phase 2/4 checkpoints for the honest
  account.

## Security

Threat model and boundaries are documented in full in `031-Security.md` and the
repository's own `SECURITY.md`. In brief: worktree isolation for AI edits, an explicit
plan-approval gate, a command allow-list plus (platform-dependent) kernel sandboxing for
Autonomous mode, and local accounts with Argon2id hashing for multi-user Workspaces.

## Testing

392 tests pass across the workspace as of Phase 4 (`cargo test --workspace`, `npm test`).
See `037-Testing.md` for the full breakdown by category (unit, integration, protocol
fuzzing, worktree property tests, performance budgets).

## Implementation Order

Phases 0–4 are complete as of this document. See `041-Roadmap.md` for what shipped in
which phase and what remains conditional (Phase 5+).

## Acceptance Criteria

- A user can connect a repo, start a Session, get a Planner-authored plan, approve it,
  watch the Implementer execute with per-tool approval, review a per-hunk diff, see a
  Confidence score, and merge — entirely within one Session thread. This is Flow 1 from
  the founding brief, exercised end-to-end by `tests/e2e/flow1.spec.ts`.
- Every claim in this document is backed by a real file path, RPC method, or test that
  exists in the repository at the time of writing — not aspirational architecture.

## AI Coding Rules

- Read the relevant `docs/0XX-*.md` file before implementing a milestone that touches it,
  per the Doc Template's own instruction — and update the doc if implementation reveals it
  was wrong.
- Never describe unbuilt functionality as though it exists. If something is planned but
  not built, say so and name the phase it belongs to.
- No placeholder code presented as done (Appendix A Part 0, rule 2) — this discipline
  produced the honest checkpoint reports these docs are built from.
