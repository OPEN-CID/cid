# Phase 1 review — multi-provider routing, ACP host, Autonomous mode

Source of truth: `docs/CHECKPOINT-Phase1.md`. This checkpoint documents real gaps found
in what an earlier session claimed was complete — the ACP host and the Planner/Reviewer
plan-approval gate were both unwired despite being reported done. Verify the fixes hold.

## Claims to verify

1. **Multi-provider model routing**: Anthropic, OpenAI, Google natively, plus one generic
   OpenAI-compatible endpoint slot. Check: `cid-core/src/model/mod.rs`'s provider-specific
   call functions, `model.list` RPC (`api_integration.rs`'s
   `model_list_exposes_all_phase1_providers` test).
2. **ACP host has real RPC methods, not zero.** Check: `cid-core/src/acp/mod.rs`
   (`AcpHostManager`, `handoff`/`take_back`/`take_back_async`), and in `router.rs`:
   `acp.editors.list`, `acp.handoff`, `acp.take_back`, `acp.handoff.get`,
   `acp.handoff.list`. Run the `acp_*` tests in `api_integration.rs` (7 tests as of this
   writing).
3. **Planner → human-approval → Implementer gate actually blocks execution.** This is the
   literal Phase 0 golden path and was found completely unimplemented in an earlier
   session despite being reported done. Check: `cid-core/src/roles/mod.rs`
   (`RoleRunner::generate_plan`, `implementer_is_gated`), the plan-approval check at the
   top of `handle_mission_send_message` in `router.rs`. Run
   `co_pilot_mission_is_gated_until_a_plan_is_approved`,
   `editing_an_approved_plan_revokes_the_approval`,
   `rejecting_a_plan_keeps_the_gate_closed`, and `manual_autonomy_has_no_plan_gate` in
   `api_integration.rs` — all four should exist and pass.
4. **Reviewer pass produces a real, structured verdict**, not a stub. Check:
   `RoleRunner::run_review`/`latest_review`, `parse_findings`/`verdict_for` in
   `roles/mod.rs` (17+ unit tests), `mission.review.run`/`.get`/`.list` RPC methods.
5. **Full `SKILL.md` support** (not just a single markdown snippet). Check:
   `cid-core/src/skills/mod.rs`, `skills.bundles.list`/`skills.bundle.write`/
   `skills.resolve` RPC methods, the layered-resolution tests
   (`skills_resolve_returns_a_layered_context_stack`,
   `skills_resolve_puts_mission_context_last`) in `api_integration.rs`.
6. **Headless Core server mode** — Core runs with no shell attached, driven entirely over
   RPC. Trivially true by construction (Core is a Tokio daemon with no GUI dependency),
   but confirm no code path assumes a frontend is present (e.g., a panic or silent no-op
   if no WebSocket client is ever connected).
7. **Opt-in Structural Context Engine** (Tree-sitter), off by default per repo. Check:
   `cid-core/src/context_engine/mod.rs`, the `context_engine_is_off_by_default_and_toggles_per_repo`
   test.
8. **GitHub bridge**: issue → Mission trigger, PR open/status sync. Check:
   `cid-core/src/github/mod.rs`.
9. **Autonomous mode with command allow-lists, unsandboxed** (Phase 1's stated boundary,
   not a defect — kernel sandboxing is Phase 2). Check: `cid-core/src/autonomy/mod.rs`,
   `autonomy.allowlist.*` RPC methods, and specifically that an *unconfigured* scope
   denies by default rather than allowing (`autonomy_denies_by_default_when_no_allowlist_is_configured`
   test) — this is the single most important claim in this file to actually re-run, since
   "deny by default" vs. "allow by default" is exactly the kind of thing that's easy to
   silently invert in a refactor.

## Known caveats from this phase's own checkpoint (confirm still accurately described)

CORS was `Any` at Phase 1's end, tightened in Phase 2 — confirm `router.rs`'s
`CorsLayer` still reflects an explicit allow-list, not `Any`, as of the current code (a
regression here would be a real, serious finding). The Reviewer's diff input is a
serialized `GitDiff` struct rather than a raw unified diff (a token-cost tradeoff, not a
bug) — confirm this is still true or has been changed, and if changed, whether
`docs/`'s description was updated to match.
