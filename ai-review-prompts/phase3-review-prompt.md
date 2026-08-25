# Phase 3 review — multi-user, governance, mobile, forges/trackers

Source of truth: `docs/CHECKPOINT-Phase3.md`.

## Claims to verify

1. **Local multi-user auth**: Argon2id password hashing, role hierarchy
   Viewer<Reviewer<Developer<Admin<Owner, rate-limited login. Check:
   `cid-core/src/auth/mod.rs` (`AuthManager`, `Throttle`), run
   `the_first_registration_needs_no_session_and_yields_an_owner`,
   `repeated_failures_lock_the_account_briefly`,
   `changing_a_password_revokes_existing_sessions`,
   `an_expired_or_bogus_session_is_refused_with_a_clear_message`,
   `login_returns_a_session_that_resolves_and_can_be_revoked`,
   `later_registrations_require_an_admin_session`,
   `listing_users_requires_an_admin_session`,
   `a_viewer_cannot_approve_a_plan` in `api_integration.rs`.
2. **Workspace governance policy**: who can enable Autonomous mode, on which repos, spend
   caps. Check: `cid-core/src/governance/mod.rs` (`GovernanceManager`,
   `can_enable_autonomous`/`can_approve_plan`/`can_merge`/`check_spend`/`record_spend`).
   Run `governance_policy_defaults_to_autonomous_disabled`,
   `creating_an_autonomous_session_is_refused_by_default_policy`,
   `an_autonomous_session_is_allowed_once_policy_permits_the_repo`,
   `only_an_admin_can_change_governance_policy`,
   `spend_caps_are_enforced_before_the_spend`,
   `plan_approval_records_the_approving_user`.
   **Known open gap, confirm still open or find it's been closed**: per the checkpoint,
   `governance.check.merge` and `governance.spend.record` exist as RPC methods but
   nothing calls them automatically at the real merge/spend decision points (grep for
   call sites in `router.rs` and `model/mod.rs` outside the RPC dispatch table itself).
3. **GitLab/Bitbucket bridges** at parity with the GitHub bridge. Check:
   `cid-core/src/forges/mod.rs` (`ForgeManager`, `ForgeKind`, normalized `ForgeIssue`/
   `ForgeChangeRequest`).
4. **Jira/Linear linkage** (not a project-tracker replacement — Part 1's non-goal).
   Check: `cid-core/src/trackers/mod.rs` (`TrackerManager`).
   **Known, deliberate gap**: response-shape parsing is tested against realistic fixture
   JSON, not a live API round-trip (no network access in the original build
   environment) — confirm this is still accurately described wherever it's documented,
   and if a live-account validation pass has since happened, that the docs reflect it.
5. **Mobile companion shell**: approval/monitoring, not full editing. Check:
   `src/mobile/MobileApp.tsx`, `src/main.tsx`'s `isMobileShell()`.
   **Known, deliberate gap, likely still true**: never run on real iOS/Android hardware
   or the Tauri mobile runtime — only exercised as a web build with touch/narrow-viewport
   emulation. Confirm via `docs/048-Platform-Verification.md` whether this has changed;
   if it hasn't, a reviewing AI cannot close this gap either (it requires physical
   devices), but should confirm the docs still say so honestly rather than having
   quietly dropped the caveat.
6. **Cross-platform test-matrix floor**: protocol fuzzing, worktree property tests,
   performance budgets. Check: `cid-core/tests/protocol_fuzz.rs` (9 tests),
   `cid-core/tests/worktree_property.rs` (11 tests), `cid-core/tests/performance_budget.rs`
   (5 tests, with real measured numbers — re-run and confirm the numbers in
   `docs/CHECKPOINT-Phase3.md` are in the same order of magnitude, not that they must
   match exactly, since hardware varies).
