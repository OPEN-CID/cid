# CID — Codebase Review Findings & Remediation Prompt

**Status (2026-07-27): all numbered sections below (§1–§7) are closed.** Also fixed along
the way, found mid-pass rather than listed here originally: a critical bug where real
provider tool calls were parsed off the stream and never executed (see
`CRITICAL-FINDING-tool-calls-not-executed.md`, now RESOLVED), and a three-way duplication
of the system-prompt-building logic that left the strongest (sanitizing) implementation
wired to nothing. See `CLAUDE.md`'s "Current state snapshot" for the up-to-date picture,
including a second AI audit pass's claims verified/refuted against real code, and what
was deliberately *not* attempted (subagent file locking, full network isolation, ONNX
embeddings, CI code-signing) with reasons — don't re-attempt those without reading why
first. This file is kept for the historical record and because its ground rules (§0)
are still the right ones for future passes.

---

**Audience:** an AI coding agent (Sonnet or equivalent) with real file/shell/git access, working in this repository.
**Written:** July 2026, by auditing the actual code — not the checkpoint docs' summaries of it.
**Method:** every finding below names a specific file, line, RPC method, or command. Nothing here is inferred from a doc claiming something works; each was verified by reading the implementation or running it.

---

## 0. How to use this file

Work top-down. Section 1 is security and must land before anything else ships. Sections 2–3 are things that are claimed to work but don't. Sections 4–6 are the gap between "backend exists" and "a developer can actually use it." Section 7 is the next-gen platform work.

**Ground rules for whoever executes this:**

1. **Verify before you fix.** Reproduce each finding first. If a finding is wrong, say so and move on — don't build a fix for a bug that isn't there.
2. **No placeholder code presented as done.** A stub mid-task is fine; a stub in something you report as complete is not. This repository has a documented history of exactly that failure (see `docs/041-Roadmap.md` § Failure Modes — four separate "already built" claims were false on re-verification).
3. **Every fix needs a regression test that fails before the fix and passes after.** Not a test that exercises the happy path — a test that reproduces the specific bug.
4. **Run the full suite after each section**, not just at the end:
   ```bash
   cargo test --workspace --exclude cid --all-features
   cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
   npx tsc --noEmit && npx vitest run
   # E2E needs: cargo run -p cid-core -- --port 5919   AND   npm run dev
   npx playwright test
   ```
   Baseline at time of writing: **407 Rust tests, 2 vitest, 30 Playwright, all green.** If your change drops any of these, fix it before continuing.
5. **Do not commit or push** unless explicitly asked by the human.

---

## 1. CRITICAL — Security. Fix these first.

### 1.1 Sandbox bypass: file tools accept arbitrary absolute paths and are auto-approved

**This is the most serious finding in the review.** It makes the entire Phase 2 sandbox effort moot.

`cid-core/src/model/mod.rs`:

- `execute_tool_direct_in`'s `"read_file"`, `"write_file"`, `"edit_file"`, and `"list_files"` arms take a model-supplied `path` string and pass it **directly** to `tokio::fs::read_to_string(path)` / `tokio::fs::write(path, …)` with **no validation whatsoever** — no canonicalization, no root check, no symlink resolution.
- `autonomy_decision` (~line 2283) returns `AutonomyDecision::PreApproved` for **every tool that isn't `run_terminal`**, with the comment:
  > *"Only `run_terminal` consults the command allow-list — the other tools are file and git operations already confined to the Session's own directory."*

  **That comment is false.** `run_terminal` is genuinely confined (it calls `ctx.confined_root()` → `ctx.resolve_workdir()` → `SandboxConfig`). The file tools are not confined by anything.

**Impact:** In Autonomous mode the agent can read and write any file the Core process can, with no approval prompt. `run_terminal("cat ~/.ssh/id_rsa")` is correctly blocked by the allow-list; `read_file("~/.ssh/id_rsa")` succeeds silently. Write access is worse — `edit_file`/`write_file` can reach `~/.bashrc`, `~/.ssh/authorized_keys`, `.git/hooks/pre-commit` (arbitrary code execution on the next commit), or the CID binary itself.

This is the same vulnerability class as Cursor's **DuneSlide** (CVE-2026-50548 / CVE-2026-50549, both CVSS 9.8), where the sandbox honored an agent-chosen write path, and **CVE-2026-22708**, where — in the researchers' framing — *the allowlist made the attack easier by auto-approving the very commands the attacker needed.* CID reproduces both shapes.

**Required fix:**

1. Add a single path-confinement helper on `ExecutionContext` (e.g. `resolve_confined_path(&self, requested: &str) -> Result<PathBuf>`) that:
   - resolves the path against `ctx.root` when relative;
   - **canonicalizes** it (`std::fs::canonicalize`, or `dunce::canonicalize` on Windows) so `..`, symlinks, junctions, and 8.3 short names all normalize;
   - canonicalizes `ctx.root` too, then verifies the result is a prefix — string comparison on un-canonicalized paths is not sufficient and is itself a common bypass;
   - returns a hard `Err` on escape (do **not** silently clamp to root the way `resolve_workdir` does for `workdir` — a file path that escapes is an attack signal, not a typo to be quietly corrected).
2. Route **every** file tool (`read_file`, `write_file`, `edit_file`, `list_files`, and any git tool taking a path) through it.
3. Fix `autonomy_decision`: file tools must be confined **and** their pre-approval must depend on confinement succeeding. Deny-by-default on any path that fails resolution.
4. Delete the false comment; replace it with what the code actually guarantees.

**Regression tests required** (these must fail against current `main`):
- `read_file` with an absolute path outside the worktree → denied.
- `edit_file` with `../../../etc/passwd` (and the Windows equivalent) → denied.
- A symlink inside the worktree pointing outside it → denied (this is the case a naive `starts_with` check misses).
- `.git/hooks/pre-commit` write → denied.
- A legitimate in-worktree relative path → still works (guard against over-correcting into a broken tool).

### 1.2 No prompt-injection defense on untrusted repository content

CID's entire value proposition involves pointing an agent at a repository, then feeding file contents, `AGENTS.md`, `SKILL.md`, git diffs, and GitHub/GitLab/Jira issue text into a model that holds file-write and shell tools. There is currently **no** boundary marking, provenance tracking, or injection heuristic anywhere in that path.

Per OWASP's 2026 reporting, prompt injection remains the dominant agentic-AI failure in production, and the delivery vector is precisely this: instructions hidden in a README, a dependency, a GitHub issue, or a code diff — live the moment an agent is pointed at an unfamiliar repo.

**Required (minimum viable, not a research project):**
1. Wrap all untrusted content (file contents, diffs, issue/ticket text, MCP tool results) in explicit delimiters with a system-prompt rule that content inside them is **data, never instructions**.
2. Treat `AGENTS.md`/`SKILL.md` from a newly-connected repo as untrusted on first load — surface a one-time "this repo ships agent instructions, review them" card rather than silently loading them into the system prompt. This is a real attack path today: `handle_repo_connect` auto-detects and loads `AGENTS.md` with no human review step.
3. Log every tool call whose arguments were influenced by untrusted content into the History panel with a provenance marker.
4. Document the residual risk honestly in `SECURITY.md` — this class is mitigated, never eliminated.

### 1.3 Governance is enforced at creation but not at spend or merge

Carried forward from `docs/CHECKPOINT-Phase3.md`, still open and verified during this audit:
- `governance.check.merge` exists as a callable RPC; **nothing invokes it** at an actual merge/PR decision point.
- `governance.spend.record` exists and is tested; **nothing in the model-router path calls it** after a real API call, so spend caps can never trip from real usage.

**Fix:** call `check_merge` in the merge/PR path before the operation, and `record_spend` after every provider call, threading real token counts from each provider's response (`usage` on Anthropic/OpenAI, `usageMetadata` on Google). Test that a session exceeding its cap is actually blocked on the *next* call.

---

## 2. Claimed to work, doesn't

### 2.1 MCP stdio tool calls are fabricated

`cid-core/src/mcp/mod.rs`, `call_tool`, the `McpTransportType::Stdio` arm (~line 230) does not talk to the child process at all. It returns:

```rust
"result": format!("Stdio MCP tool call '{}' executed (simulated). …", tool_name),
"simulated": true
```

**stdio is the primary transport for local MCP servers** — it remains fully supported in the 2026-07-28 spec alongside Streamable HTTP. So in practice most real MCP servers a developer would connect are non-functional, while `README.md` advertises a working MCP client and the agent receives a *success-shaped* response containing fabricated content. An agent told "the tool ran successfully" will confidently build on a result that never happened. That is worse than an error.

**Fix:** implement real duplex stdio JSON-RPC framing — a persistent task per server owning the child's stdin/stdout, newline-delimited JSON-RPC, request-ID correlation, timeouts, and child-lifecycle cleanup. Until it's real, it must return a hard error, never a synthetic success.

**While you're in here**, the 2026-07-28 spec also replaces server-initiated `sampling/createMessage` and `elicitation/create` with the **Multi Round-Trip Requests** pattern (SEP-2322): results carry `inputRequests` plus an opaque `requestState` the client echoes back unmodified with `inputResponses`. Roots, Sampling, and Logging are deprecated. `docs/045-Dependency-Audit.md` already commits to a validation pass against the final spec text — do it here.

### 2.2 `npm run lint` fails — the `lint-frontend` CI job is red

There is **no ESLint configuration file anywhere** in the repo (no `.eslintrc*`, no `eslint.config.js`). `package.json` defines `"lint": "eslint . --ext ts,tsx …"` and `.github/workflows/ci.yml` runs `npm run lint` as a required job. ESLint 8 exits non-zero with `couldn't find a configuration file`.

This is the same failure mode already found and fixed twice in this project's history (see `docs/CHECKPOINT-Phase5.md`): CI claiming to enforce a gate that cannot pass.

**Fix:** add a real flat config (`eslint.config.js`) for TypeScript + React, fix whatever it legitimately flags, and confirm the job passes. `--max-warnings 0` is already in the script — keep it.

### 2.3 No LSP integration

`docs/` and the founding brief (Part 11) describe Monaco "with LSP integration for supported languages." There is no LSP client, server management, or protocol code in `cid-core/` at all.

**Fix:** either build it, or correct every doc that claims it. Prefer correcting the docs now and scoping the build separately — an LSP client is a real subsystem, not a patch.

---

## 3. Core-loop production gaps

### 3.1 No context compaction or token budgeting — long Sessions will hard-fail

`cid-core/src/model/mod.rs` (~line 1346) does `self.persistence.list_messages(session_id)?` and passes the **entire** history to `build_anthropic_messages` / `build_openai_messages` / `build_google_contents` on **every turn**. There is no summarization, no truncation, no token accounting, no `/compact` equivalent. The only token constants are output caps (`max_tokens: 4096/8192`).

Tool observations typically consume 70–80% of an agentic session's token budget, which is exactly why compaction is now standard: Claude Code ships automatic compaction plus `/compact` and `/context`. Without it, a CID Session grows until it exceeds the context window and then fails permanently — with cost growing quadratically over the session in the meantime.

**Fix:**
1. Track token usage per turn from each provider's response.
2. At a configurable threshold (default ~70% of the model's window), summarize the oldest non-pinned turns into a compact digest message, preserving the approved plan, the current task, and recent tool results verbatim.
3. Expose it: a `/compact` composer command and a visible context-usage indicator in the thread.
4. Persist the digest so it survives a reload.

**Test:** a Session with an artificially long history must still complete a turn instead of erroring.

### 3.2 No checkpoint / rewind

There is no snapshot, checkpoint, or undo anywhere in Core. Automatic pre-change snapshots with a rewind that can revert conversation, code, or both are now table-stakes in this category.

CID is unusually well-positioned here and should not build this generically: **every Session already runs in a dedicated git worktree.** A checkpoint is a git commit or stash on the Session branch; rewind is a reset. Implement it on that existing primitive rather than inventing a parallel snapshot store.

**Scope:** auto-checkpoint before each Implementer tool batch; `session.checkpoint.list` / `.rewind` RPCs; a rewind affordance in the thread and the diff view.

### 3.3 The Reviewer role has no UI at all

`session.review.run`, `.get`, and `.list` are implemented, tested, and **completely unreachable from any surface** — no component calls them. The Reviewer is one of the three founding roles and Flow 1 step 6; a user cannot invoke it or see its findings.

**Fix:** a `ReviewCard` in the Session thread (mirroring the existing `PlanCard`, which is a good model to copy) — run the Reviewer, render findings by severity with file links, show the verdict, keep the raw output expandable.

---

## 4. Backend far ahead of frontend — 35 orphaned RPC methods

171 RPC methods are registered in `cid-core/src/api/router.rs`. **35 have no caller anywhere in `src/`.** These are built, tested, and documented — but there is no way for a user to reach them. In several cases this makes a shipped feature effectively nonexistent.

Verify the current list yourself before starting (it should shrink as you work):

```bash
comm -23 \
  <(grep -oP '"\K[a-z_]+\.[a-z_.]+(?=" =>)' cid-core/src/api/router.rs | sort -u) \
  <(grep -rhoP '(?:this\.call|api\.call)\("\K[^"]+' src/ | sort -u)
```

Grouped, in priority order:

| Group | Orphaned methods | Why it matters |
|---|---|---|
| **Context Engine toggle** | `context_engine.toggle` | **Highest priority in this section.** The Context Engine is off by default *by design* and this is the only way to turn it on — so today it can never be enabled from the UI. A flagship feature is unreachable. |
| **Reviewer** | `session.review.run/get/list` | See §3.3. |
| **Role profiles** | `role_profile.create/get/list/update/delete/check_permission` | Phase 4 deliverable with real enforcement in the tool-dispatch path; no way to create or assign a profile. |
| **Semantic engine** | `semantic_engine.test_impact.*` (3), `docs.for_symbol`, `docs.stale`, `index_file`, `load_blame` | The test-impact and documentation graphs — a headline Phase 4 feature — are invisible. |
| **Decisions & deployment** | `decisions.list`, `decisions.for_session`, `deployment.record/list/webhook` | Phase 4 deliverables, no surface. |
| **Slack / Teams** | `slack.configure`, `slack.config.get`, `slack.trigger_session`, and the three Teams equivalents | Cannot be configured without hand-crafting RPC calls. |
| **Code analysis** | `code.analyze_file/analyze_directory/search_symbols/get_imports` | Useful standalone; currently only reachable internally. |
| **Misc** | `confidence.history`, `mcp.task.subscribe`, `workspace.get` | Smaller gaps. |

**Approach:** do not build eight new panels. Fold these into surfaces that already exist — Context Engine toggle into the repo/settings area, test-impact and doc-graph into the existing Health panel, Decisions into a thread tab, Slack/Teams into the settings surface alongside Providers. Each one you wire, add an E2E assertion that the UI path actually reaches the RPC — that is the specific check that would have caught the `settings.get` shape bug documented in `docs/CHECKPOINT-Phase5.md`.

---

## 5. Quality and verification gaps

| Gap | Detail | Suggested action |
|---|---|---|
| **Frontend test coverage is 2 tests** | `LeftRail.test.tsx`, `ChatThread.test.tsx` only. Zero tests for `PlanCard`, `DiffViewer`, `ConfidenceCard`, `AutonomyPanel`, `RepoHealthPanel`, `ProvidersPanel`, `McpPanel`, `SkillsPanel`, `AcpPanel`, `EditorPane`, `TerminalPane`. | Component tests for the approval-critical ones first: `PlanCard` (approve/reject/edit-revokes-approval) and `DiffViewer` (per-hunk accept/reject). These gate real code changes. |
| **Agent loop never run against a real model** | No API key was available in the build environment, so every E2E run exercises the simulated-response fallback (`model/mod.rs` ~line 1372), not a real tool-use loop. | Add an opt-in integration test gated on `ANTHROPIC_API_KEY` that runs one real Session end-to-end. Skip cleanly when unset — never fail CI for a missing key. |
| **Tauri desktop never actually launched** | CI runs `cargo check -p cid` only. No `tauri build`, no click-through. See `docs/048-Platform-Verification.md`. | Add a `tauri build` job on at least one OS; do one manual launch pass and record the result. |
| **Mobile never on real hardware** | Web-build emulation only. Push notifications and voice input are untested. | Cannot be closed without devices — keep it honestly documented. |
| **`cid-tui` has no diff view** | CLI-first users must switch surfaces to review a change. | Known and tracked; build if the CLI persona matters. |
| **Embeddings are a hash projection, not a model** | `semantic_engine` uses a deterministic hash-based vector. Hybrid search is BM25 doing the real work. | Wire a real embedding endpoint (local or cloud) or stop calling it semantic search in user-facing copy. |
| **No file watcher → UI refresh** | `notify` is used inside `context_engine` only; git status/diff still require a manual refresh. | Broadcast file-change events over the existing WebSocket and refresh diff/status reactively. |

---

## 6. Correctness follow-ups

- **`git.hunk.apply` reject is whole-file.** `router.rs` (~line 1049) discards the entire file via `git checkout HEAD --` rather than reverse-applying the single hunk; `DiffViewer.tsx` documents this inline. **A user rejecting one hunk silently loses every other change in that file.** Implement real per-hunk reverse patch (`git apply -R` with a constructed single-hunk patch). This is a data-loss bug, not a cosmetic limitation — it deserves higher priority than its current framing suggests.
- **`repo.connect` has no validation.** It accepts any path, including one that is not a git repository, and creates a channel for it. Validate and return an actionable error.
- **`confidence/mod.rs` (~line 707)** contains a self-described heuristic (*"real implementation would be much more sophisticated"*). Fine as a heuristic — make sure the Confidence card never presents it as more certain than it is.

---

## 7. Next-generation platform work

Grounded in `docs/049-Extensibility-And-Sync-Roadmap.md`, which already contains the full analysis — read it before starting any of this, and specifically **do not build a second tool-plugin format.** CID's answer to "how do I extend this" is already MCP servers (capabilities), MCP Apps (custom UI), and `SKILL.md` (procedures) — real multi-vendor standards, which is a genuine advantage over a proprietary plugin API. What's actually missing:

1. **Theming (build this first — highest value per unit of effort).** There is no theme system and no light mode; the UI is hard-coded dark. Implement a JSON token→CSS-variable map with no code execution — trivially sandboxable, immediately gives users the customization VS Code's ecosystem is known for, and is independent of the larger extension question. Ship a light theme with it.
2. **UI contribution points.** Registering a named right-panel tab or left-rail section. Reuse MCP Apps' sandboxed-HTML rendering model rather than injecting arbitrary React — CID has no single privileged UI process (Part 15's "many thin surfaces over one Core"), so a VS Code-style in-process extension host is the wrong shape here regardless.
3. **Same-network device pairing.** Cross-device access **already works today** and is merely undocumented: the mobile shell is the same bundle reading `VITE_CID_CORE_HOST`, and `AccessPolicy::new` already requires a bearer token for any non-loopback bind. All that's missing is a QR-code pairing flow and a safe "bind to LAN" toggle. This is small, real, and shippable — do it before contemplating any relay or hosted sync.
4. **Cross-network sync** stays evidence-gated and off by default (`sync.enabled`), per `docs/049-…md`. Self-hosted CID must remain fully functional with it permanently off.

---

## 8. Suggested execution order

1. **§1.1 sandbox confinement** — nothing else ships before this.
2. **§2.2 ESLint config** — 30 minutes, unblocks a red CI job.
3. **§1.3 governance wiring**, **§6 hunk-reject data loss** — small, high-value correctness fixes.
4. **§2.1 real MCP stdio** — largest single "claimed but false" gap.
5. **§3.1 context compaction** — production blocker for any long Session.
6. **§4 orphaned RPCs**, starting with `context_engine.toggle` and the Reviewer UI.
7. **§3.2 checkpoint/rewind** on the existing worktree primitive.
8. **§1.2 prompt-injection defense** — do it properly rather than quickly; it needs design, not a patch.
9. **§5 test coverage**, **§7 theming**.

## 9. What to report back

For each item: what you changed, the regression test that now covers it, and the honest test count before/after. If a finding above turns out to be wrong, say so plainly — a corrected finding is a good outcome, not a failure. If something is genuinely deferred, name it and say which section it belongs to. Do not report anything as complete that has a stub behind it.

---

## Sources for the external research in this review

- [Critical Cursor Flaws Could Let Prompt Injection Escape Sandbox and Run Commands — The Hacker News](https://thehackernews.com/2026/07/critical-cursor-flaws-could-let-prompt.html)
- [How to Secure Your AI Coding Agent After the Sandbox Escapes](https://www.techjuice.pk/four-major-ai-coding-tools-share-the-same-flaw/)
- [Prompt injection still drives most agentic AI security failures in production — Help Net Security](https://www.helpnetsecurity.com/2026/06/11/owasp-prompt-injection-ai-security-failures/)
- [AI Agent Security in 2026: Tool Poisoning, Prompt Leaking, and MCP Sandbox Escapes — KENSAI](https://gokensai.com/blog/2026-04-06-ai-agent-security-framework-tool-poisoning-prompt-leaking-mcp-sandbox-escapes/)
- [The 2026-07-28 MCP Specification Release Candidate — MCP Blog](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
- [MCP 2026-07-28: From Local Tool to Distributed Protocol — AAIF](https://aaif.io/blog/mcp-2026-07-28-whats-changing-and-how-to-migrate)
- [MCP 2026-07-28 spec: what changed, what breaks — Stacktree](https://stacktr.ee/blog/mcp-2026-spec-changes)
- [Claude Code Guide 2026: 25 Features with Examples — MarkTechPost](https://www.marktechpost.com/2026/06/14/claude-code-guide-2026-25-features-with-examples-demo/)
- [How AI Coding Agents Handle a Full Context Window — wasnotwas](https://wasnotwas.com/writing/context-compaction/)
- [State of CLI Coding Agents, Mid-2026](https://blog.arcbjorn.com/state-of-cli-coding-agents-2026)
