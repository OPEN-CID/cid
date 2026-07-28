# Changelog

All notable changes to CID, summarized in plain language. See `docs/CHECKPOINT-Phase0.md`
through `docs/CHECKPOINT-Phase6.md` for the full detail behind each entry, and
`docs/041-Roadmap.md` for the phase-by-phase build history this file summarizes.

## v1.0.0 — Release

The first tagged release. Everything below shipped across Phases 0–6; this release adds
no new capability — it audits, regression-tests, documents, and packages what already
exists.

### Added across Phases 0–6

- **Chat-native workspace model**: Workspaces → Repo Channels → Mission Threads, with a
  real editor (CodeMirror inline + Monaco full-pane), terminal, diff viewer, MCP tool
  access, and command history embedded in the thread.
- **Planner → Implementer → Reviewer** agent loop with three autonomy levels (Manual,
  Co-Pilot, Autonomous) and a **vibe-coding preset** for low-ceremony quick changes.
- **Confidence Engine**: 9-signal patch scoring surfaced before approval.
- **Isolated git worktrees per Mission** (or shared-clone mode), per-hunk diff
  accept/reject, atomic auto-commits, a real native PTY terminal with secret redaction.
- **Context & code intelligence**: opt-in Tree-sitter structural indexing, `AGENTS.md` +
  `SKILL.md` layering with nearest-scope-wins resolution, a test-impact graph, a
  documentation graph, hybrid BM25+embedding search.
- **Model routing**: Anthropic/OpenAI/Google natively, one generic OpenAI-compatible
  slot, hardware-gated local-model detection, per-role model overrides.
- **Security & governance**: least-privilege MCP scoping, OS-native credential storage,
  a two-layer Autonomous-mode sandbox (command allow-list + path policy everywhere;
  kernel isolation on macOS/Linux — Windows's real limitation documented, not hidden),
  local multi-user auth with a role hierarchy, workspace governance policy.
- **Integrations**: GitHub, GitLab, Bitbucket, Jira, Linear, Slack, Microsoft Teams.
- **Surfaces**: desktop (Tauri v2), web (headless Core), mobile (approval/monitoring),
  CLI/TUI (`cid-tui`).
- **Repository Health**: test-presence and duplicate-test signals over the repo's own
  suite.
- **Observability**: Prometheus-style `/metrics`, a local secret-redacted crash log with
  a tested no-code-leakage guarantee.
- **Autonomous-mode command controls UI**: per-repo allow/ask-first toggles, custom
  patterns, denied paths.
- 45+ design/spec documents (`docs/000-*.md` onward) and 16+ ADRs.

### Fixed

A representative sample of real bugs found by testing against genuine data rather than
hand-built fixtures or trusting a prior checkpoint's summary — the full list, with every
item's disposition, is in the Release Report:

- The Confidence Engine, test-impact graph, and documentation graph each shipped with
  real internal bugs in an earlier pass (architecture-validation false-positive/negative,
  a definitions-vs-references confusion, a staleness-detection tautology) — found via
  real Tree-sitter/file-content output, fixed, regression-tested.
- `settings.get` leaked every configured provider's plaintext API key in an unused
  response field; `settings.update` rejected any partial update. Both fixed; the
  regression test that should have caught the leak (and didn't, because it checked a
  field path that never existed in the real response) was rewritten to actually exercise
  the redaction path.
- The E2E test suite had rotted (CommonJS globals in an ESM project, mismatched RPC
  param names, assertions that silently passed without checking anything) — fixed and
  wired into CI, which had also silently excluded most of the integration/fuzz/property
  test suite until this pass.
- `repo_health.scan` initially miscounted a test fixture string (one that quotes example
  `#[test]` source as a string literal) as a real duplicate test — found by running it
  against this actual repository, fixed with a masking pass and a regression test.

### Security

- Windows sandbox boundary documented honestly: Job Objects do not confine the
  filesystem; Autonomous mode relies on the command allow-list and path policy on
  Windows, with kernel isolation only on macOS (`sandbox-exec`) and Linux (`bubblewrap`).
- Non-loopback Core binds require a bearer token by construction (fail at startup
  otherwise), not by convention.
- Secrets are never sent to a model as plain context, redacted by default in terminal
  output and stored history, and held in OS-native credential storage rather than
  plaintext in SQLite.

### Deliberately not included

Native rendering engine, enterprise/air-gapped hardening, hosted "CID Cloud",
deployment-provider integrations. See `docs/041-Roadmap.md`'s v1.0 scope statement for
why, and what evidence would change each one.
