# Phase 2 review — web shell, sandboxing, semantic context engine

Source of truth: `docs/CHECKPOINT-Phase2.md`. Four real gaps were found and fixed in this
phase's own verification pass: a tautological sandbox-boundary test, Windows Job Objects
not actually confining the filesystem despite being described as if they did, a
web-shell access-control layer that was UI state enforcing nothing (CORS `Any`), and a
repository indexer that logged "Would index" and never actually indexed anything despite
Tantivy being named in the tech-stack table. Verify all four fixes hold.

## Claims to verify

1. **Web shell**: the same React bundle served by headless Core, not a separate backend.
   Check: `npm run dev` + `cargo run -p cid-core -- --port 5919`, confirm the browser app
   at `http://localhost:1420` talks to Core's JSON-RPC API directly (`src/lib/api.ts`).
2. **Sandbox boundary is real, not a tautology.** Check:
   `cid-core/src/sandbox/mod.rs`'s `execute_sandboxed`, `path_policy_violation`,
   `verify_sandbox_boundary`/`boundary_report`. Run `sandbox_test_rpc_reports_the_boundary_held`
   and `sandbox_status_describes_what_it_actually_enforces` in `api_integration.rs`. The
   critical claim: `status()` must report `available: false` on Windows, honestly, not
   claim kernel confinement it doesn't have. Read `docs/adr/0011-windows-sandbox-boundary.md`
   and confirm the code matches what the ADR says, not what an earlier, corrected draft
   said.
3. **Access control**: non-loopback binds require a bearer token by construction (fail at
   startup otherwise), not by UI convention. Check: `cid-core/src/access/mod.rs`'s
   `AccessPolicy::new`, and that `main.rs` actually calls it before `Core::new()` (a
   policy built but never enforced would be the same class of bug as the original gap).
   Run `protected_core_accepts_the_right_token`, `protected_core_rejects_a_wrong_token`,
   `protected_core_rejects_rpc_without_a_token`.
4. **Semantic Context Engine actually indexes**, not a no-op. Check:
   `cid-core/src/semantic_engine/index.rs` (real Tantivy `SearchIndex`),
   `semantic_engine/mod.rs`'s `index_repository_blocking`. Run
   `semantic_engine_is_off_by_default` and confirm a *positive* test exists proving a
   real search index gets built when enabled (not just an off-by-default test — the
   original bug was specifically that enabling it did nothing).
5. **Slack/Teams bridges**: plugin-style, channel-mapped triggers. Check:
   `cid-core/src/slack_bridge/mod.rs`, `cid-core/src/teams_bridge/mod.rs`.
6. **Subagents-per-Session**: scoped, short-lived workers inheriting the parent's
   worktree and tool permissions. Check: `cid-core/src/subagent/mod.rs`
   (`SubagentOrchestrator`) — and specifically confirm this file has **not** been
   corrupted or replaced with a version referencing nonexistent modules; an external
   process corrupted this exact file once during this project's build history (see
   `CLAUDE.md`'s Background Task Delegation Rule caution, if still present, for that
   incident's context) and it was restored via `git checkout`.
7. **MCP Apps + Tasks extension support**, targeting the 2026-07-28 spec shape. Check:
   `cid-core/src/mcp/mod.rs`, `cid-core/src/mcp_tasks/mod.rs`.
8. **Linux CI target added.** Check: `.github/workflows/ci.yml`'s `test-rust-linux` job.

## What to specifically distrust and re-verify

This phase's checkpoint is explicit that "already built" claims from an earlier session
were false four separate times in this phase alone. Do not accept "the module exists and
compiles" as evidence any of the above actually works — re-run the specific tests named,
and where reasonable, exercise the RPC method directly against a running Core rather than
only reading the source.
