# Phase 4 review — Confidence Engine, graphs, role profiles, CLI/TUI

Source of truth: `docs/CHECKPOINT-Phase4.md` (written retroactively during Phase 5 — the
checkpoint itself was skipped when this phase was originally built, a process gap worth
noting if it happens again). The Confidence Engine, test-impact graph, and doc graph
were each found to have real, distinct bugs during this phase's own verification —
verify none have regressed.

## Claims to verify

1. **Confidence Engine is actually wired in, not dead code.** This is the single most
   important claim in this file: an earlier version had a fully-built
   `cid-core/src/confidence/mod.rs` that was never `pub mod`-declared in `lib.rs` and
   therefore never compiled into the running binary at all. Check: `lib.rs` has
   `pub mod confidence;`, `Core` holds a `confidence_engine: Arc<ConfidenceEngine>` field,
   `AppState` in `router.rs` does too, and a real RPC method calls it. Run the 28 unit
   tests in `confidence/mod.rs` plus `confidence_score_is_computed_and_logged_to_the_mission`,
   `confidence_score_reads_the_worktree_file_when_no_content_is_supplied`,
   `confidence_score_without_content_or_an_existing_file_fails_clearly` in
   `api_integration.rs`.
2. **9-signal scoring is real, not a placeholder average.** Check: `ArchitectureRule`,
   `ArchitectureRule::check`, `parse_architecture_rules_from_md`,
   `extract_backticked`, `Patch::from_content` in `confidence/mod.rs`. The checkpoint
   names a specific found-and-fixed bug: an architecture-validation false-positive/false-
   negative pair. Confirm the regression test for that specific bug still exists and
   passes.
3. **Test-impact graph reflects real symbol↔test relationships, not just definitions.**
   The original bug: it counted where a symbol was *defined*, not where it was
   *referenced by a test*, making it useless for its stated purpose. Check:
   `cid-core/src/semantic_engine/graphs.rs`'s `TestImpactGraph::build`, which must take
   `test_contents: &[(String, String)]` — real file content, not just paths. Run
   `test_impact_and_docs_are_empty_before_the_engine_is_enabled` and
   `test_impact_and_doc_graphs_populate_after_enabling_the_semantic_engine`.
4. **Documentation graph staleness detection isn't a tautology.** The original bug: it
   filtered mentions at write-time in a way that made "is this doc stale" always
   evaluate true regardless of actual doc content. Check: `DocGraph` in `graphs.rs` now
   stores all mentions unconditionally, filtering happens at read-time instead.
5. **Role profiles have real tool-permission enforcement**, not just stored config.
   Check: `cid-core/src/role_profiles/mod.rs` (`RoleProfile`, `ToolPermission`,
   `check_tool_permission`), and specifically that `ExecutionContext.role_profile` in
   `cid-core/src/model/mod.rs` is actually checked at the top of
   `execute_tool_direct_in` — not just present as a field nothing reads. Run the
   `role_profile_enforcement_tests` module in `model/mod.rs`.
6. **Decisions view and Deployment record.** Check: `cid-core/src/decisions/mod.rs`
   (`list_adrs`, `adrs_relevant_to_mission`, `DeploymentLog`/`DeploymentRecord`).
   Critical non-goal to confirm still holds: `DeploymentLog` must only *log* deployments
   (source, timestamp, what/where), never orchestrate one — re-read Part 0's deployment-
   provider exclusion and confirm no code path here calls out to a cloud provider SDK.
   Run `deployment_record_cannot_orchestrate_anything_it_can_only_log`.
7. **`cid-tui` CLI/TUI shell**: chat, mission status, tool-call/plan approval over the
   same WebSocket event stream other surfaces use. Check: `cid-tui/src/main.rs`,
   `api.rs`, `app.rs`, `events.rs`, `ui.rs`. **Known, still-open gap** (confirmed as of
   Phase 5/6's own audits): no diff view. Confirm this is still true, or that it's been
   closed and the roadmap doc updated to match.
8. **45-document `docs/` backfill**, per `CID-Doc-Template.md`'s structure. Spot-check a
   few (`docs/014-Patch-Verification.md`, `docs/015-Test-Impact-Analysis.md`) against the
   actual code they describe rather than trusting the doc template was followed
   correctly everywhere.
