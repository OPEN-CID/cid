# CID — Phase 3 Checkpoint Report

**Scope:** Appendix A Part 22, Phase 3 — "Team-ready."

---

## 1. What was built

### Local accounts, sessions, and roles (ADR 0013)

`cid-core/src/auth/` — Argon2id-hashed local accounts, opaque 48-char session tokens
(12h TTL), and a five-tier role ladder (Viewer < Reviewer < Developer < Admin < Owner).
The first account registered becomes Owner; later ones default to Developer. Failed
logins are rate-limited (5 attempts, 60s lockout) per username. The last Owner cannot be
demoted or deactivated. Changing a password revokes every existing session for that user.
20 unit tests.

RPC: `auth.status`, `.register`, `.login`, `.logout`, `.session`, `.users.list`,
`.user.set_role`, `.user.set_active`, `.user.change_password`.

### Workspace governance (`cid-core/src/governance/`)

Sits above the Phase 1 per-repo Autonomous-mode allow-lists: this decides *who* may turn
Autonomous mode on, *which repos* permit it, and *spend caps*. Every decision
(`PolicyDecision::Allow`/`Deny`) carries a human-readable reason for the audit trail.
Defaults are closed — Autonomous mode is off, no repo is allow-listed, until an Admin
configures it. 13 unit tests.

Enforced at two real decision points, not just exposed as a checkable RPC:
- `mission.create` refuses an Autonomous-mode Mission unless the session's role and the
  target repo both clear the Workspace policy.
- `mission.plan.approve` checks the approver's role against `min_role_for_plan_approval`
  and records their real username as `approved_by`, replacing the free-text string the
  Phase 1 implementation accepted from any caller.

RPC: `governance.policy.get/.set`, `.check.autonomous/.plan_approval/.merge`,
`.spend.check/.record/.summary`.

### GitLab and Bitbucket bridges (`cid-core/src/forges/`)

Parity with the existing GitHub bridge behind one abstraction (`ForgeManager`,
`ForgeKind::GitLab | Bitbucket`): connect a Repo Channel to a project, issue→Mission
trigger, and merge/pull-request create/list/status. GitLab uses `PRIVATE-TOKEN` auth and
`iid` for the visible issue number; Bitbucket uses HTTP basic (`user:app_password`) and
paginates under `values`. Both map to one normalized `ForgeIssue`/`ForgeChangeRequest`
shape. 16 unit tests covering response mapping, URL encoding, and connection validation.

RPC: `forge.connect`, `.config.get`, `.disconnect`, `.issues.list`, `.issue.get`,
`.issue.to_mission`, `.change_request.create/.list/.status`.

### Jira and Linear linkage (`cid-core/src/trackers/`)

Deliberately narrow per Part 1's non-goal: **Mission ↔ ticket linkage**, not a tracker
replacement. Attach a ticket to a Mission, fetch its summary for display, post a progress
comment, open a Mission from a ticket. Cannot create tickets, change status, or manage
sprints. Jira uses REST v3 with Atlassian Document Format flattened to plain text for
Mission context; Linear uses its GraphQL API. 19 unit tests.

RPC: `tracker.token.set`, `.status`, `.issue.get`, `.link`, `.links.list`, `.unlink`,
`.issue.to_mission`, `.comment`.

### Mobile companion app (`src/mobile/MobileApp.tsx`)

Built on the Phase 2 bake-off decision (ADR 0010: Tauri v2 Mobile, same React bundle,
same JSON-RPC contract) — approval/monitoring only, per Part 1's mobile non-goal and Part
19's screen spec: Mission list (blocked-on-approval Missions surface first) → tap into a
Mission → approve/deny/comment on pending tool calls → read-only diff and terminal tabs.
No file tree, no editor, no code written from mobile. Push-style approval alerts via the
Notification API when the tab is backgrounded; voice input via the Web Speech API where
the platform provides it (`useVoiceInput` reports `supported: false` rather than showing
a dead button otherwise). `main.tsx` selects the mobile shell by platform (Tauri
Android/iOS, or a narrow touch viewport) rather than by window size, so a narrow desktop
window still gets the full app; `?mobile=1` forces it for testing.

### Cross-platform test matrix floor (Part 21)

- **Protocol fuzzing** (`tests/protocol_fuzz.rs`, 9 tests, 2 using `proptest`): malformed
  JSON, deeply nested payloads, arbitrary method names, arbitrary params against every
  real method category, hostile MCP server registration and tool-call arguments, hostile
  ACP handoff identifiers, path-traversal-shaped file reads. The invariant throughout:
  Core answers with an orderly error and keeps serving `/health` afterward — it never
  returns a 5xx or drops the connection.
- **Worktree lifecycle property tests** (`tests/worktree_property.rs`, 11 tests, 3 using
  `proptest`): creation is all-or-nothing (never a half-created directory), removal is
  idempotent, duplicate creation cannot corrupt an existing worktree, sibling worktrees
  survive each other's removal, the parent repo survives worktree churn and stays
  readable, and worktrees are verifiably created inside the managed root.
- **Performance budgets against Part 17** (`tests/performance_budget.rs`, 5 tests, real
  measurements, not estimates): cold start to first `/health` response (measured 12.5ms
  against a <2s budget), `Core::new_in_memory` construction (<500ms budget), `git status`
  on a small repo (<500ms budget), a 200-file repository scan (57.7ms), 100 concurrent RPC
  calls (26ms, 100/100 succeeded). All comfortably clear budget — expected, since these
  run against an in-memory DB and no disk I/O; the numbers are a regression floor, not
  proof the shipped desktop app hits the same figures under real disk-backed load.

### Documentation

- [ADR 0013](adr/0013-local-auth-model.md) — the minimal local-auth decision and what it
  does and doesn't protect.
- `SECURITY.md` extended with the account model's limits (no MFA, no password reset, no
  email verification) alongside the existing sandbox and access-control sections.

---

## 2. What was deferred or stubbed

| Item | Why | Phase |
|---|---|---|
| SSO / OIDC | ADR 0013's explicit non-goal; the local schema is shaped so an external identity provider can populate it later without redesign | Phase 4+, on real demand |
| Password reset flow | No email infrastructure exists; an Admin can reset another user's password as the interim path | Phase 4+ |
| GitLab/Bitbucket self-hosted CI status checks | Only issue/MR data is read; pipeline status is not surfaced | Not scoped for Phase 3 |
| Jira sprint/board data | Explicitly out of scope — Part 1's non-goal | Never (by design) |
| Full mobile push notifications (APNs/FCM) | Uses the in-browser/in-webview Notification API, not a real push service requiring a backend relay | Phase 4+, if demand appears |
| A physical-device pass for the mobile shell | Not available in this environment; tested via responsive/touch-emulation and the `?mobile=1` override | Noted as a real gap below |

---

## 3. Known issues

- **The mobile shell has not been run on real iOS/Android hardware or in the Tauri mobile
  runtime.** It has been exercised as a web build with touch/narrow-viewport emulation.
  Part 3's own guidance says a real-device pass matters most for push notifications and
  voice input specifically — both are real risk areas until that pass happens.
- **Governance is enforced only at Mission creation and plan approval,** not yet at merge
  time or mid-Mission autonomy switches (Flow 2: switching a running Mission to Autonomous
  mid-run). `governance.check.merge` exists as an RPC but nothing calls it automatically
  yet.
- **Spend tracking has no automatic recording.** `governance.spend.record` exists and is
  tested, but nothing in the model-router path calls it after an actual API call — so
  spend caps are enforceable but not yet self-populating from real usage.
- **Forge and tracker credentials share the OS keyring namespacing scheme** as GitHub's
  existing bridge; verified for correctness in tests but not validated against a live
  GitLab/Bitbucket/Jira/Linear account in this pass (no network access in this
  environment) — response-shape parsing is tested against realistic fixture JSON, not a
  live API round-trip.
- **The performance numbers are a floor, not a ceiling proof.** They confirm Core's own
  logic doesn't regress by an order of magnitude; they say nothing about disk-backed
  SQLite, a real multi-thousand-file repository, or the Tauri desktop shell's own startup
  cost.

---

## 4. Test status

```
cargo test -p cid-core
  unit:                233 passed
  integration:          47 passed
  protocol_fuzz:          9 passed
  performance_budget:     5 passed
  worktree_property:     11 passed
  ------------------------------
  total:                305 passed, 0 failed

npm run test:    2 passed
npx tsc --noEmit: clean
npm run build:   clean
```

This is a genuine increase in coverage over Phase 2's bar (Part 21's Phase 3+ floor:
cross-platform matrix, load/benchmark tests, MCP/ACP fuzzing, worktree property tests) —
all four items in that list now have real, passing tests, not placeholders.

Not covered: a live cross-provider round-trip against real GitLab/Bitbucket/Jira/Linear
accounts (no network egress in this environment), and the mobile real-device pass noted
above.

---

## 5. Proposed go/no-go for Phase 4+

**GO for Phase 4**, on the terms Phase 4's own brief sets: is there real profiling
evidence Monaco/CodeMirror is a bottleneck? **No** — nothing in Phases 0–3 measured editor
rendering as a cost center; the performance work this phase did was at the Core/API layer,
not the editor. Is there real demand for a hosted "CID Cloud"? **No evidence gathered
either way** — this remains a legitimate "not yet," not a decision to build one.

**One process note, not a scope item:** partway through this phase, a background process
(consistent with an OpenCode/OpenRouter free-model delegation hook) silently rewrote
`cid-core/src/subagent/mod.rs` — replacing 444 working, tested lines with ~96 lines that
imported modules (`crate::error`, `crate::tasks`, `crate::skills::SkillName`) not present
anywhere in this codebase, and would not have compiled. It also left behind a fabricated
`.claude/CLAUDE.md` referencing a nonexistent "Gottlieb list." Both were caught by the
routine `cargo test` pass immediately after, restored from git, and confirmed not to have
touched any other file. Recorded here because it's a real repository event, and because
it's the concrete argument for why free-tier model delegation should stay scoped to
small, isolated, easily-diffed files rather than pointed at existing, load-bearing
modules.
