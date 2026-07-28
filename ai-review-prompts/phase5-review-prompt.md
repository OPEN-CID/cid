# Phase 5 review — dependency audit, contributor experience, vibe mode

Source of truth: `docs/CHECKPOINT-Phase5.md`. This phase's own verification pass found a
real secrets-exposure bug (plaintext API keys in an RPC response), a broken
partial-update path, and a rotted E2E suite that had been silently passing without
checking anything. Verify all three fixes hold — they're exactly the kind of thing that
regresses silently in a later refactor.

## Claims to verify

1. **`settings.get` never returns a plaintext secret, anywhere in its response.** Check:
   `handle_settings_get` in `cid-core/src/api/router.rs` — confirm there is **no**
   `full_settings` field or any other field carrying an unredacted API key; the response
   should be a single flat object with `has_*_key` booleans and redacted secret fields.
   Run `settings_never_return_a_full_api_key` in `api_integration.rs` — read the test
   itself, not just its pass/fail: it should round-trip a real fake secret through
   `settings.update` and then assert it's absent anywhere in the `settings.get` response
   body (`!raw.contains(...)` on the full serialized JSON), not just check one assumed
   field path. A version of this test that only checks `result["anthropic_api_key"]` at
   the top level while the real secret lives elsewhere in the response would pass
   vacuously without proving anything — that was the original bug in this exact test.
2. **`settings.update` supports partial updates.** Check: `handle_settings_update` merges
   the incoming JSON onto the persisted settings before deserializing into the `Settings`
   struct, rather than requiring every field. Try `settings.update` with just `{"theme":
   "dark"}` against a running Core and confirm it succeeds.
3. **The E2E suite (`tests/e2e/*.spec.ts`) genuinely passes, not vacuously.** Run
   `npx playwright install && npm run test:e2e` against a real `cargo run -p cid-core --
   --port 5919` and `npm run dev`. All 30 tests across `flow1.spec.ts`,
   `flow2-analysis.spec.ts`, `flow3-models-mcp.spec.ts`, and `health-check.spec.ts`
   should pass. If any RPC call in these tests uses `.catch(() => null)` followed by an
   `if (data) { ... }` check, verify that pattern isn't hiding a real RPC-level error
   response (`{"error": ...}` with HTTP 200) as a silent skip — that exact pattern caused
   several tests to "pass" without checking anything in an earlier version of this suite.
4. **CI actually runs the full test surface**, not just `cid-core --lib`. Check:
   `.github/workflows/ci.yml`'s `test-rust-linux`/`-windows`/`-macos` jobs run
   `cargo test --workspace --exclude cid --all-features`, not a narrower `-p cid-core
   --lib` that would silently exclude `cid-core/tests/*.rs` and all of `cid-tui`.
5. **Vibe-coding Mission preset**: `vibe: true` on `mission.create` produces an
   already-`Approved` plan with `approved_by: "vibe-preset"`, so the Implementer is
   unblocked immediately — but tool-call approval (Co-Pilot) and the diff viewer are
   unaffected. Check: `RoleRunner::generate_vibe_plan` in `cid-core/src/roles/mod.rs`.
   Run `vibe_preset_mission_starts_with_an_already_approved_plan`,
   `vibe_preset_does_not_bypass_tool_call_approval`,
   `non_vibe_mission_still_uses_the_full_planner` in `api_integration.rs`, and confirm
   the frontend actually exposes this (a checkbox in `MissionCreationModal` in
   `src/App.tsx`, not just a backend flag nothing in the UI sets).
6. **Dependency audit is real and dated**, not a repeat of the original Part 18 table.
   Check: `docs/045-Dependency-Audit.md` cites specific, checkable findings (e.g., the
   MCP 2026-07-28 spec finalization date, gitoxide's push-support maturity) rather than
   generic "still fine" statements. A later addition documents a real `dompurify`
   security advisory found via `npm audit` during Release validation, tracked as an
   upstream (monaco-editor) issue rather than silently ignored — confirm the
   `dependency-audit` CI job in `ci.yml` still runs `npm audit` and `cargo audit`.
7. **Persona-coverage audit** named a real, still-open gap (cid-tui's missing diff view)
   rather than declaring every persona served. Check `docs/CHECKPOINT-Phase5.md`'s
   persona table and confirm the CLI-first row's caveat is still accurate.
