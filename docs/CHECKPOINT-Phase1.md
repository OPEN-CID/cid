# CID — Phase 1 Checkpoint Report

**Scope:** Appendix A Part 22, Phase 1 — "Extensible and remote-capable."

This report was written after auditing the repository against the Phase 1 prompt. Most
Phase 1 backend work had been built in an earlier session but was never checkpointed;
this pass found and closed the gaps, so the report covers both what already existed and
what had to be added.

---

## 1. What was built

### Already present and verified working

| Deliverable | Where | Verified by |
|---|---|---|
| Multi-provider routing (Anthropic, OpenAI, Google) + generic OpenAI-compatible slot | `cid-core/src/model/mod.rs` | `model_list_exposes_all_phase1_providers` |
| Per-role provider/model selection (Planner / Implementer / Reviewer) | `resolve_for_role`, settings columns | unit tests + `ProvidersPanel` UI |
| Local runtime detection (Ollama, LM Studio, `llama.cpp --server`) | `cid-core/src/local_models/mod.rs` | `local_runtime_list_returns_known_runtimes` |
| Structural Context Engine (Tree-sitter), off by default per repo | `cid-core/src/context_engine/mod.rs` | `context_engine_is_off_by_default_and_toggles_per_repo` |
| GitHub bridge (issue → Mission, PR create/status sync) | `cid-core/src/github/mod.rs` | module unit tests |
| Autonomous mode + command allow-lists | `cid-core/src/autonomy/mod.rs` | `autonomy_denies_*` integration tests |
| Headless Core server mode | `cid-core/src/main.rs`, `Core::serve` | every integration test runs against it |
| Multi-file `SKILL.md` bundle discovery and precedence resolution | `cid-core/src/skills/mod.rs` | `skills_resolve_*`, `skills_bundles_list_*` |

### Gaps found and closed in this pass

1. **ACP host had no API surface.** `AcpHostManager` was fully implemented (editor probing,
   handoff lifecycle, take-back) but not reachable — zero `acp.*` RPC methods existed, so the
   feature could not be used from any shell. Added `acp.editors.list`, `acp.handoff`,
   `acp.take_back`, `acp.handoffs.list`, `acp.handoff.get`, `acp.handoff.remove`, plus an
   `AcpPanel` UI with detected-editor list, hand-off, and take-back.

2. **Planner and Reviewer were never invoked.** This is the gap the Phase 1 prompt's Part 0
   correction anticipated, and it was real: the three roles existed only as *model-routing
   configs*. Nothing produced a plan, nothing gated on approval, and nothing ran a review.
   Flow 1 steps 3 and 6 — the literal Phase 0 golden path — were not implemented. Per the
   prompt's instruction, this was fixed as a Phase 0 gap before continuing, and is reported
   honestly here rather than folded into Phase 1's own scope.

   Added `cid-core/src/roles/mod.rs`:
   - **Planner** produces an editable Requirements/Approach/Steps plan, persisted in a new
     `mission_plans` table, generated automatically on Mission creation.
   - **Plan-approval gate**: outside Manual autonomy, `mission.send_message` refuses to start
     the Implementer until a plan exists *and* a human has approved it. Editing an approved
     plan returns it to draft, since the approval applied to the previous text.
   - **Reviewer** runs over the Mission's diff (read from the Mission's own worktree), parses
     `severity | file | description` findings, and records a verdict in `mission_reviews`.
     Runs automatically when a Mission is closed.
   - RPC: `mission.plan.generate|get|update|approve|reject`, `mission.review.run|get|list`.
   - UI: `PlanCard`, rendered inline in the Mission thread per Part 5.

3. **`ModelManager` had no non-streaming completion path.** Planner and Reviewer produce a
   document rather than driving a tool loop. Added `complete_text`, which returns
   `Ok(None)` when a role has no usable credentials so callers degrade to a documented
   placeholder instead of failing.

4. **Skill discovery skipped its own convention directory.** `find_skill_md_files` excluded
   every dot-directory, which meant `.cid/skills/` — and `.claude/skills/`, the convention
   other SKILL.md-aware tools use — were invisible. Now skips hidden directories *except*
   the known agent-config ones.

5. **Frontend was still Phase 0-shaped.** The Autonomy selector was hard-disabled with a
   "Phase 0 ships Co-Pilot only" note, the status strip hardcoded "Claude 3.5 Sonnet", and
   the Metrics tab said Phase 1 hadn't happened. Autonomy levels are now selectable
   including Autonomous, the status strip reads real settings, and the API client covers
   the Phase 1 and Phase 2 RPC surfaces.

### How to run it

```powershell
# Headless Core (this is Phase 1's headless server mode, now a supported surface)
cargo run -p cid-core -- --port 5919 --db C:\Temp\cid.db
# Health: http://127.0.0.1:5919/health

# Browser shell
npm install
npm run dev          # http://localhost:1420

# Desktop shell
npm run tauri:dev
```

Exercising the new surfaces:

```powershell
# ACP: list detected editors
curl -X POST http://127.0.0.1:5919/api/rpc -H "Content-Type: application/json" `
  -d '{"jsonrpc":"2.0","id":"1","method":"acp.editors.list","params":{}}'

# Plan gate: sending a message before approval returns blocked:true
curl -X POST http://127.0.0.1:5919/api/rpc -H "Content-Type: application/json" `
  -d '{"jsonrpc":"2.0","id":"2","method":"mission.send_message","params":{"mission_id":"<id>","content":"go"}}'
```

---

## 2. What was deferred or stubbed

| Item | Phase |
|---|---|
| Hardware-gated model filtering (grey out models the machine can't run) | Phase 2 — Part 6 puts it there explicitly; Phase 1 is detection and listing only |
| Sandboxing for Autonomous mode | Phase 2 — Part 14; the allow-list is the only guardrail in Phase 1 |
| Web and mobile shells, Slack/Teams bridges | Phase 2–3 |
| Semantic/embedding context engine | Phase 2 |
| Sparse checkout for large monorepos | Phase 1+ per Part 4; not built, not required by Part 22's Phase 1 list |

---

## 3. Known issues

- **The Reviewer's diff input is a serialized `GitDiff` struct, not a raw unified diff.**
  It is accurate but more verbose than a `git diff` would be, which costs review tokens.
  Passing an explicit `diff` parameter to `mission.review.run` overrides it.
- **Finding parsing is strict by design.** Reviewer output lines that do not match
  `severity | file | description` are dropped rather than guessed at. A model that ignores
  the format produces zero findings and a `clean` verdict, which understates risk. The raw
  output is always persisted so nothing is lost.
- **The Autonomous-mode allow-list runs unsandboxed** — this is Phase 1's stated boundary,
  not a defect, but it means an allow-listed command pattern is the *only* thing between an
  approved plan and real execution. Verified: an unconfigured scope denies by default rather
  than defaulting to allow (`autonomy_denies_by_default_when_no_allowlist_is_configured`).
- **CORS is still `Any`.** Fine for the localhost dev loop; tightened as part of the Phase 2
  web-shell access-control work.

---

## 4. Test status

```
cargo test -p cid-core
  unit:        137 passed
  integration:  28 passed   (cid-core/tests/api_integration.rs)
npm run test:    2 passed
npx tsc --noEmit: clean
```

The 28 integration tests are new in this pass and cover the Phase 1 bar from Part 21:
the headless Core API surface over real HTTP, Skills resolution precedence, the ACP
surface, the autonomy allow-list, and the plan-approval gate.

Not yet covered: snapshot tests for Diff and History rendering (Part 21 lists these for
Phase 1). Diff and History are exercised by the Playwright E2E specs but have no snapshot
assertions.

---

## 5. Proposed go/no-go for Phase 2

**GO.** Phase 1's scope is complete and the two structural gaps found — an unreachable ACP
host and missing Planner/Reviewer lifecycle — are closed and tested rather than papered
over. The Phase 0 golden path (Flow 1) now actually runs end to end including its plan
step and review pass, which was not previously true.
