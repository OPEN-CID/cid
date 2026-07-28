# Phase 6 review — Repository Health, crate layout, observability

Source of truth: `docs/CHECKPOINT-Phase6.md` and `docs/047-Repository-Health-Observability.md`.
Note: `CID-Phase6-Build-Prompt.md` never existed as a file in this project's history —
this phase was built directly from `CID-Roadmap-Index.md`'s one-line description of it.
If a reviewer finds a copy of a "Phase 6 build prompt" file somewhere, treat it as
someone else's later addition, not the original spec this phase was built against.

## Claims to verify

1. **`repo_health.scan` reports real numbers for a real repo**, and does not miscount a
   test fixture *string* as a real test. This exact bug was found and fixed by running
   the tool against this actual repository, not just its own unit-test fixtures. Check:
   `cid-core/src/repo_health/mod.rs`'s `mask_non_code` (blanks string/comment interiors
   before pattern matching) and `extract_test_bodies` (takes both raw and masked
   content). Run
   `does_not_mistake_test_attributes_inside_string_literals_for_real_tests` — this is the
   regression test for the exact false positive found. Also run `repo_health.scan`
   against this repo's own root directory via RPC and sanity-check the numbers look
   plausible (hundreds of functions/tests, not zero, not obviously inflated).
2. **The "tests" number is an honest presence signal, not a fake coverage percentage.**
   Check that `RepoHealthPanel.tsx` (frontend) explicitly says this is not instrumented
   coverage — a UI that silently presented this number as "% coverage" would be a real,
   user-facing honesty regression even if the backend computation itself is correct.
3. **`/metrics` returns valid Prometheus text and reflects real RPC activity.** Check:
   `cid-core/src/observability/mod.rs`'s `Metrics`, the `/metrics` route in
   `router.rs`'s `create_router`. Call a few RPC methods against a running Core, then
   `curl http://127.0.0.1:5919/metrics` and confirm `cid_rpc_requests_total` and
   `cid_rpc_requests_by_method_total{method="..."}` reflect the calls actually made.
4. **Crash log never contains raw file content, and redacts secrets in captured panic
   messages.** Check: `CrashLog`/`CrashReport` in `observability/mod.rs` — `CrashReport`
   should have exactly five fields (`id`, `timestamp`, `message`, `location`,
   `thread_name`), nothing that could hold a file's contents. Run
   `crash_report_has_no_field_that_could_hold_file_contents` (a structural check on the
   serialized field set) and `captured_panic_messages_are_secret_redacted` (a behavioral
   check that triggers a real panic with a fake secret in the message and confirms it's
   redacted in the captured report).
5. **The Autonomy allow-list panel is real and wired to the existing backend**, not a new
   backend built to match a new UI. Check: `src/components/autonomy/AutonomyPanel.tsx`
   calls `api.autonomy.allowlistGet`/`allowlistSet`/`allowlistDefault` — RPC methods that
   existed since an earlier phase but had zero frontend surface until this phase. Confirm
   toggling a command pattern between "auto-run" and "ask first" in the UI actually
   changes what `autonomy.command.check` returns for that pattern.
6. **Crate-layout doc matches the actual workspace.** Check: `docs/046-Crate-Layout.md`'s
   table against the root `Cargo.toml`'s `[workspace] members` — if a new crate has been
   added since this was written and isn't in the table, that's a real, immediate finding
   (the doc's own "AI Coding Rules" section says exactly this should never happen).
