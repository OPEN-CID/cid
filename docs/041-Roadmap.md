# 041 — Roadmap

## Vision

The real, phase-by-phase build history — what shipped when, cross-referenced to
checkpoint reports and ADRs, not an aspirational future plan.

## Goals

**Phase 0 — "A real single-agent coding assistant in a Slack-shaped UI."** Desktop app
(Tauri v2, macOS+Windows), one Workspace/Repo Channel at a time, worktree/shared-clone
Isolation, real PTY terminal, `git2-rs` diff viewer with per-hunk accept/reject,
Anthropic-only chat with a real tool-use loop under Co-Pilot autonomy, basic MCP client,
`AGENTS.md` auto-detection, SQLite persistence. See `docs/CHECKPOINT-Phase0-Final.md`.

**Phase 1 — "Extensible and remote-capable."** Multi-provider routing (OpenAI, Google,
generic OpenAI-compatible), local-runtime detection, full `SKILL.md` support, ACP host,
headless Core server mode, opt-in Structural Context Engine, GitHub bridge, Autonomous
mode with command allow-lists. **Also closed in this pass, found unwired from an earlier
session**: the ACP host had zero RPC methods; Planner/Reviewer existed only as
model-routing configs with no actual plan-approval gate or review pass — the literal
Phase 0 golden path was not implemented despite being the stated exit criterion. See
`docs/CHECKPOINT-Phase1.md`.

**Phase 2 — "A real multi-surface platform."** Web shell, Slack/Teams bridges,
multi-agent-per-Session subagents, background/ambient local model, Semantic Context
Engine (Tantivy + embeddings + dependency graph), MCP Apps/Tasks, sandboxing, Linux CI.
**Also corrected in this pass**: the sandbox boundary test was a tautology and Windows
Job Objects don't confine the filesystem (ADR 0011); the sandbox was never applied to
real tool execution; Web Shell access control was UI state enforcing nothing with CORS
`Any` (ADR 0012); repository indexing was a no-op that logged "Would index" and never
indexed anything — no Tantivy dependency existed despite Part 18 naming it. See
`docs/CHECKPOINT-Phase2.md`.

**Phase 3 — "Team-ready."** Local accounts and Workspace roles (ADR 0013), governance
policy enforced at real decision points, GitLab/Bitbucket bridges, Jira/Linear linkage,
mobile companion shell (ADR 0010's bake-off selected Tauri v2 Mobile), the Part 21
cross-platform test-matrix floor (protocol fuzzing, worktree property tests, performance
budgets). See `docs/CHECKPOINT-Phase3.md`.

**Phase 4 — "Confidence, role profiles, decisions, deployment, CLI/TUI."** Confidence
Engine wired end-to-end after being found completely dead code (never in `lib.rs`, never
compiled, with five real bugs including a false-confidence architecture-validation bug —
`014-Patch-Verification.md`); test-impact and documentation graphs
(`006-Repository-Digital-Twin.md`, `015-Test-Impact-Analysis.md`); configurable role
profiles with real tool-permission enforcement (`008-Agent-Operating-System.md`);
Decisions view and Deployment record (`013-Repository-Health.md`); `cid-tui` CLI/TUI
shell (ADR 0014); this 45-document backfill.

**Phase 5 — complete.** Dependency audit (`045-Dependency-Audit.md`), contributor
experience (`CONTRIBUTING.md`, devcontainer decision/ADR 0016, CI extension closing the
coverage gap named in `036-CI-CD.md`, `CODEOWNERS`, PR template), vibe-coding Session
preset, persona-coverage audit (manual/Co-Pilot/Autonomous/CLI-first/GUI-first/
diff-review-first — cid-tui's missing diff view flagged, not silently fixed mid-audit).
**Also found and fixed in this pass, via actually running the E2E suite rather than
trusting it passed**: `settings.get` leaked every provider's plaintext API key in an
unused `full_settings` field; `settings.update` rejected any partial update because it
required the full `Settings` struct; the E2E suite itself had rotted (CommonJS globals in
an ESM project, mismatched param names, tests that silently passed without checking
anything). See `docs/CHECKPOINT-Phase5.md`.

**Phase 6 — complete.** `CID-Phase6-Build-Prompt.md` never existed as a file — built to
the one-line `CID-Roadmap-Index.md` description instead: Repository Health (test
presence + duplicate-test detection, `repo_health.scan`), crate-layout documentation
(`046-Crate-Layout.md`), observability (`/metrics`, a local secret-redacted crash log),
and — found missing during a product-design audit prompted by real usage feedback — a
frontend panel for the Autonomous-mode command allow-list, whose backend had existed
since an earlier phase with zero UI. **Also found and fixed by dogfooding the Health
feature against this actual repository**: it initially misparsed a test fixture string
that quotes example `#[test]` source as a second real duplicate test. See
`docs/CHECKPOINT-Phase6.md` and `047-Repository-Health-Observability.md`.

**Release — this pass.** Cross-checkpoint known-issues audit, full regression pass, a
rewritten README/CHANGELOG, and this roadmap update, per `CID-Release-Prompt.md`. See
the Release Report for the consolidated disposition of every open item across all seven
checkpoints.

## v1.0 scope statement

Everything listed under Phases 0–6 above is in v1.0. Deliberately **not** in v1.0, as a
scoping decision rather than an oversight:

| Deferred | What evidence would change that |
|---|---|
| A native rendering engine (Monaco/CodeMirror instead) | Real profiling on a real, used CID instance showing Monaco/CodeMirror is *actually* the bottleneck — not a hypothetical one. Zed's own ~5-year build time with dedicated funding and Tree-sitter's creators on the team is the standing evidence this is not a small undertaking. |
| Enterprise/air-gapped hardening | Real demand from a team that needs it — SSO/OIDC, an offline model-and-MCP-server story, and audit requirements beyond the current History panel are each their own scoped project once that demand is concrete. |
| A hosted "CID Cloud" | Real demand from users who don't want to self-host — a different business, not a feature flag on the current architecture. |
| Deployment-provider integrations (Vercel/AWS/Azure/GCP "Deploy to X") | None — this is a permanent, non-evidence-gated boundary (Part 1's own non-goal): deploy orchestration is itself a full product category CID does not compete in. The Deployment record (Phase 4) logs what/when/where; it will never orchestrate. |
| Instrumented test-coverage percentages (vs. the current presence signal) | A CI build step (tarpaulin/llvm-cov) this repo doesn't have wired up yet — a real, scoped follow-on, not evidence-gated the way the three items above are. |
| cid-tui diff view | Real signal that the CLI-first persona is used enough in practice to be worth a second diff-rendering implementation, distinct from the web/desktop one. |
| A third-party UI extension/theme ecosystem (VS Code-style) | Real demand beyond what MCP servers + MCP Apps + `SKILL.md` already cover (see `049-Extensibility-And-Sync-Roadmap.md` for the full analysis of what's already an extension point vs. what's genuinely missing). |
| Cross-device sync beyond the same-network case (which already works today — see `049-...md`) | Real users who've outgrown LAN-only mobile access and specifically want cross-network visibility — natural home for an opt-in, off-by-default `sync.enabled` flag and a possible paid hosted tier. |

"Not yet" is a complete, legitimate answer for every row above — not a gap to force-fill
before a release.

## Non-Goals

Everything in the v1.0 scope statement's table above.

## Architecture

N/A.

## Tradeoffs

N/A — see each phase's own checkpoint for its specific tradeoffs.

## Failure Modes

This roadmap's most important honest thread: **at least four significant "already built"
claims from earlier sessions turned out to be false on re-verification** (Phase 1's
unwired ACP/plan-gate, Phase 2's tautological sandbox test and no-op indexer, Phase 4's
dead Confidence Engine). Each was caught by treating the phase prompts' own instruction
to verify seriously rather than trusting a prior checkpoint's summary — the corrective
this document, and `039-AI-Implementation-Rules.md`, both exist to institutionalize.

## Security

See each phase's security-relevant corrections above and `031-Security.md`.

## Testing

Test count by phase-end: Phase 0 — a handful of unit tests + 1 E2E. Phase 2 (as
originally checkpointed, before correction) — 125. Phase 3 — 305. Phase 4 — 392. Phase 5
— 391 Rust + 30 E2E + 2 frontend unit. Phase 6 — 406 Rust + 30 E2E + 2 frontend unit.

Current (2026-07-27, every gate re-run in the `050-Gold-Standard-Review.md` pass):
**498 Rust (1 ignored) + 32 frontend unit across 8 files + 30 E2E**, with clippy at
`-D warnings` and `tsc --noEmit` both clean. Note that E2E requires `cid-core` running on
:5919 — `playwright.config.ts` starts vite only, so a bare `npx playwright test` reports
16 failures against a dead socket (`050` F9).

## Implementation Order

This document is the implementation order, stated retrospectively.

## Acceptance Criteria

Every phase's checkpoint report states honestly what was built, what was deferred, known
issues, and test status — none claim completion without the corresponding tests passing
at time of writing.

## AI Coding Rules

Before starting Phase 6+ work, re-read this document's Non-Goals — Phase 6+ is
explicitly conditional on evidence that, as of Phase 5, still does not exist. Do not
begin it speculatively.
