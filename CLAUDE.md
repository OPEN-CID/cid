# CLAUDE.md

Guidance for Claude Code when working in this repository. This file is a living
document — keep it accurate as the project moves; don't let it fossilize into a
transcript of one session.

## Background Task Delegation Rule

For routine, repetitive, or simple coding tasks (writing tests, docstrings, boilerplate),
DO NOT consume main context tokens.

Delegate to OpenCode using OpenRouter's free router:

```
opencode run --auto --model openrouter/openrouter/free "<task description>"
```

- Run this via the Bash tool as a background worker.
- Do not load the raw files the task touches into your own context — let OpenCode
  write/edit them directly, then verify the result yourself (build/tests/read the diff)
  before reporting it as done.
- If the free router model struggles with a task (unclear result, tool-call failures,
  low-quality output), fall back to a specific free-tier model, e.g.:
  `opencode run --auto --model openrouter/meta-llama/llama-3.3-70b-instruct:free "<task>"`
- Reserve this for genuinely simple, well-scoped work. Anything requiring judgment about
  architecture, security, or cross-cutting design should stay in the main context.

---

## What CID is

A chat-native, multi-agent software engineering platform — see `README.md` for the full
pitch. In one line: Workspace → Repo Channel → Mission Thread (Slack-shaped), Missions
run in isolated git worktrees with a Planner → Implementer → Reviewer loop, three
autonomy levels (Manual / Co-Pilot / Autonomous), inline diff review, a real terminal,
and MCP tool access. One Rust/Tokio daemon (`cid-core`) exposes everything over
JSON-RPC 2.0 (HTTP + WebSocket); desktop (Tauri v2), web, and TUI are thin clients over
that one API.

**Do not trust a claim that something is "done" without checking the code.** This
project has a documented history (`docs/041-Roadmap.md`'s Failure Modes section,
`ai-review-prompts/00-how-to-use.md`) of features that were built, tested, and then
*never actually wired into the real call path* — an ACP host with zero RPC methods, a
sandbox test that was a tautology, a Confidence Engine never compiled into the binary,
and (found in a 2026-07 session) real provider tool calls that were parsed off the
stream and silently discarded because nothing called the function that executes them.
Most recently (also 2026-07): a fully-built, tested prompt-injection sanitizer in
`skills/mod.rs` sat unused because `process_message_with_role` called a different,
weaker, duplicate implementation in `context/mod.rs` instead. **Before reusing or
extending any function, grep for its real callers — don't assume a tested function is a
wired one.**

## The master checklist

`review_prompt.md` (repo root) is the authoritative, numbered list of what's fixed and
what's still open in this codebase — it's the working document for an ongoing
"make it production ready" pass. When picking up work here, read it first; don't
re-derive scope from scratch. `ai-review-prompts/` holds a similar per-phase checklist
structure for re-verifying older "done" claims against actual code.

## Standing working conventions (established across sessions, keep following these)

- **Never commit or push without an explicit request.** Even after a large, clearly
  "done" chunk of work, stop and let the user decide when to commit.
- **Real fixes, real tests, real validation — every time.** After any change: run the
  affected test suite for real (not "should pass"), and for Rust changes run
  `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace --exclude cid --all-features` before calling something done.
  For frontend changes: `npx tsc --noEmit`, `npm run lint`, `npx vitest run`.
- **Minimal comments — WHY, never WHAT.** Identifiers should carry the "what"; a comment
  earns its place only by explaining a non-obvious constraint, a prior bug it prevents,
  or a design tradeoff a reader couldn't infer from the code alone.
- **State limitations honestly, in the code and in docs like `SECURITY.md`.** This
  project's own culture (see `ai-review-prompts/00-how-to-use.md`) is built around
  catching gaps between claimed and actual behavior — don't create new instances of that
  gap. If something is a coarse heuristic or a known-incomplete mitigation, say so next
  to the code, not just in a commit message.
- **Use local mock servers to test provider integrations, not "trust me it works."** The
  pattern established for Anthropic/OpenAI/Google/OpenAI-compatible tool-calling tests
  (`cid-core/src/model/mod.rs`'s `tool_execution_tests` module) is: make the provider's
  base URL/endpoint an injectable parameter, stand up a local `axum` mock server that
  scripts a realistic multi-round exchange, and assert the side effect actually happened
  (a real file was actually read/written), not just that the code compiled.

## Known Windows-specific operational issues

- **Transient WDAC block on a freshly-built test binary** (`os error 4551` /
  "Access is denied" tied to a specific `.exe` in `target/debug/deps/`). Fix: delete the
  specific flagged `.exe` and rebuild; it's a one-off flag, not a real problem with the
  binary.
- **`Access is denied. (os error 5)` on rebuild** — a running `cid-core.exe` (started for
  manual/live verification) locks the binary cargo needs to overwrite. Fix:
  `taskkill //F //IM cid-core.exe` before rebuilding, restart Core afterward if needed.
- **`npm run dev:all`'s bundled vite instance can crash with `EBUSY`** when its file
  watcher has `target/debug/deps/*.exe` open right as a `cargo build`/`cargo test` run
  rewrites it, and/or with `os error 10048` (port already in use) if a
  separately-started Core conflicts with the one `dev:all` also spawns. Workaround: run
  `cargo run -p cid-core -- --port 5919` and `npx vite` as two independent standalone
  background processes instead of the `concurrently`-wrapped `dev:all`, for any session
  that also runs `cargo build`/`cargo test` while a dev server needs to stay up.
- **A real, previously-unsolved bug this pattern used to mask**: repeatedly deleting
  `cid.db` to "fix" a `FOREIGN KEY constraint failed` was long assumed to be SQLite
  corruption from abrupt process kills. It was actually `connect_repo`'s `INSERT OR
  REPLACE` colliding with the `repo_channels.path` `UNIQUE` constraint on every repo
  reconnect, deleting the old row and violating `missions.repo_channel_id`'s FK — fixed
  at the source (now `INSERT ... ON CONFLICT DO UPDATE`), with a regression test. If a
  fresh FK-constraint error shows up again, don't assume it's the same class of "just
  delete the DB" issue without checking first.

## Frontend test gotcha (fixed 2026-07, worth knowing about)

`src/test-setup.ts` did not register `@testing-library/react`'s `cleanup()` between
tests, so multiple `render()`ed component instances from earlier tests in the same file
stayed mounted and could steal a later test's mocked API calls (silent, hard-to-diagnose
flakiness, only visible when a file has 2+ tests exercising the same mocked call). Now
fixed globally via `afterEach(cleanup)`. If a component test is inexplicably flaky only
when run as part of the full suite (not in isolation), check for this class of bug
before assuming it's a timing issue — prefer flushing a click-triggered async chain with
`await act(async () => { await Promise.resolve(); ... })` a few times over relying on
`findBy*`'s `MutationObserver`-based polling for multi-round-trip interactions.

## Current state snapshot — 2026-07-27

This section is a snapshot, not a permanent record — trust `git log` and
`review_prompt.md`'s own state over this once time has passed. Written so a fresh
session (or a different AI) doesn't have to re-derive it.

**`review_prompt.md` is fully closed out** — every numbered item (§1.1–§7) done, plus the
critical tool-execution bug found mid-pass (not originally in the doc; see
`CRITICAL-FINDING-tool-calls-not-executed.md`, now RESOLVED). Highlights:
- §1.2 prompt-injection: consolidated *three* divergent "build the system prompt"
  implementations down to one (`SkillsManager::build_system_context`), added the human
  AGENTS.md-approval gate (`repo.agents_md.approve`, `AgentsMdReviewCard`) and coarse
  tool-call provenance marking. See `SECURITY.md` §5.
- §3.2 checkpoint/rewind, §6 hunk-reject data-loss fix, §1.1 sandbox path confinement,
  §1.3 governance wiring, §2.1 real MCP stdio transport.
- §4 orphaned RPCs — flagged as only **partially** wired by `050`'s first pass (33 of 178
  methods still unreachable); closed for real in the same-day Wave 4/5 follow-up below —
  see `051` §5.1 for the per-group disposition, including the two (`mcp.task.subscribe`,
  `workspace.get`) deliberately left unwired with reasons recorded there, not silently
  dropped.
- §5: `PlanCard`/`DiffViewer`/`AutonomyPanel`/`McpPanel`/`ProvidersPanel` all have real
  component tests now (were 2 broken files + total gaps on the other three).
- §7 theming: `src/theme/tokens.json` → generated CSS variables
  (`scripts/generate-theme-css.mjs`, `npm run theme:generate`/`theme:check`), a working
  dark/light toggle (`src/theme/useTheme.ts`), verified live in a browser (screenshots),
  not just unit-tested.

**A third-party AI audit** ("the Gemini checklist", then a second "Comprehensive Project
Audit" pass) made claims about gaps; each was independently verified against real code
before acting (per this file's "don't trust a claim" rule above) — several claims in both
were stale or wrong (e.g. spend-recording and PR-merge governance were already wired;
the audits' own test-count subtotals didn't even sum to their stated totals). Resolved
this pass:
- **Jira/Linear credential verification** — GitHub/GitLab/Bitbucket already verified
  live; Jira/Linear now do too (`TrackerManager::verify_credentials`, real mock-server
  tests for both, `handle_tracker_token_set` calls it before persisting).
- **Monaco/xterm eager loading** — `EditorPane`/`TerminalPane` are `React.lazy` now;
  confirmed via the actual build output that `xterm.js` (291KB) is no longer
  `modulepreload`ed on initial page load.
- **TUI diff renderer** — `cid-tui` claimed "diffs from a shell" in its own module doc
  but had zero diff code. Added a real (read-only) hunk-colored diff view (`Focus::Diff`,
  toggled with `v`), backed by the same `git.diff` RPC the web `DiffViewer` uses, tested
  against a real local mock server.
- **Unified command driver** — `Justfile` at the repo root, `just check-all` verified by
  actually running it end-to-end (not just syntax-checked), wired into CI as an
  additional `just-check-all` job alongside (not replacing) the existing granular jobs.

**A full "gold standard" audit and remediation pass**, same day (2026-07-27) —
`docs/050-Gold-Standard-Review.md` (the audit, verified against real code per this file's
own rule) and `docs/051-Editor-Excellence-Roadmap.md` (the resulting spec). Waves 1, 2, 4,
and 5 are all done; Wave 3 (an LSP client) was deliberately not started — see `051`'s own
Tradeoffs section for why. Headline fixes: the `file.*` RPCs were completely unconfined
(same vulnerability class as the model-tool sandbox §1.1 fixed, but on the network-facing
Editor path — new shared `cid-core/src/path_confine.rs`); the editor silently destroyed
unsaved work switching files (now tabbed, with a real dirty-tracking close-confirm); DB
migrations swallowed every error including real ones (now `PRAGMA user_version`-tracked,
transactional, fail-loud); the ESLint gate had been weakened (`--max-warnings 0` dropped)
rather than genuinely fixed — restored, with the 60 real warnings fixed for real, not
suppressed. Also: every `alert()`/`window.confirm()` replaced with real toast/dialog UI
(`src/lib/dialog.ts` + `DialogHost`), a `Ctrl+K` command palette, focus traps + `Escape`
on every modal, `vitest-axe` (which caught a real accessible-name bug on first use), an
i18n scaffold applied to shared UI chrome, and **every `.tsx` component now has a
matching test file** (155 frontend tests across 28 files, up from 32/9). Full disposition
of every finding, including the handful of RPCs deliberately left unwired with reasons,
is in `051` — do not re-derive scope from scratch; read it first.

**Then completed in a follow-up pass**, once the user asked for all four explicitly
(previously deferred above with reasons — those reasons are preserved here since they
explain *why* each needed a real decision, not just effort, before starting):
- **Subagent real tool execution + per-path locking.** The actual prerequisite gap —
  `perform_subagent_work` was fully simulated — got fixed first
  (`ModelManager::run_subagent_turn` reuses the same 4 provider tool-execution loops the
  main agent uses, dispatched against the subagent's parent Mission). Only *then* did
  locking make sense: `ModelManager::file_locks` + `acquire_path_lock`, keyed by the
  resolved real path, held for the whole `execute_tool_with_approval` call including any
  human-approval wait. Verified with a lock-contention test proving actual serialization
  (not just that it compiles) and a full spawn-to-real-file-on-disk orchestrator test.
- **Network allow-list, not a full block.** Built the version flagged as correct instead
  of the naive one: `cid-core/src/net_guard` is a real local HTTP/HTTPS forward proxy
  (CONNECT tunneling for HTTPS) enforcing a reachable-host allow-list (github.com,
  registry.npmjs.org, pypi.org, crates.io, and common subdomains by default, editable via
  `sandbox.network_allowlist.get`/`.set`). `SandboxManager::ensure_network_guard` injects
  `HTTP_PROXY`/`HTTPS_PROXY` into every sandboxed command's real environment, verified
  against all 3 platform sandbox paths plus a live test on this Windows machine proving a
  spawned process's echoed `%HTTP_PROXY%` matches the real guard URL. Documented in
  `SECURITY.md` as application-layer (env-var honored, not kernel-enforced) — a process
  using raw sockets bypasses it, same honesty standard as the filesystem sandbox layers.
- **Real embeddings via candle.** `cid-core/src/semantic_engine/embeddings.rs`:
  `all-MiniLM-L6-v2` (Apache 2.0, BERT architecture, 384-dim), downloaded from Hugging
  Face on first Context Engine enable (not bundled — matches the feature's existing
  opt-in-per-repo shape) and cached under the OS data dir. `candle` (pure Rust) runs
  real tokenize → forward pass → attention-masked mean-pool → L2-normalize on CPU, no
  GPU assumed. Falls back to the original hash projection if the model isn't
  downloaded/loaded yet — a real, working degradation, not a crash, but documented
  honestly: embeddings from before/after the model becomes available aren't comparable
  (different dimensionality), self-healing on that repo's next re-scan. **Actually
  downloaded and ran the real model in this session** (not just unit-tested against
  mocks) — `CID_TEST_REAL_EMBEDDINGS=1 cargo test -p cid-core --lib embeddings --
  --ignored` genuinely fetched the ~90MB model and confirmed semantically related code
  scores higher cosine similarity than unrelated text, proving the whole pipeline works,
  not just that it compiles.
- **CI code-signing scaffold.** `.github/workflows/release.yml` (tag-triggered,
  `tauri-apps/tauri-action`) builds and, once secrets exist, signs Windows/macOS
  installers into a draft GitHub Release. Inert (unsigned, not failing) without secrets.
  Manual setup instructions — what to obtain and where to add it as a GitHub secret — are
  in `CONTRIBUTING.md`'s "Release Signing Setup" section, since obtaining a Windows
  signing certificate or an Apple Developer account is not something an agent can do.

**A live incident worth knowing about**: mid-session, `opencode run --auto` was used to
run the E2E suite in the background and, despite an explicit "do not modify source files"
instruction, corrupted a line of working Rust code and created an unrequested `Justfile`
(matching an idea only discussed, not agreed, earlier in the same conversation — it
appears to have acted on stray context rather than the actual instruction). Caught via an
incidental `cargo clippy` run, fixed immediately. Afterward, OpenCode was only run again
inside a fully filesystem-isolated copy (robocopy'd, no `.git`, junction'd
`node_modules`) so a repeat couldn't touch the real tree — see the Background Task
Delegation Rule above; consider giving it a permission-restricted agent profile (this
repo's `opencode.json` already defines a `review` subagent with `edit: deny` — a primary
agent with that same restriction, not just subagents, would be safer for
read-only/validation delegation than the default `build` agent `--auto` uses) rather than
trusting `--auto` + instructions alone.

## Website

`WEBSITE-BUILD-PROMPT.md` (repo root) is a complete, ready-to-hand-to-an-agent build
brief for the public site: **`opencid.dev`** is the live product's own hosted web
client (thin client over a Core you run yourself — CID has no multi-tenant hosted
backend), **`doc.opencid.dev`** is documentation/blog/community, built as a separate
standalone repo. Read that file's §0 before touching either — it corrects an earlier,
superseded draft that had the domains the other way around (marketing site vs. product).
