# 050 — Gold-Standard Review

## Vision

A full-surface audit of CID as it actually stands on 2026-07-27, done the way this
repository requires: every claim below was verified by reading the implementation or
running it, not by trusting `review_prompt.md`'s closure notes, `041-Roadmap.md`'s phase
summaries, or `CLAUDE.md`'s own state snapshot. Where a prior document says something is
closed and the code says otherwise, this document says so plainly and names the file and
line.

Its companion is `051-Editor-Excellence-Roadmap.md`, which turns the gaps found here into
a sequenced feature spec.

**Status update (2026-07-27, same day, follow-up pass):** Wave 1 (F1–F3, all three S1
defects) and Wave 2 (F4, F5, and the `context_engine.toggle` half of F6) are fixed, each
with a regression test that failed before the fix. See each finding's row below for what
changed.

**Second status update (2026-07-27, same day, second follow-up once the user asked for
the full backlog through Wave 5 explicitly):** F6's remaining orphaned-RPC groups, F7
(dead tab), F8 (editor credibility), F9 (local E2E harness), F10 (accessibility), F11
(i18n), F12 (`alert()`), and F13 (test parity) are now **all fixed** — see
`051-Editor-Excellence-Roadmap.md` Waves 4–5 for exactly what was built against each.
F14 (debugger/test-explorer) remains an intentional non-goal, unchanged from the original
Tradeoffs section below. Two RPCs (`mcp.task.subscribe`, `workspace.get`) were evaluated
and deliberately left unwired with reasons recorded in `051` §5.1 — not an oversight.

## Goals

Answer two questions with evidence:

1. **Is the green baseline real?** — run every gate, report honest numbers.
2. **Where is CID short of a gold standard, area by area?** — and which of those gaps are
   defects, which are unreachable features, and which are deliberate non-goals that
   should stay closed.

## Verified baseline (all gates run in this pass)

| Gate | Command | Result |
|---|---|---|
| Rust tests | `cargo test --workspace --exclude cid --all-features` | **498 passed**, 0 failed, 1 ignored |
| Rust lints | `cargo clippy --workspace --all-targets -- -D warnings` | **clean** (exit 0) |
| TypeScript | `npx tsc --noEmit` | **clean** (exit 0) |
| Frontend unit | `npx vitest run` | **32 passed** across 8 files |
| E2E | `npx playwright test` *with Core on :5919* | **30 passed** |
| ESLint | `npm run lint` | exit 0 — **but see F4**: 0 errors, **60 warnings**, gate weakened |

**Re-verified after the Wave 1/2 fix pass, same day:** Rust **510 passed** (0 failed, 1
ignored — the 12 new tests from F1/F3), clippy clean, `tsc` clean, frontend unit **37
passed** across 9 files (the 5 new `EditorPane.test.tsx` cases), E2E **32 passed** with
Core running (the 2 new confinement cases), `npm run lint` **0 warnings** at
`--max-warnings 0`. One honest caveat: `flow1.spec.ts` (Flow 1 golden path) runs close to
its 30s test timeout on this machine under load — passed reliably alone (~25-29s) but
timed out twice mid full-suite runs; confirmed via isolated reruns this is pre-existing
timing marginality, not a regression from this pass's changes (it doesn't touch anything
Flow 1 exercises).

**Re-verified again after the Wave 4/5 pass, same day:** Rust unchanged at **510 passed**
(no Rust code touched this pass), clippy clean, `tsc` clean, frontend unit **155 passed
across 28 files** (every `.tsx` component now has a matching test file — up from 9),
`npm run lint` **0 warnings**, E2E **32/32 passed** standalone with no manually-started
Core (F9's fix — `npx playwright test` now auto-starts it via `playwright.config.ts`'s
second `webServer` entry). No regressions from Wave 1/2's baseline in any gate.

The baseline is genuinely green and materially better than the 406/2/30 recorded in
`041-Roadmap.md` § Testing, which is now stale. Rust coverage in particular has grown by
~92 tests since that number was written.

**One caveat on the E2E number.** `npx playwright test` on a clean machine gives *16
failed, 10 skipped, 4 passed*. `playwright.config.ts`'s `webServer` block starts vite
only; nothing starts `cid-core`, so every RPC assertion fails against a dead socket. CI
does it correctly (`.github/workflows/ci.yml:171-178` builds and backgrounds Core first),
so this is a local-DX trap, not a broken suite — but a contributor's first E2E run looks
like a catastrophic regression. Tracked as F9.

## Findings

Ranked by consequence. `Sn` = severity, `Fn` = finding id used by `051`.

### S1 — Defects that should block a release claim

**F1 — FIXED.** New shared `cid-core/src/path_confine.rs` (`resolve_confined_path`,
`resolve_confined_path_in_any`), reused by both `model::ExecutionContext` (§1.1's original
path) and the three `file.*` handlers, confined against every connected repo channel's
path. Regression tests: 8 unit tests in `path_confine.rs`, plus 2 new E2E cases in
`tests/e2e/health-check.spec.ts` (traversal and outside-every-repo denial).

**F1 (original) — The `file.*` RPC surface is completely unconfined.**
`cid-core/src/api/router.rs:1328-1372`. `handle_file_read`, `handle_file_write`, and
`handle_file_list` take a caller-supplied path straight to `tokio::fs` with no
canonicalization and no root check. `handle_file_write` additionally calls
`create_dir_all` on the parent, so it will happily manufacture a path that didn't exist.

This is the *same vulnerability class* `review_prompt.md` §1.1 fixed — but §1.1 hardened
`execute_tool_direct_in`, the path the **model's** tools take. The RPC the **Editor pane**
takes was never confined, and it is the one exposed on the network socket.

Severity is honestly **low today and high on the next feature**: the default bind is
loopback with no token (`access/mod.rs:69-73`), CORS is a real origin allow-list
(`router.rs:88-99`), so the practical reach is "a local process running as the same user"
— which could already write those files directly. But `review_prompt.md` §7.3 and
`049-Extensibility-And-Sync-Roadmap.md` both plan same-network device pairing as the next
shippable feature. The moment Core binds to a LAN address, one bearer token becomes
arbitrary read/write on the whole host filesystem, not "review diffs from my phone."
**Confine these three handlers before LAN pairing ships, not after.**

**F2 — FIXED.** `EditorPane.tsx` now tracks `savedContent` separately from `content`,
blocks a dirty switch behind an in-app `UnsavedChangesModal` (save/discard/cancel, not
`window.confirm`), binds `Ctrl+S`/`Cmd+S`, and guards `beforeunload`. 5 new tests in
`EditorPane.test.tsx`.

**F2 (original) — The editor silently destroys unsaved work.**
`src/components/editor/EditorPane.tsx:28-40`. `handleFileSelect` overwrites `content`
unconditionally. There is no dirty flag, no confirmation, no autosave, and no `Ctrl+S`
binding — saving requires clicking the Save button. Edit a file, click another file in
the list, and the edit is gone with no indication it ever existed.

This is the same data-loss class as the `git.hunk.apply` reject bug that §6 correctly
called out as deserving higher priority than its framing suggested. It is currently
unmitigated and untested (`EditorPane` has no test file).

**F3 — FIXED.** `run_migrations` now tracks `PRAGMA user_version`, applies pending
migrations inside a transaction, tolerates only the one specific expected error
(duplicate column, from the base-schema overlap described in `MIGRATIONS`'s doc comment),
propagates every other error as a hard startup failure, and refuses to open a database
stamped with a version newer than the binary knows. 4 new tests in `persistence/mod.rs`,
including one proving a genuine failure aborts and rolls back rather than partially
applying.

**F3 (original) — Schema migrations swallow every error and track no version.**
`cid-core/src/persistence/mod.rs:275-301`. Migrations are a flat array of
`ALTER TABLE … ADD COLUMN` run as `let _ = conn.execute(sql, [])`. There is no
`PRAGMA user_version`, no ordered ledger, and no record of what has been applied.

The comment says "ignore errors" because a re-run legitimately fails with "duplicate
column name." The problem is that this makes a *real* failure — a lock, a full disk, a
constraint violation, a corrupted file — indistinguishable from that benign case. The
process then continues and runs queries against a schema it merely assumes exists,
surfacing later as an unrelated runtime error. There is also no way to detect a database
written by a *newer* CID than the running binary.

### S2 — "Claimed vs. actual" — this repository's signature failure mode

`CLAUDE.md` and `041-Roadmap.md` both institutionalize catching gaps between claimed and
real behavior. These are new instances of exactly that.

**F4 — FIXED.** `--max-warnings 0` restored to `package.json`'s `lint` script (the
now-redundant `lint:strict` removed); all 60 warnings fixed for real — typed properly at
~50 sites, 8 `react-hooks/exhaustive-deps` fixed (two were real stale-closure risks, not
just lint noise: `DiffViewer`'s notification handler and `TerminalPane`'s PTY write path),
and the handful of genuinely-dynamic JSON-RPC-boundary cases routed through one named,
justified `RpcValue` alias in `src/lib/api.ts` instead of a bare `any`. `npm run lint`
now passes at 0 warnings.

**F4 (original) — The ESLint gate was weakened rather than satisfied.**
`review_prompt.md` §2.2 fixed the missing config and closed with an explicit instruction:
*"`--max-warnings 0` is already in the script — keep it."* `package.json:22` now reads:

```
"lint": "eslint . --ext ts,tsx --report-unused-disable-directives"
```

`--max-warnings 0` is gone. `npm run lint` reports **60 warnings** and exits 0, so the
`lint-frontend` CI job is green while enforcing nothing beyond "parses." The gate was
made to pass by lowering the bar, which is the failure mode §2.2 existed to fix.

**F5 — FIXED (docs corrected, per 051 Wave 2.2's recommended option).**
`018-Native-Editor.md`, `012-Semantic-Editing.md` (including its diagram and the
hunk-reject claim, also stale — §6's real per-hunk reverse-apply had shipped but this doc
still described the old whole-file `git checkout` behavior), `000-Executive-Vision.md`,
`042-ADRs.md`, and `docs/adr/0006-editor-strategy.md` (given a superseding-status note
rather than rewritten, since it's a decision record) now state plainly that Monaco is the
only editor CID ships and that the planned LSP integration was never built — tracked as
real, not-yet-started work in `051` Wave 3, not a stale claim.

**F5 (original) — CodeMirror is documented as shipped and does not exist; the LSP claim
was never corrected.**
Nine documents describe CodeMirror as a live component — `018-Native-Editor.md:11-12`
states flatly *"CID uses CodeMirror 6 (inline) and Monaco (full pane)"*. There is **no
`codemirror` dependency in `package.json` and no reference to it anywhere in `src/`.**
The inline-editing story it describes does not exist in any form.

Separately, `review_prompt.md` §2.3 found no LSP integration and required *"either build
it, or correct every doc that claims it,"* recommending the doc correction. Neither was
done: `docs/adr/0006-editor-strategy.md:8` still advertises Monaco *"with LSP integration
for supported languages,"* and there is still no LSP client, no `lsp-types`/`tower-lsp`
dependency, and no protocol code in `cid-core`.

**F6 — §4's orphaned-RPC closure is overstated: 33 of 178 methods are still unreachable.**
`CLAUDE.md`'s snapshot lists "§4 orphaned RPCs wired to UI" as done. Re-running the
review's own detection command finds 33 methods with no string-literal match anywhere in
`src/`. Real progress *was* made — the Reviewer (`session.review.run/get`) and the Context
Engine (`enable`/`disable`) are genuinely wired now, and those were the two named
priorities. But six whole feature groups still have zero surface:

| Group | Methods | Status |
|---|---|---|
| Role profiles | `role_profile.create/get/list/update/delete/check_permission` | 6 — no UI. Phase 4 deliverable with real enforcement in tool dispatch; no way to create or assign one. |
| Semantic engine | `test_impact.entries/for_symbol/for_symbols`, `docs.for_symbol`, `docs.stale`, `index_file`, `load_blame` | 7 — headline Phase 4 feature, invisible. |
| Slack / Teams | `slack.configure/config.get/trigger_session` + 3 Teams equivalents | 6 — unconfigurable without hand-written RPC. |
| Code analysis | `code.analyze_file/analyze_directory/search_symbols/get_imports` | 4 — reachable only internally and from E2E tests. |
| Decisions & deployment | `decisions.list/for_session`, `deployment.record/list/webhook` | 5 — Phase 4 deliverables, no surface. |
| Misc | `confidence.history`, `mcp.task.subscribe`, `workspace.get`, `session.review.list` | 4 — smaller gaps. |

**One correction to the original review**, in the spirit of §9's "a corrected finding is a
good outcome": `context_engine.toggle` was §4's *highest-priority* item on the grounds
that the Context Engine could never be enabled from the UI. That is no longer true and
the fix was better than the one prescribed — `LeftRail.tsx:41-46` calls
`enable`/`disable` explicitly, which is strictly better UI behavior than a blind toggle.
`context_engine.toggle` was dead API surface — **now deleted** (handler and registration
removed from `router.rs`; confirmed no other caller anywhere in the repo before removal).
The other six groups in the table above are unchanged — still open, Wave 5 scope.

**F7 — Dead `files` tab.** `src/App.tsx:36` declares `"files"` in the `RightTab` union and
`:307` renders it, but it is absent from the tab-bar array at `:278`. The branch is
unreachable, and it renders `EditorPane` — a duplicate of the `editor` tab regardless.

### S3 — Gold-standard gaps by area

**F8 — The editor pane is demo-grade.** Beyond F2, `EditorPane.tsx`:
- The file tree is a **flat, single-level listing**. Directories render with a 📁 and
  `:60`'s `!f.is_dir && handleFileSelect` makes clicking one do *nothing* — there is no
  expansion, no recursion, no way to reach any file in a subdirectory. On this repository
  that means `cid-core/src/**` is entirely unreachable from the editor.
- **One file at a time.** No tabs, no history, no back/forward, no split.
- **Language detection covers three extensions** (`:89`) — `.rs`, `.ts`, `.tsx`. Python,
  Go, JSON, Markdown, YAML, TOML, CSS, and everything else render as `plaintext`, despite
  Monaco shipping grammars for all of them and `cid-core` already carrying tree-sitter
  grammars for Python, Go, JavaScript, and JSON.
- **Monaco's theme is hardcoded `vs-dark`** (`:95`). §7's theming work shipped a real
  light mode; switching to it leaves a black editor embedded in a light UI.
- No find/replace, no go-to-definition, no symbol jump, no breadcrumbs, no minimap.
- The pane is locked to a **fixed 520px right panel** (`App.tsx:276`) with no resize and
  no pop-out.

**F9 — Local E2E harness doesn't start Core.** `playwright.config.ts:21-28`. See the
baseline caveat above.

**F10 — Accessibility is effectively absent.** Seven `aria-label` attributes in the entire
frontend. Two `onKeyDown` handlers total (`ChatThread.tsx:295`, `LeftRail.tsx:96`), both
single-purpose. No focus management, no focus traps on the modal (`SessionCreationModal`
is a raw `fixed inset-0` div), no skip links, no command palette, no keyboard shortcuts,
no keymap customization, no documented screen-reader pass.

**F11 — No i18n scaffolding.** No `i18n` library, no `useTranslation`, no message
catalogue; every string is a hardcoded English literal in JSX.

**F12 — `alert()` is the error channel.** Nine calls across `App.tsx`, `LeftRail.tsx`,
`McpPanel.tsx`, and `SkillsPanel.tsx`, including the primary Session-creation failure path
(`App.tsx:70`). Blocking, unstyled, untestable, and impossible to act on.

**F13 — Frontend test coverage lags the backend badly.** 8 test files against 27 source
modules, and coverage is concentrated on the approval-critical components §5 correctly
prioritized. Untested: `EditorPane` (which has F2's data-loss bug), `TerminalPane`,
`SkillsPanel`, `AcpPanel`, `RepoHealthPanel`, `HistoryPanel`, `WebShell`,
`AgentsMdReviewCard`, `CheckpointCard`, `ConfidenceCard`, `ReviewCard`, `McpAppCard`, and
the entire `mobile/` shell. Ratio: 498 Rust tests to 32 frontend tests, for a product
whose whole value is delivered through the frontend.

**F14 — No debugger, no test explorer.** No DAP client, no breakpoint concept, no
run-tests-from-the-UI affordance anywhere in Core or the frontend. `repo_health` detects
*test presence*; nothing runs them on demand.

## What is genuinely strong

Stating this because a findings list reads worse than the codebase deserves:

- **The security posture is unusually honest.** Path confinement on model tools, the
  `AGENTS.md` human-approval gate, a real forward-proxy network allow-list documented as
  application-layer rather than kernel-enforced, and constant-time token comparison — with
  `SECURITY.md` stating residual risk instead of claiming elimination.
- **The provider integration testing pattern is a genuine best practice.** Injectable base
  URLs plus local `axum` mock servers asserting the real side effect (a file actually
  written), for all four provider families.
- **498 green Rust tests with clippy at `-D warnings`** is a real bar, well above typical
  for a project this age.
- **The failure-mode culture works.** Four false "already built" claims caught across
  phases, plus the tool-calls-never-executed bug — all found by institutionalized
  re-verification. F4–F7 above are that same process continuing, not a new problem.

## Tradeoffs

The largest question this review has to answer honestly is what "gold standard for all dev
areas — a perfect editor" should even mean here.

`018-Native-Editor.md`, `docs/adr/0006-editor-strategy.md`, and `041-Roadmap.md`'s scope
table all make the same deliberate, evidence-gated decision: **CID is not an editor.** It
is a chat-native multi-agent platform where the editor is a support surface for reading
and verifying what agents did. `018` argues the point well — Zed took ~5 years with
dedicated funding and Tree-sitter's creators to build the alternative.

Chasing VS Code parity would reverse that decision without the evidence `018` demands, and
would be a multi-year project. So this review does **not** recommend it. The bar it
applies instead:

> Gold standard **for what CID is** — the agent loop, its safety envelope, and its
> surfaces — plus lifting the editor from demo-grade to genuinely credible for the
> read → verify → tweak loop an agent platform actually needs.

Under that bar, F1–F13 are all in scope and F14's debugger is not. The one place this
review argues for real *new* subsystem investment is an LSP client (`051` Wave 3) — not
for parity, but because diagnostics are the highest-value context an agent can be given,
and CID currently gives its models text with no compiler truth attached.

## Failure Modes

The specific way this document could be wrong: it verifies *reachability and shape*, not
*runtime behavior under load*. It did not exercise a real Session against a live model
(no API key in this environment — §5's long-standing gap, still open), did not launch the
Tauri desktop shell, and did not test on mobile hardware. Those three remain honestly
unverified, exactly as `048-Platform-Verification.md` records.

## Security

F1 is the security finding. F3 is a durability finding with security-adjacent
consequences. Everything else in S2/S3 is correctness, reachability, or quality.

## Testing

Baseline table above. `051` states the test required for each fix.

## Implementation Order

See `051-Editor-Excellence-Roadmap.md`. Summary: F1–F3 first (defects), then F4–F7 (truth
in documentation and dead surface), then the editor and reachability waves.

## Acceptance Criteria

This document is accepted when every finding either has a fix with a regression test, or
an explicit written decision not to fix it with the reason recorded — the same standard
`review_prompt.md` §9 set.

## AI Coding Rules

Do not close a finding here by editing this document. Close it by changing the code, then
update the row. If a finding turns out to be wrong, say so and delete it with a note —
per §9, a corrected finding is a good outcome. F6's `context_engine.toggle` correction is
the worked example.
