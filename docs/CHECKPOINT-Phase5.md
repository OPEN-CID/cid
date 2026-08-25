# Checkpoint — Phase 5

## 1. What was built

**Dependency audit** — `docs/045-Dependency-Audit.md`, researched against the live web as of
2026-07-26, not re-stated from memory. Confirmed `git2-rs` (writes) / `gix` (opportunistic hot
reads), Tauri v2, `ratatui`, and the rest of Part 18's stack still hold. Deliberately deferred
major version bumps (`rusqlite` 0.32→0.40, `tantivy` 0.22→0.26, `axum` 0.7→0.8) — no concrete
problem found, and the prompt's own instruction is "don't churn dependencies for novelty."

**Contributor experience:**
- `CONTRIBUTING.md` rewritten — every command in it was actually run this session, not assumed.
- `.devcontainer/devcontainer.json` (ADR 0016) — Rust + Node, scoped to the browser+Core dev loop
  only (not Tauri, which needs a native display).
- `.github/CODEOWNERS`, `.github/pull_request_template.md` — added.
- **Closed a real CI gap**: `test-rust-*` jobs ran `cargo test -p cid-core --lib --all-features`
  only, silently excluding `cid-core/tests/*.rs` (81 tests: integration, fuzz, property,
  performance) and all of `cid-tui`. Changed to
  `cargo test --workspace --exclude cid --all-features` in all three jobs; `lint-rust`'s clippy
  step extended to `-p cid-core -p cid-tui`. Verified locally against the exact command CI runs.

**Vibe-coding Session preset** — `vibe: bool` on `session.create`
(`cid-core/src/api/types.rs`). When set, `RoleRunner::generate_vibe_plan` writes a minimal
one-line plan and immediately marks it `Approved` (`approved_by: "vibe-preset"`), so the
Implementer is unblocked the moment the Session is created — no separate plan-approval round
trip. This shortens *planning* only: Co-Pilot's per-tool-call approval, the diff viewer, and
History are untouched, and the Session still runs at whatever autonomy level was requested.
Wired into the frontend (`SessionCreationModal` in `src/App.tsx`) as a checkbox with an inline
explanation. 2 unit tests (`roles::tests::vibe_plan_*`) + 3 integration tests
(`vibe_preset_session_starts_with_an_already_approved_plan`,
`vibe_preset_does_not_bypass_tool_call_approval`, `non_vibe_session_still_uses_the_full_planner`).

**Persona-coverage audit** (Part 34's explicit instruction: confirm manual editing, Co-Pilot
review, full Autonomous, CLI-first, GUI/editor-first, and diff-review-first are genuinely served):

| Persona | Genuinely served? | Evidence |
|---|---|---|
| Manual editing | Yes | Monaco/CodeMirror editor panes, Manual autonomy level does no autonomous tool calls |
| Co-Pilot review | Yes | Per-tool-call approval in `model/mod.rs`'s execution loop; approval cards render in desktop/web/mobile; cid-tui approves over the same WS event stream |
| Full Autonomous | Yes | Governance-gated (`governance/mod.rs`: who can enable it, spend caps, command allow-list); human reviews the final diff, not each step |
| CLI-first | **Partially** | cid-tui covers chat, session status, and approvals over a real Core connection — but has no diff view (see below); a CLI-only user must switch surfaces to review a diff |
| GUI/editor-first | Yes | Monaco full-file editing, ACP pop-out to Zed/JetBrains (`acp/mod.rs`) for deeper IDE power |
| Diff-review-first | Yes | `DiffViewer.tsx` does real per-hunk accept/reject over `git.hunk.apply`, not just a read-only view (hunk-reject is whole-file `git checkout HEAD --` pending true `git apply -R` per-hunk reversal in Phase 1+, documented inline) |

**Real bugs found and fixed during this phase's validation pass** (found by actually running the
E2E suite against a live Core, per the operating rule that a stub or an untested claim isn't
"done" — not by inspection):

- **`settings.get` leaked plaintext API keys.** The handler returned a `full_settings` field
  containing every secret unredacted, alongside the redacted `settings` field — dead weight the
  frontend never read, and a real secrets-exposure bug now that Phase 3 added multi-user sessions
  (any authenticated Viewer could have called `settings.get` and read the Owner's provider keys).
  Removed `full_settings` entirely; the response is now a flat, fully-redacted object. The existing
  regression test (`settings_never_return_a_full_api_key`) was itself tautological — it asserted
  against a top-level `anthropic_api_key` field that never existed in the actual response shape,
  so it passed vacuously without ever exercising the redaction path. Rewritten to round-trip a real
  secret through `settings.update` → `settings.get` and assert it's absent anywhere in the
  response body, not just at one assumed key path.
- **`settings.update` couldn't do partial updates.** It deserialized the request body directly into
  the full `Settings` struct, which has several non-optional fields (`theme`, `anthropic_model`) —
  so any caller sending less than the entire settings object (e.g. `{"theme": "dark"}`) got a
  `missing field` error. Only the current frontend's exact "always send the merged full object"
  pattern happened to avoid this. Fixed by merging the incoming JSON onto the persisted settings
  before deserializing, so any subset of fields can be updated safely.
- **The E2E suite itself had rotted**: `__dirname`/`__filename` (CommonJS globals) used in a
  `"type": "module"` project — every test depending on them threw before making its RPC call,
  which cascaded into ~10 downstream failures. Fixed via `fileURLToPath(import.meta.url)`.
  `mcp.server.remove` was called with `server_id` in both E2E files, but the real API (and the
  real frontend) uses `id` — fixed the tests to match the real, working contract rather than
  changing the contract. `pty.list` was called with no `session_id` (a required param) and
  `git.status` was pointed at a non-repo directory — both errored at the RPC level in a way the
  tests' `.catch(() => null)` didn't detect, so they silently "passed" without checking anything;
  fixed to exercise the real success path.
- **Net effect**: the full Playwright suite (`tests/e2e/*.spec.ts`, 30 tests across 4 files) went
  from silently-passing-while-broken to genuinely green, run against a real `cid-core --release`
  and a real `vite` dev server, not mocked.

## 2. What was deferred or stubbed, and which phase it belongs to

- **cid-tui diff view** — genuinely missing, not stubbed. Belongs with Phase 6+ CLI-surface work
  if the CLI-first persona turns out to matter enough in practice to invest further; flagged here
  per the prompt's explicit "when in doubt, flag it, don't add it unilaterally" instruction rather
  than expanded mid-audit.
- Deployment-provider integrations, native rendering engine, enterprise/air-gapped hardening,
  hosted "CID Cloud" — unchanged non-goals (Part 0, Part 1).

## 3. Known issues

- **The Settings/Providers panel had never actually been exercised end-to-end before this pass.**
  The shape mismatch above (`full_settings`/`settings` wrapper vs. the flat object the frontend
  code assumed) meant `ProvidersPanel.tsx` could not have correctly loaded or saved a provider API
  key in the shipped state — it would always show blank fields and fail to save. This shipped
  silently because no test (backend or E2E) ever drove the real RPC shape through the real
  frontend code path; the backend's own tests only checked the handler's return value in
  isolation. Fixed now (see above); this is the clearest single argument in this phase for keeping
  the E2E suite genuinely wired into CI rather than treating it as optional.
- The local dev SQLite database (`%APPDATA%/cid/cid.db`) accumulated cross-run drift during this
  session's repeated manual Core restarts (stale `repo_channels` rows pointing at a workspace ID
  from an earlier schema state), which surfaced as a `FOREIGN KEY constraint failed` on
  `repo.connect` until the dev DB was reset. This is expected drift from ad hoc manual testing on
  a single persistent dev machine, not a reproducible product bug — confirmed by a clean run
  against a freshly-seeded database passing all 30 E2E tests. Worth noting for `CONTRIBUTING.md`:
  a contributor who hits unexplained FK errors during local development should delete their local
  `cid.db` and let it reseed, rather than assume a code bug.

## 4. Test status

Honest, as of this checkpoint:

- `cargo test --workspace --exclude cid --all-features`: **391 passed, 0 failed** (304 `cid-core`
  lib + 59 `api_integration` + 5 `performance_budget` + 9 `protocol_fuzz` + 11
  `worktree_property` + 3 `cid-tui`).
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `npx vitest run` (frontend unit tests): **2 passed, 0 failed** — real but thin coverage
  (`LeftRail`, `ChatThread` only). Not extended this phase; noted as a real gap rather than
  padded with low-value tests to make a number look better.
- `npx playwright test` (E2E, against a real `cid-core --release` + real `vite` dev server):
  **30 passed, 0 failed**, after the fixes described above.
- `npx tsc --noEmit`: clean.

## 5. Proposed go/no-go for Phase 6+

**Not yet**, unchanged from Phase 3/4's framing. Nothing in this phase's audit turned up new
evidence for the native editor, enterprise/air-gapped hardening, or a hosted CID Cloud — the one
concrete signal (cid-tui's missing diff view) is a scoped extension of an existing surface, not a
case for the Phase 6+ bucket. The more material finding this phase is process, not roadmap: the
Settings panel bug survived because no test drove the real frontend↔backend contract end-to-end —
worth treating "does the E2E suite actually run in CI" as a harder gate before adding new surface
area, more than it was treated as one through Phase 0–4.
