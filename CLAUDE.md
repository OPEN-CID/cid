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
pitch. In one line: Workspace → Repo Channel → Session Thread (Slack-shaped), Sessions
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
- **WDAC can also block *toolchain* binaries, where "delete and rebuild" doesn't apply**
  (hit 2026-08-19 on `cargo-fmt.exe` and `clippy-driver.exe`, both `os error 4551`). What
  works, in order: `rustup component remove <rustfmt|clippy>` then `add` it again — that
  re-extraction cleared the block on `cargo-fmt.exe`. It did **not** clear
  `clippy-driver.exe`, and neither did copying it elsewhere, so that block is on the file
  itself, not its path. The working fallback is to run the gate in Docker, mirroring CI's
  own command exactly: `docker run --rm -v C:/Projects/cid:/src -w /src -e
  CARGO_TARGET_DIR=/tmp/target rust:1-bookworm bash -c "... cargo clippy -p cid-core -p
  cid-tui --all-targets -- -D warnings"` (`CARGO_TARGET_DIR` inside the container keeps
  the Linux artifacts out of the Windows `target/`). Do this rather than skipping the gate.
- **A corrupted `target/` shows up as `can't find crate for <x>`, not as a file error.**
  Also 2026-08-19: `ratatui` and then `indoc` failed to resolve mid-build with artifacts
  visibly present in `target/debug/deps/` — cargo's fingerprint said fresh, the file was
  gone. `cargo clean -p <crate>` for the named crate fixes it one at a time.
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
  reconnect, deleting the old row and violating `sessions.repo_channel_id`'s FK — fixed
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
  main agent uses, dispatched against the subagent's parent Session). Only *then* did
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

## Production-readiness verification pass — 2026-08-05

A full re-verification of the 2026-07-27 snapshot above, against real code rather than
this file's own prior claims (the standing rule at the top of this file, applied to
itself). Most claims held up. What didn't, now fixed:

- **Light theme was incomplete, not fully "shipped."** `TerminalPane.tsx`'s xterm.js
  instance and one `DiffViewer.tsx` hunk panel had hardcoded dark hex colors
  (`#0a0e13`/`#e2e8f0`) independent of the token system — they stayed dark in light mode.
  Fixed: `TerminalPane` now reads `--background`/`--foreground` from the live CSS custom
  properties (re-applied on theme toggle via `useTheme`), and both panels use the
  `bg-background`/`border-border` token classes like the rest of the app.
- **This file's own "two deliberately unwired RPCs" claim undercounted.** `docs/051`'s
  §5.1 table itself already documented four more (`deployment.webhook`,
  `slack.trigger_session`, `teams.trigger_session`, `code.analyze_directory`) — this file
  just phrased it as if `mcp.task.subscribe`/`workspace.get` were the only two. Separately,
  a genuinely new, undocumented orphan turned up on re-running §4's `comm` check:
  `semantic_engine.test_impact.for_symbols` (the batch/union variant of the already-wired
  `.for_symbol`) had no frontend caller at all. Closed for real — a "look up covering
  tests for several symbols at once" affordance added to `SemanticInsights.tsx`'s
  existing Test Impact tab (not a new panel), with a component test
  (`RepoHealthPanel.test.tsx`) and `docs/051`'s table corrected to match.
- **`docs/033-Observability.md` was stale, not just incomplete.** It stated Prometheus
  metrics export and crash reporting were unbuilt Non-Goals. Both are real and shipped
  (`observability/mod.rs`: `Metrics::render_prometheus` backing a real `/metrics`
  endpoint, `install_panic_hook` + `CrashLog` backing `observability.crashes.list`) — the
  doc just never got reconciled after that work landed. Rewritten from the actual code.
  This is exactly the doc/code drift failure mode this file warns about elsewhere,
  caught by re-reading the implementation instead of trusting the doc.
- **Editor `file.*` RPC path-confinement had thinner test coverage than the model-tool
  side of the same fix.** The underlying guard (`path_confine::resolve_confined_path_in_any`)
  was already shared and correct, but `router.rs`'s `handle_file_read/write/list` had no
  RPC-layer regression test of their own — only the primitive's unit tests plus a fuzz
  test that asserted "no HTTP 500," not "the write was actually refused." Added six real
  tests against a spawned Core (`cid-core/tests/api_integration.rs`'s
  `file_rpc_confinement` module): absolute-path escape, `..` traversal, a `.git/hooks`
  target, a symlink escape, an out-of-repo `file.list`, and a legitimate in-repo path
  still working.
- **No production deployment documentation existed as a single, complete artifact.** The
  manual steps to run `cid-core` for a real team (TLS, a persistent service, backups,
  upgrades, monitoring, containerization) were scattered and, for several steps, entirely
  undocumented — no TLS example config, no backup procedure, no service-supervision
  guidance, no container image. Closed: `docs/052-Production-Deployment.md` (the runbook)
  plus real artifacts to back it — `Dockerfile`/`docker-compose.yml`/`.dockerignore` at
  the repo root, and `deploy/` (`Caddyfile`, `nginx.conf.example`, `cid-core.service`,
  `backup-cid-db.sh`, `prometheus-scrape-example.yml`). **The `Dockerfile` was written
  correctly against `cid-core/Cargo.toml`'s real dependencies but has not been
  build-verified — no Docker daemon was available in this session.** Say so plainly
  rather than claiming it works; verify it before relying on it.

Verified and left as-is (real, not stubs, on direct re-reading of the code): §1.1 sandbox
path confinement (model-tool side), §2.1 real MCP stdio transport (SEP-2322 multi-round-trip
still genuinely absent, correctly low-priority), §1.3 governance merge/spend wiring, §6
per-hunk reverse-apply, §2.2 ESLint config, §3.1 context compaction, §3.2 checkpoint/rewind.

Full Rust workspace suite (`cargo test --workspace --exclude cid --all-features`),
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`npx tsc --noEmit`, `npm run lint`, and `npx vitest run` (156 passed, up from 155 — the
new batch-lookup test is additive, plus 6 new Rust integration tests for the RPC-layer
confinement gap) all run clean after every change in this pass, on this machine, not
"should pass."

## Product-review pass — 2026-08-10

Driven by a UX review rather than a code audit (Session description should be optional,
no repo folder picker, no model selection, stale model catalog, clean up dummy data).
The features were built by delegated agents; **this pass was the verification on top,
and it found seven real defects — several in the brand-new code — none of which the
707 passing tests caught.** Full write-up, including the per-defect "why the suite
missed it", is `docs/053-Production-Readiness-Review.md`. Read it before touching this
area; do not re-derive.

The short version, because these are all instances of failure modes this file already
warns about:

- **Schema defaults don't migrate existing rows.** The model-catalog refresh changed
  the `CREATE TABLE` `DEFAULT`, the seed `INSERT`, and an in-memory fallback — so every
  *existing* install kept the retired `claude-3-5-sonnet-20241022` and 404'd. Now four
  real `UPDATE` migrations, verified against the actual `cid.db` on this machine. **If
  you change a default, ask what happens to databases that already exist.**
- **Bugs live in seams.** `fs.list_dirs` returned `\\?\C:\Projects\cid`; `repo.connect`
  stored paths verbatim onto a `UNIQUE` column. Two correct, individually-tested
  components; one duplicate-row bug on the exact column CLAUDE.md's FK-corruption
  incident was traced to. Fixed at the single storage boundary
  (`path_confine::normalize_stored_path`, `dunce`).
- **A fixture that invents a field can't fail.** The frontend read `m.enabled`; the RPC
  sends `available`. Every model option rendered disabled — the feature was
  100% broken — while its tests passed, because the mocks encoded the invented name.
  **Check new frontend types against a real RPC response, not against the code that
  consumes them.** `call()` returns an asserted type; TypeScript cannot save you here.
- **Use the feature before believing it.** `repo.disconnect` failed for *any* repo with
  a Session (bare `DELETE` vs. a live FK) — found by trying to click it.
- **Tests were writing to the real database.** `playwright.config.ts` started Core with
  no `--db`, so E2E runs polluted `%APPDATA%/cid/cid.db`; 15 dead channels had piled up
  in the real install and (because of the bug above) could not be removed. Now
  `npm run dev:core:e2e` → disposable `.cid-e2e/`.
- **The last simulated implementation on the user path is gone.** With no API key, Core
  used to write an *Assistant* message — "here's a simulated response… I would have:
  1. Analyzed the repo…" — and set the Session to `Review`, as if work were awaiting
  inspection. Now a `System` notice stating only what is true, and `Failed`.
- **`.cid/` is excluded from Vite's watcher** — a worktree Session on CID's own repo
  copies a `tsconfig.json` into `.cid/worktrees/`, which forced a full page reload and
  wiped the store mid-Session.

Verified live end-to-end (real Core, real `cid.db`, real browser: 11/11 Playwright
checks, console-clean), not just green tests. Gates after the final change: 541 Rust
tests, 181 frontend tests, fmt/clippy/tsc/lint all clean.

**Follow-up in the same pass — the model catalog is no longer hand-maintained.**
Verifying `OPENAI_MODELS`/`GOOGLE_MODELS` against the [models.dev](https://models.dev)
registry (the open catalog `opencode` uses) and OpenRouter found all three arrays wrong at
once: Google offered four `gemini-1.5-*` ids that **no longer exist** (one flagged
`default`, so the headline Google option was a guaranteed 404), OpenAI had nothing newer
than `gpt-4o`/`o1`, and `claude-sonnet-5` was priced $3/$15 instead of $2/$10 — 50% high on
the model the §1 migration had just made the default, which is a governance-cap input.
`context_window_tokens` also returned a flat 200k for every Anthropic model, so compaction
fired at 140k on a 1M window.

Replaced by `cid-core/src/model/catalog.rs`: live registry fetch → disk cache (24h TTL) →
a generated bundled snapshot, so any layer failing degrades instead of breaking. The
snapshot comes from `scripts/generate-model-catalog.mjs` (`npm run models:generate` /
`models:check`), following the existing `generate-theme-css.mjs` convention. **Do not
re-add a hand-written model array** — that is the thing that broke.

- Selection uses the registry's own `tool_call` + text-output flags, not name matching.
- `models:check` is intentionally *not* a blocking CI gate: the runtime prefers live data,
  so snapshot drift ages only the offline fallback, and a check that fails whenever a
  vendor ships a model is noise.
- **The bug worth remembering:** the per-provider cap was applied when parsing the registry
  but not when loading the disk cache, so a Core with an existing cache kept serving 37
  OpenAI models *after* the fix. Caught by re-running the real binary, not by the parser's
  own passing unit test. The cache is now `schema_version`-stamped and discarded on
  mismatch. **A cache is a second code path — fix both.**

**Still open at the time of that pass:** the `Dockerfile` was *not* build-verified and
could not be on this machine — Docker Desktop was installed but `HypervisorPresent` was
`False` and WSL absent. What was verified without a daemon: both base image tags resolve
in the registry, every `COPY` source exists and covers all three workspace members, and
one real defect was found and fixed by inspection — `VOLUME ["/home/cid/data"]` named a
directory the image never created, so Docker would have made it `root:root` while the
container runs as `cid`, and the default `CMD`'s `--db` write would have failed on first
start. **This is now closed — see the 2026-08-19 section below.**

## Release day and container verification — 2026-08-19

Two things happened after the snapshot above, and this section exists because the file
went a week without recording either. Read `docs/054-Browser-Release-2026-08-10.md` first
for the release-day detail; it is the live checklist and was updated in place.

**2026-08-17/18 (PRs #4 and #5, both merged).** Release day turned up defects that only
appear when you check the artifact a *consumer* gets rather than your own working tree:
`Cargo.lock` was gitignored and therefore absent from a fresh clone, so the `Dockerfile`'s
`COPY Cargo.toml Cargo.lock ./` could never have built; and with no lockfile every build
resolved its own dependency versions, which is why CI was green on an `h2` advisory that a
local `cargo audit` flagged. Also fixed: the web client could not authenticate to a
token-protected Core *at all* (`new WebSocket(...)` cannot set headers), so every hosted
deployment would have failed with an opaque closed socket — Core now also accepts the
token as a `cid.bearer.<base64url>` subprotocol (`SECURITY.md` §2), and `api.ts` became
protocol-aware for a TLS-fronted deployment.

**2026-08-19 (this pass).** Closing the items `docs/054` §4 had left open:

- **The `Dockerfile` is build-verified for real.** Docker Desktop works on this machine
  now. Built from a `git archive` of `HEAD` — the artifact Coolify clones, deliberately
  *not* the working tree, since that distinction is exactly what hid the `Cargo.lock`
  break. Clean build, 185 MB image; the container then passed `/health` 200, `/api/rpc`
  401 with no token *and* with a wrong one → 200 with the right one, `/ws` 401 → **101
  Switching Protocols** with the bearer subprotocol, and `cid.db` created in the volume
  owned by `cid:cid` (confirming the `VOLUME` ownership fix works rather than failing as
  root). `docs/052` §1 and `SECURITY.md` §2 now record this instead of the honesty note.
  **Not covered:** `docker-compose.yml`'s Caddy/TLS pairing, and an **arm64** build —
  Oracle's Always Free box is ARM, and that cross-build is the one gap left here.
- **`git2` 0.19 → 0.21 and `portable-pty` 0.8 → 0.9 — the two audit warnings that were
  reachable from our own `Cargo.toml`.** git2 0.21 clears RUSTSEC-2026-0008/-0183/-0184;
  its `StringArray` iterator now yields `Result<Option<&str>>` instead of `Option<&str>`
  — which *is* the unsoundness fix — so `list_worktrees` and `get_remote_url` were
  rewritten, the latter also losing a latent `.unwrap()`, with three regression tests over
  its branches. portable-pty 0.9 swaps the unmaintained `serial` crate for `serial2` and
  needed no code change at all. Net: **25 warnings → 21**, on a dependency graph that got
  *smaller* (704 crates, down from 710).
- **Why the remaining 21 are staying, with the arithmetic that decides it.** Every one is
  transitive, and the split is: **12** in the GTK3 stack Tauri v2 requires on Linux (the
  11 gtk-rs crates plus `proc-macro-error`, which arrives via `glib-macros`) with no
  upstream successor; **5** `unic-*` via `tauri-utils` → `urlpattern`; `instant` via
  `tantivy` → `measure_time`; `paste` via `candle`/`gemm`; and **2** for `lru`. `lru` is
  the interesting one: bumping `tantivy` 0.22 → 0.26 (an on-disk **search index format
  change**, so every indexed repo would need a rebuild) and `ratatui` 0.29 → 0.30 (a large
  API refactor) would clear RUSTSEC-2026-0002 — but **not** RUSTSEC-2026-0253, which needs
  `lru` ≥ 0.18.2 while tantivy 0.26 pins ^0.16.3. Two risky bumps to remove one of two
  warnings on a non-vulnerability is a bad trade; re-check when tantivy moves past
  `lru` 0.18.
- **Doc drift closed.** `WEBSITE-BUILD-PROMPT.md` still put the app at the `opencid.dev`
  root; the decision actually deployed against is **`cid.opencid.dev`** (the root stays
  free — `houses` shares that zone). Its Part A deploy section also still specified
  Cloudflare Workers, which `docs/054` §3 Option C superseded with Coolify; it is now
  marked superseded rather than left as a second competing target. Its "build a Connect to
  Core screen" spec was half-built by PR #5, so it now says which half exists (token entry,
  `localStorage`, rejected-vs-absent) and which is the real remaining gap (a **runtime**
  Core URL — host/port are build-time `VITE_*` only, so a hosted bundle can talk to
  exactly one Core).

- **Auth-token rotation without a restart** — `docs/052` §9's longest-standing operational
  gap, closed on request. `access.token.rotate` (authorized by the *current* token, like
  any RPC) swaps the token in place and then **closes every live WebSocket session**. That
  second half is the whole point: a socket is authorized once, at handshake, so without it
  a revoked credential keeps driving RPCs on the connection it already holds — the
  rotation would have been cosmetic. Mechanically: `AccessPolicy`'s token moved behind an
  `Arc<RwLock<..>>` so every clone of the policy observes the swap (a per-clone copy would
  have left the old token working wherever it was missed), and `handle_ws`'s read loop
  became a `select!` over the socket and a new `session_reset_tx` broadcast. Refuses a
  replacement under 16 chars, one identical to the current token, and any rotation on a
  Core with no token configured — introducing an auth requirement at runtime would lock
  out the desktop shell already connected without one. **In memory only**: no config file
  exists, so a restart reverts to `--auth-token`/`CID_AUTH_TOKEN`, and both `SECURITY.md`
  §2 and `docs/052` §9 say so rather than implying persistence. Verified against a real
  running Core, not only in tests (this file's "use the feature before believing it"
  rule): rotated over `curl`, watched the old token start returning 401, held a real
  browser-shaped socket open through it and saw the server close it with
  `disconnected_clients: 1`.
- **The E2E suite had a real fragility, found by running it rather than trusting it.**
  `flow1.spec.ts`'s golden path failed at `page.goto` while all 31 API-level specs in the
  same run passed — Playwright's **default 30s per-test timeout** is shorter than the
  first real navigation on a cold checkout, since that request is what triggers vite's dep
  pre-bundling. Two fixes, both at the source: an explicit `timeout: 90_000` (not a retry,
  which would have hidden a genuine hang behind a warm second attempt), and `127.0.0.1`
  everywhere instead of `localhost` — `vite.config.ts` binds `host: "127.0.0.1"` while on
  Windows `localhost` resolves to `::1` first, so every navigation paid a failed IPv6
  connect first. The specs now `page.goto("/")` against `baseURL` rather than hardcoding a
  host twice. Full suite: **32/32 cold**, including real worktree creation through the
  newly-bumped git2.

Gates, all run on this machine this session: **574 Rust tests** (0 failed, 1 ignored — the
network-dependent real-embeddings test), **201 frontend tests**, **32/32 Playwright E2E**,
`cargo fmt --check`, `cargo clippy -p cid-core -p cid-tui --all-targets -- -D warnings`
(clean — run in a Linux container, see below), `tsc --noEmit`, `npm run lint`,
`npm audit` (0 vulnerabilities), `cargo audit` (0 vulnerabilities, 21 warnings as above).

**One environment lesson worth more than the work it cost.** WDAC blocked
`clippy-driver.exe` itself, so `cargo clippy` could not run natively at all — see the
Windows issues section above for what does and doesn't clear that, and for the container
fallback that runs CI's exact clippy command instead of skipping the gate. Related: do
**not** run an emulated `buildx --platform linux/arm64` build alongside another container
build on this machine — doing so killed the Docker daemon mid-run and took both with it.

## Pre-release sanity pass — 2026-08-21

A verification pass over the previous session's *uncommitted* work, run before release.
The work itself was sound; **it was not finished**, and the gap was in the places a
session tends to stop checking once the interesting part works.

- **`cargo fmt` had never been run on the change that was left in the tree.** Two lines in
  `cid-core/tests/api_integration.rs` were over-width, so `cargo fmt --all -- --check` —
  a CI gate — failed on the first thing checked. Fixed. Worth internalizing: a working
  tree left mid-session is not a gate-passing tree, and fmt is the cheapest possible
  thing to get wrong.
- **The flaky-test fix it contained is real and correct.** `session_context_compact_is_a_real_manual_trigger`
  moved from a `before + 1` *count* assertion to *set containment over message ids*,
  because each `send_message` in the setup spawns a background turn that appends its own
  System notice at an arbitrary time. Re-verified under full-suite load, which is the only
  condition that reproduced it. The replacement is also a strictly stronger assertion (no
  prior message deleted, **and** the digest actually persisted into the Session's history).
- **The arm64 claim in the doc diff was true — checked, not assumed.** `cid-core:arm64-verify`
  was still in the local image store; `docker image inspect` confirms `linux/arm64`, and it
  was re-run here under QEMU: `uname -m` → `aarch64`, `/health` 200, `/api/rpc` 401 with no
  token *and* a wrong one → 200 with the right one, `cid.db` owned by `cid:cid` in the
  volume. This is the "use the feature before believing it" rule applied to a *doc claim*.
- **`.github/workflows/publish-image.yml` was new, untracked, and undocumented** — and it
  contradicted the runbook it exists to serve. It publishes the repo-root Dockerfile to
  GHCR as a multi-arch manifest list (`linux/amd64` + `linux/arm64`, each built **natively**
  on its own runner, merged by digest, with a job that fails if either architecture is
  missing). But `docs/054` §3 Option C step 2 still told you to build from source on the
  Oracle ARM box — a multi-hour compile that can OOM on a small shape, which is the exact
  thing the workflow removes. `docs/052` §1 and `docs/054` Option C now document the
  image; `docs/054` also referenced **`ghcr.io/open-cid/cid`**, an image name that will
  never exist (the workflow publishes **`cid-core`**). The workflow itself has **not run
  yet** — it first runs on the commit that adds it — and `docs/052` says so rather than
  implying a verified pipeline.
- **`docs/054` §4 contradicted its own header.** The header said arm64 was covered; §4
  still carried the "built for linux/amd64, that cross-build has not been run here"
  caveat. Same document, opposite claims — the drift this file warns about, three
  paragraphs apart.
- **The E2E suite's `webServer` timeout could never pass a genuinely cold run.**
  `dev:core:e2e` is `cargo run`, so on a cold `target/` that 120s budget had to cover
  compiling the entire dependency graph before Core could answer `/health` — it didn't,
  and the whole suite failed before a single spec ran. Raised to 900s with the reason
  recorded inline. CI never hit this because CI builds and starts Core in a separate step;
  only local cold runs pay it. (This is the *same shape* as the 2026-08-19 per-test
  timeout fix — a cold-start cost exceeding a default budget — one layer further out.)
- **The bundled model catalog had drifted** (`npm run models:check` failing, snapshot dated
  2026-07-24 vs. the registry's 2026-08-13). Deliberately not a CI gate, but the snapshot
  *is* what a fresh install uses when offline, so shipping a release on a month-old
  fallback is not what "degrades gracefully" was supposed to mean. Regenerated; the diff is
  additions/reordering/pricing only — no `default`-model change, so none of the
  `docs/053` §1 "schema defaults don't migrate existing rows" hazard applies here.

**WDAC note, updated:** `cargo clippy` runs **natively again** on this machine — the
`clippy-driver.exe` block described in the previous section has cleared. Try native first
now; the Docker fallback is still there if it returns.

Gates, all run this session: `cargo fmt --check` ✓, `cargo clippy -p cid-core -p cid-tui
--all-targets -- -D warnings` ✓ (native), **574 Rust tests** (0 failed, 1 ignored — the
network-dependent real-embeddings test), **201 frontend tests**, **32/32 Playwright E2E**,
`tsc --noEmit` ✓, `npm run lint` ✓, `npm run theme:check` ✓, `npm run build` ✓,
`npm audit` 0 vulnerabilities, `cargo audit` 0 vulnerabilities / 21 warnings (the same
transitive set dispositioned above, 704 crates). Plus a real browser against a real Core
on the real `cid.db`: app renders, `Core: connected (ws://127.0.0.1:5919)`, a page-origin
`workspace.list` returns 200, **zero console errors**.

## Product pass — "Mission" is now "Session", and Search actually works (2026-08-21)

Driven by the user *using* the app and reporting that "most of the functionality" was
broken. Every complaint was real, and none of the 587 tests covered any of them. Read
this before touching search, the editor, or the naming.

**What was actually wrong** (all reproduced against a running Core with a scripted
browser, not inferred):

- **Search hung forever.** `analyzer::analyze_directory` recursed with *no ignore list* —
  `target/`, `node_modules/`, `.git/`, and CID's own `.cid/worktrees/` copies of the repo
  — reading and tree-sitter parsing every `.rs/.ts/.js/.py/.go/.json` it found. 27,301
  files on this repo; one `code.search_symbols` took **218 seconds**, and it ran
  *synchronously inside an async handler*, holding a Tokio worker the whole time. The
  `.cid` case also returned a duplicate of every hit, since a Session worktree is a full
  copy of the repo.
- **The editor was a CDN download.** `@monaco-editor/react` was never given
  `loader.config({ monaco })`, so it fetched Monaco from **cdn.jsdelivr.net** at runtime —
  in a self-hosted tool. Offline, behind a proxy, or under a CSP it showed "Loading..."
  forever. It also served a *different build* than the one vite bundles (CDN 0.55.1 vs
  installed 0.56.0), and `monaco-editor` **was not even in `package.json`** despite
  `vite.config.ts` naming it in `manualChunks` — it resolved by hoisting luck.
- **Creating a Session looked like it failed.** `session.create` succeeded, but nothing
  selected the new row, so the header stayed on "no session selected" and the thread on
  its empty state.
- **Twelve right-panel tabs on by default**, including acronyms (`Mcp`, `Acp`) that told a
  first-time user nothing.
- **`\\?\` leaked into the UI.** Confinement canonicalizes, and the extended-length path
  went straight to the file tree and editor tabs. Fixed at the shared boundary
  (`path_confine` now returns `dunce::simplified`) — the comparisons still run on the
  canonical form, which is what makes them sound.

**Search is now ripgrep's engine.** New `cid-core/src/search` on `ignore` +
`grep-searcher` + `grep-regex` (the crates ripgrep itself is built from, which is also
what VS Code ships), behind a new `search.text` RPC, confined to connected repo roots
exactly like the `file.*` RPCs and run in `spawn_blocking`. `.gitignore`-aware, so build
output is skipped because the repo already says to skip it rather than because of a list
that drifts. Literal-by-default (a stray `(` must not error), smart-case, bounded by a hit
cap that reports `truncated` instead of streaming forever. **218,000 ms → 39 ms** on the
same query, verified against the real repo. `analyze_directory` keeps symbol search but
now has its own ignore list, a file cap and a size cap.

**"Mission" became "Session" everywhere** — UI, RPC methods, DB tables — at the user's
explicit direction, twice. `SessionMode` (worktree vs shared) became **`IsolationMode`**,
and the auth system's login-session concept became **`AuthSession`/`auth_sessions`**,
because `Session`, `sessions` and `SessionMode` were all already taken by it.

**Three hazards this rename hit. All three are traps for the next person.**

1. **PowerShell's `-replace` is case-INSENSITIVE.** The first sweep ran
   `-replace 'MISSION','SESSION'` first, which matched *every* casing and rewrote the
   whole codebase to `SESSION_id`/`SESSIONs`. Reverted from a `git diff` patch taken
   beforehand — take one before any sweep. Use `-creplace`.
2. **`permission` contains `mission`.** The second sweep turned 114 of them into
   `persession`, including the `role_profile.check_permission` RPC string and the
   serialized `tool_permissions` field — **and it still compiled**, because the corruption
   was self-consistent. Only a targeted grep for `[A-Za-z]+session` found it. Any
   identifier sweep needs a `(?<![A-Za-z])` guard *and* a corruption grep afterwards.
3. **A rename cannot be an ordinary entry in `MIGRATIONS`.** `ALTER TABLE ... RENAME` is
   not idempotent and the base `CREATE TABLE IF NOT EXISTS` batch runs on *every* open, so
   whichever names the base batch used, something broke: new names → an empty
   `auth_sessions` gets created before the real one can be renamed there; old names → a
   stray empty `missions` table reappears beside the real `sessions` forever. It is now
   `rename_mission_schema_to_session`, which runs **before** the base batch and guards
   every step on `sqlite_master`, so it is a no-op on a fresh DB and on an already-migrated
   one. The auth table is identified by its `token` column, not its name — after the
   rename a table called `sessions` also exists and is a completely different thing.
   Migrations 18/19 were edited from `missions` to `sessions` (normally forbidden) because
   the fixup guarantees the table is already renamed by the time they run.

**Verified on the real database, not just fixtures.** A copy of the actual
`%APPDATA%/cid/cid.db` (4 Sessions, 4 messages, 4 plans) was migrated by starting the real
binary against it: every row survived under the new names, the login table moved to
`auth_sessions`, and no stray `missions` table was left. There is a backup at
`scratchpad/real-cid-backup.db`. Two regression tests pin it — one builds a pre-rename
database *with data* and reopens it through `Persistence::new`, the other proves a second
open is a no-op.

**A test that was pinning the wrong thing.** `estimate_cost_usd_prices_google_by_model_tier`
hardcoded Gemini's price list and broke the moment Google repriced Flash from $1.50/$7.50
to $0.75/$3.75 — an assertion about a vendor's pricing decisions, not about our code. All
three pricing tests now derive the expected figure from the catalog and assert the
*behaviour* (per-model lookup, tiers ordered correctly, two same-family ids not collapsing
to one price), which is what the family-heuristic bug was actually about.

**The default UI is now Editor / Terminal / Diff**, with the other nine one click away
behind a ＋ menu that persists to `localStorage`; every panel still works, and the command
palette and LeftRail rows reveal a hidden one rather than silently doing nothing. Tab
labels are words now (`Tools`, `External agents`, `Decision log`, `Repo health`). The left
rail's Context section is collapsed by default. Note for tests: panel visibility persists,
so `localStorage.clear()` belongs in `beforeEach` — without it one test's reveal leaks
into the next.

## Follow-up from real use — same day (2026-08-21)

Found by the user driving the app, not by tests. Each one is recorded because
the test suite was green through all of them.

- **A file saved in the Editor never appeared in Diff.** The Editor resolved its
  path from `repos.find(...).path` — the *main* checkout — while the Diff panel
  and the Terminal (`pty.create` in `router.rs`) both use
  `worktree_path.unwrap_or(repo.path)`. So with a worktree Session selected, the
  Editor was writing to a different working tree than the one being diffed, and
  human edits landed **outside the Session entirely**: never checkpointed, never
  reviewed, never merged. Both panels now share `useSessionRepoPath`, which is
  the point — two copies of this rule is what let them drift. Panels that
  configure the repo as a whole (Skills, Repo health, Automation) still use the
  main repo path deliberately.
- **Opening `.coverage` showed "stream did not contain valid UTF-8" as the file's
  contents.** It is a SQLite database. The real hazard was the next step: that
  error text sat in the editor buffer as if it were the file, so pressing Save
  would have written it over the real bytes. `file.read` now reports `binary` /
  `too_large` as *properties*, the Editor renders a read-only notice, and every
  save path refuses a tab that carries `readOnlyReason`.
- **The Terminal never said which tree it was in.** It silently used the
  Session's worktree with no way to reach the main checkout. `pty.create` takes
  `workdir: "session" | "repo"` (defaulting to `session`, so existing clients are
  unaffected) and returns the resolved `cwd`, which the UI displays rather than
  reconstructs. Opening the main repo — or another Session's worktree — is
  flagged amber, because commands run there are not captured by the selected
  Session's checkpoints and will not show in its diff.

**Local models became a real feature rather than a status line.** It was
detection-only: three HTTP probes and a list. Now split into `Cloud providers` /
`Local models` tabs, with:

- `local_models::system` — measured RAM/cores/GPU (`sysinfo`, which was a
  declared dependency that nothing had ever used, plus `nvidia-smi`/CIM/
  `system_profiler` for GPUs). Unknown VRAM is reported as `None`, never guessed,
  because a made-up number would change a recommendation.
- `local_models::catalog` — a curated, code-capable model list classified
  `Comfortable | Tight | TooLarge` against that machine's real budget (VRAM, or
  RAM minus 4 GB of OS headroom). Sizing is off *working memory*, not download
  size, which is the usual way these recommendations go wrong.
- `local_models::manager` — start/stop and `ollama pull` with streamed progress.

**Two boundaries here are deliberate and should not be "improved" away.** First,
CID does **not** install the runtime: that is software installation on someone's
machine and it belongs to the user, so an absent binary links to the official
download instead. Second, `stop` only ever kills a child *this process* spawned —
a server already running as a service is reported `External` and refused, with
the reason shown in the UI. Both have tests.

## Why "most things don't work" — the root cause (2026-08-22)

The user reported the product was broken. Every panel opened, all 186 RPCs
resolved, all 7 notifications matched, 603 Rust and 216 frontend tests passed —
and the product still did nothing useful. **Start from the agent loop, not the
UI.** The audit that found it, in order, was: enumerate frontend RPC calls vs.
backend handlers (clean) → notification names both directions (clean) → open
every panel and record RPC errors (clean) → *actually send a message* (broken).

**`provider_default_model` hardcoded `gemini-1.5-flash`.** Google retired that
id. With `GEMINI_API_KEY` set in the environment and no explicit `google_model`,
every Planner and Implementer turn resolved to it and got a **404 on every
call**. `model.list` correctly reported Google as enabled, so nothing looked
misconfigured — the Session just silently produced a placeholder plan and the
Implementer stayed blocked, forever.

This is the *same failure* `docs/054` already fixed once. `model::catalog` was
built specifically so no hand-written model id could rot — but this function
never consulted it, so the fix didn't reach the code path that actually picks a
model. **Grepping for the fixed symbol is not the same as grepping for the
pattern.** The defaults now come from the catalog, skipping `preview` ids, and
`each_providers_default_model_actually_exists_in_the_catalog` fails if any
provider's default is not in its catalog — the assertion that would have caught
this on day one.

Two honesty bugs found alongside it, both of which actively misdirected:

- **The placeholder plan always said "No planning model is configured"**, even
  when a model *was* configured and the call failed. It now states the real
  reason (`The planning model was called but failed: …`), so a 503 or a rejected
  key doesn't send someone to re-do correct settings.
- **The status bar always reported a model.** It defaulted the provider to
  `"anthropic"` and the id to the schema default regardless of whether a key
  existed, so an unconfigured install displayed `claude-sonnet-5 (anthropic)`.
  It now derives from `model.list`'s own `available` flag and says
  "none configured" when nothing is callable, with a banner
  (`ModelReadinessBanner`) and an inline warning in the Session dialog.

**Environment variables count as configuration.** `provider_api_key` falls back
to `ANTHROPIC_API_KEY`/`GEMINI_API_KEY`/etc., so "the settings table is empty"
does not mean "no provider is available" — checking the DB alone gave the wrong
answer here and cost a detour building a readiness banner for a state the
machine wasn't in.

## Website

`WEBSITE-BUILD-PROMPT.md` (repo root) is a complete, ready-to-hand-to-an-agent build
brief for the public site: **`opencid.dev`** is the live product's own hosted web
client (thin client over a Core you run yourself — CID has no multi-tenant hosted
backend), **`doc.opencid.dev`** is documentation/blog/community, built as a separate
standalone repo. Read that file's §0 before touching either — it corrects an earlier,
superseded draft that had the domains the other way around (marketing site vs. product).
