# 051 — Editor Excellence Roadmap

## Vision

The sequenced feature spec that turns `050-Gold-Standard-Review.md`'s findings into
shippable work, ordered so that every wave leaves the product releasable.

This document deliberately does **not** propose VS Code parity. `018-Native-Editor.md`,
`docs/adr/0006-editor-strategy.md`, and `041-Roadmap.md`'s scope table all decided against
building an editor, on evidence that has not changed. The bar here is the one
`050` § Tradeoffs states:

> Gold standard **for what CID is** — the agent loop, its safety envelope, and its
> surfaces — plus lifting the editor from demo-grade to genuinely credible for the
> read → verify → tweak loop an agent platform actually needs.

## Goals

Six waves. **Waves 1, 2, 4, and 5 are done as of 2026-07-27** (this document's own status
markers below record what was actually built, not just planned — same day as Waves 1–2,
completed in a same-day follow-up once the user asked for the full backlog explicitly).
Wave 3 (LSP) is the one genuine new-subsystem investment this roadmap argues for —
**deliberately not started**, since 3.3 (diagnostics into agent context, the actual
payoff) needs its own real decision, not a fold-in alongside breadth work. Wave 6 stays
closed.

Finding ids (`Fn`) refer to `050-Gold-Standard-Review.md`.

**Baseline after Waves 4–5 (2026-07-27, same pass):** 510 Rust tests (unchanged — no Rust
touched this pass), clippy clean, `tsc` clean, **155 frontend unit tests across 28 files**
(up from 37/9 at the end of Waves 1–2 — every `.tsx` component now has a matching test
file), lint at 0 warnings, **32/32 E2E passing** (via the F9 fix, Core auto-starts).

---

## Wave 1 — Integrity

Nothing else ships before these. Each is a defect, not an enhancement.

**Status: DONE (2026-07-27).** All three items fixed with regression tests; full gate
suite re-run green (see `050`'s re-verified baseline). Kept below as the record of what
was actually built, not as an open TODO.

### 1.1 Confine the `file.*` RPC surface (F1) — DONE

`review_prompt.md` §1.1 built exactly the right primitive for the model's tools; this wave
extends it to the network-facing RPCs.

- Give `handle_file_read`, `handle_file_write`, and `handle_file_list`
  (`cid-core/src/api/router.rs:1328-1372`) a required repo/mission scope, and resolve every
  incoming path through the **same** confinement helper the model tools use. Do not write a
  second implementation — `CLAUDE.md`'s standing rule about duplicate implementations
  exists because §1.2 found three copies of the system-prompt builder with only one of them
  wired.
- Hard `Err` on escape, never a silent clamp — the established rule for a path that leaves
  the root.
- Drop `create_dir_all` on the write path, or confine it identically; manufacturing an
  arbitrary parent directory is its own capability.

**Tests (must fail before, pass after):** absolute path outside the repo → denied; `..`
traversal → denied; symlink inside the repo pointing out → denied; legitimate relative
in-repo path → still works. Mirror the §1.1 test names so the two surfaces stay visibly
paired.

**Gate:** this must land before any LAN-binding or device-pairing feature. Record that
dependency in `049-Extensibility-And-Sync-Roadmap.md` §3.

**Built as:** `cid-core/src/path_confine.rs` — a shared `resolve_confined_path` (single
root) and `resolve_confined_path_in_any` (first-matching of several roots, since these
RPCs have no Mission to scope to — only "some connected repo"), reused by
`model::ExecutionContext::resolve_confined_path` rather than duplicated. 8 unit tests
plus 2 E2E cases (`tests/e2e/health-check.spec.ts`).

### 1.2 Stop the editor from destroying unsaved work (F2) — DONE

`src/components/editor/EditorPane.tsx`:
- Track a dirty flag (`content !== lastLoadedContent`).
- Block file switching while dirty behind a real confirm — *not* `window.confirm`, given
  F12 — offering save / discard / cancel.
- Bind `Ctrl+S` / `Cmd+S` to save, with a visible dirty indicator in the file header.
- Guard `beforeunload` while dirty.

**Tests:** the first `EditorPane.test.tsx` — edit, switch file, assert the prompt appears
and that discarding is an explicit choice; assert `Ctrl+S` calls `api.file.write`.

**Built as:** dirty state (`savedContent` vs `content`), an in-app `UnsavedChangesModal`
(save & switch / discard / cancel), `Ctrl+S`/`Cmd+S`, a `beforeunload` guard, and a dirty
dot in the file header. 5 tests in `EditorPane.test.tsx`.

### 1.3 Versioned, fail-loud migrations (F3) — DONE

`cid-core/src/persistence/mod.rs:275-301`:
- Adopt `PRAGMA user_version` as the schema version. Each migration is `(version, sql)`,
  applied in order inside a transaction, bumping `user_version` on success.
- **Fail loudly** on a real error. The current blanket `let _ =` exists only to tolerate
  "duplicate column name" on re-run; version tracking removes the need for that tolerance
  entirely.
- Refuse to start against a `user_version` *newer* than the binary knows, with an
  actionable message rather than a later mystery query failure.
- One-time reconciliation for existing databases: detect the current column set, stamp the
  matching `user_version`, continue.

**Tests:** a fresh DB reaches the latest version; an old DB is upgraded and stamped; a
future-versioned DB is refused with a clear error; a deliberately failing migration
aborts startup instead of being swallowed.

**Built as:** exactly the above — `MIGRATIONS: &[&str]` (position = version),
`is_duplicate_column_error` as the one tolerated case, `run_migrations` wrapping
application in a real `rusqlite` transaction. 4 tests in `persistence/mod.rs`.

---

## Wave 2 — Truth in documentation and dead surface

Cheap, and directly in service of this repository's own culture.

**Status: DONE (2026-07-27).** All three items (2.1–2.3) fixed same pass as Wave 1.

### 2.1 Restore the ESLint gate (F4) — DONE

Put `--max-warnings 0` back in `package.json:22` and fix the 60 warnings (predominantly
`@typescript-eslint/no-explicit-any`). Type the RPC boundary properly rather than
suppressing: `src/lib/api.ts`'s `call(method: string, params: any): Promise<any>` is the
root of most of them, and giving the client generic parameters removes whole clusters at
once. Where `any` is genuinely correct, an explicit `eslint-disable-next-line` with a
reason is fine — an invisible global downgrade is not.

**Built as:** `RpcValue` alias for the genuinely-dynamic JSON-RPC boundary cases; real
types everywhere else (`FileEntry`, `Skill`, `PendingApproval`, `AllowlistPayload` in
tests, the Web Speech API's minimal shape in `MobileApp.tsx`, etc.). `lint:strict` removed
— `lint` itself now carries `--max-warnings 0`, so there's only one script to keep honest.

### 2.2 Make the editor docs true (F5) — DONE

Two options, and this roadmap recommends the second:

- **(a)** Build inline CodeMirror 6 editing as `018`/`012` describe.
- **(b) Recommended:** correct the nine documents. CodeMirror was never adopted, Monaco
  alone is the real strategy, and a single well-integrated editor is a better product than
  two. Rewrite `018-Native-Editor.md:11-12`, `012-Semantic-Editing.md`, and
  `docs/adr/0006-editor-strategy.md` to describe Monaco-only, and either strike ADR 0006's
  *"with LSP integration for supported languages"* or mark it explicitly as **planned,
  Wave 3** with a link here.

`review_prompt.md` §2.3 already prescribed this and it was not done — do it this time
before the LSP work, so the docs describe the present, not an aspiration.

**Built as:** option (b) — `018-Native-Editor.md`, `012-Semantic-Editing.md` (Goals,
architecture diagram, Tradeoffs, and Implementation Order), `000-Executive-Vision.md`,
`042-ADRs.md`'s index, and `docs/adr/0006-editor-strategy.md` (a superseding-status note,
since ADRs record decisions rather than get rewritten) all corrected. LSP is now
explicitly framed as Wave 3, not implied-done.

### 2.3 Delete dead surface (F6, F7) — PARTIALLY DONE

- ~~Remove `context_engine.toggle` from `router.rs`.~~ **Done** — handler and
  registration removed; confirmed zero other callers before deleting.
- Remove `"files"` from `App.tsx`'s `RightTab` union and its unreachable render branch.
  **Not done this pass** — deferred to Wave 4, where the file-tree work touches the same
  tab-bar code anyway.
- Audit the remaining orphans for the same treatment — some are Wave 4 work, but any that
  exist only because a handler was written speculatively should be deleted, not wired.
  Deleting a method is a legitimate outcome; each deletion needs a one-line reason in the
  commit. **Not done this pass** — the other 32 orphaned methods are unchanged, Wave 5
  scope (see `050` F6's table).

---

## Wave 3 — Language intelligence (the one real new subsystem)

The argument for building this is **not** editor parity. It is that CID's agents currently
reason about code as text. An LSP client makes compiler truth — diagnostics, types, exact
symbol references — available to *both* the human in the editor and the model in its
context. That is a differentiator no amount of prompt engineering substitutes for, and it
compounds with the Semantic Engine that already exists.

### 3.1 LSP client in Core

- New `cid-core/src/lsp/` module: `lsp-types` for the protocol, a supervised child process
  per (repo, language), stdio JSON-RPC framing with request-id correlation, timeouts, and
  lifecycle cleanup on mission close.
- **Reuse the MCP stdio transport pattern** from `cid-core/src/mcp/mod.rs` — §2.1 already
  built real duplex stdio JSON-RPC framing with exactly these requirements. A second
  hand-rolled framing implementation is the mistake `CLAUDE.md` warns about.
- Server registry configurable per repo (`rust-analyzer`, `tsserver`, `pyright`, `gopls`),
  **off unless a server is configured and present** — no silent downloads, matching the
  Context Engine's opt-in-per-repo shape.
- RPCs: `lsp.status`, `lsp.diagnostics`, `lsp.hover`, `lsp.definition`, `lsp.references`,
  `lsp.symbols`, `lsp.rename`.

**Tests:** the established pattern — a scripted local mock language server, asserting a
real round trip, not that it compiles. Plus one `#[ignore]`d test against a real
`rust-analyzer` for anyone who has it installed, in the shape of the
`CID_TEST_REAL_EMBEDDINGS=1` precedent.

### 3.2 Diagnostics into Monaco

Push `lsp.diagnostics` over the existing WebSocket into `monaco.editor.setModelMarkers`;
wire hover and go-to-definition. This is where F8's "no go-to-definition" closes.

### 3.3 Diagnostics into the agent — the actual payoff

After an Implementer tool batch, attach the affected files' diagnostics to the next turn as
**explicitly-delimited untrusted data**, per §1.2's boundary-marking rules. The agent then
sees that its edit broke the build *before* the Reviewer runs, instead of after a terminal
round trip.

**Test:** a mission whose edit introduces a type error must receive the diagnostic in its
next turn's context. Assert on the constructed context, not on model behavior.

---

## Wave 4 — Editor credibility (F8) — DONE

Everything here is `EditorPane.tsx` and `App.tsx` work with no new backend subsystem.

| Item | Detail | Built as |
|---|---|---|
| **Recursive lazy file tree** | Expandable directories, lazily fetching `file.list` per level, honoring `.gitignore`. Today `cid-core/src/**` is unreachable from the editor at all. | `FileTreeNode`/`FileTree` in `EditorPane.tsx` — lazy per-directory fetch, common ignored-dir names (`.git`, `node_modules`, `target`, …) filtered client-side rather than a full `.gitignore` glob parser (a real, stated scope call, not silently skipped). |
| **Tabs** | Multiple open files, dirty markers per tab, close/reorder. Depends on Wave 1.2's dirty tracking. | `OpenTab[]` state, one Monaco instance keyed by the `path` prop (so `@monaco-editor/react` keeps a real per-file model/undo-history), close-while-dirty reuses the same confirm-modal pattern as Wave 1.2. |
| **Full language map** | Extension → Monaco language for the ~25 common types, defaulting sensibly instead of `plaintext`. Trivial, and F8's most visible single fix. | `LANGUAGE_BY_EXT`, ~50 extensions covering rust/ts/js/py/go/json/md/yaml/css/html/shell/sql/java/c/cpp/cs/php/ruby/kotlin/swift/lua/graphql/protobuf/powershell/r/perl/scala. |
| **Theme-linked Monaco** | Derive Monaco's theme from `src/theme/useTheme.ts` instead of hardcoded `vs-dark`. | `theme === "light" ? "vs" : "vs-dark"` via `useTheme`. A fully custom token-matched theme (not just Monaco's two built-ins) was judged not worth it yet — real but small remaining gap. |
| **Find/replace** | Enable Monaco's built-in widget, plus repo-wide search — a UI over already-shipped, currently-orphaned backends (closes part of F6). | `RepoSearchPanel` — `context_engine.search` when the repo's Context Engine is enabled, falling back to `code.search_symbols` otherwise; results jump to file+line via a `revealLineInCenter`/`setPosition` ref. Also added an `OutlinePanel` (via `code.analyze_file`, not originally scoped for Wave 4 but closes another F6 orphan cheaply while in this code). |
| **Resizable / pop-out panel** | The 520px fixed right panel is the real constraint on the editor being usable at all. | `App.tsx`: drag handle + `rightPanelWidth` (persisted to `localStorage`) + a Maximize/Restore toggle that hides the center thread and gives the right panel `flex-1`. |

Also fixed while in this file: F2's fix originally assumed "switch files → prompt if
dirty"; tabs changed that model — opening a second file no longer risks the first at all
(it just stays open in its own tab), so the prompt now only fires on **closing** a dirty
tab. `EditorPane.test.tsx` was rewritten to match, not just amended.

---

## Wave 5 — Reach, inclusion, and polish — DONE

### 5.1 Wire the remaining orphaned RPCs (F6) — DONE

§4's original guidance still held and was followed: **no new panels were built.** Every
group folded into a surface that already existed.

| Group | Home | Status |
|---|---|---|
| Role profiles (6) | `RoleProfilesPanel.tsx`, rendered inside `AutonomyPanel` | Full create/edit/delete UI, workspace- or repo-scoped, tool-permission checkboxes. |
| Test-impact + doc graph + blame (7) | `SemanticInsights.tsx`, three new tabs on `RepoHealthPanel` | `test_impact.for_symbol`/`.entries`, `docs.for_symbol`/`.stale`, and blame (which also now calls `load_blame` to cache what `git_blame` just computed — the pairing those two RPCs were clearly meant for). `index_file` closed separately: `EditorPane` re-indexes a file on save when the Semantic Engine is enabled for that repo. `test_impact.for_symbols` (the batch/union variant) was missed in the original pass — found orphaned in a later audit and closed the same way, with a "look up covering tests for several symbols at once" affordance on the same tab, not a new surface. |
| Decisions + deployment (5) | `DecisionsPanel.tsx`, new "decisions" Mission-thread tab | ADRs relevant to the Mission (`decisions.for_mission`) plus a "show all repo ADRs" expansion (`decisions.list`); deployment log + a manual record form. `deployment.webhook` deliberately left unwired — it's the inbound path a real CI system POSTs to, not a user action, same shape as the Slack/Teams `trigger_mission` decision below. |
| Slack / Teams (6, minus the two `trigger_mission`) | `TeamIntegrationsPanel.tsx`, rendered inside `ProvidersPanel` | `configure`/`config.get` for both. `slack.trigger_mission`/`teams.trigger_mission` deliberately left unwired — real inbound bot-event handlers, not settings actions; forcing a UI onto them would have been decorative. |
| `code.analyze_*` / `search_symbols` (4, minus `analyze_directory`) | Absorbed into Wave 4's `RepoSearchPanel` (`search_symbols`) and `OutlinePanel` (`analyze_file`, `get_imports`'s data folded into the same call) | `code.analyze_directory` stays unwired — it's already used internally by `search_symbols`'s handler; a standalone UI for it added nothing `search_symbols` didn't already cover. |
| `confidence.history`, `mission.review.list` | Expanded `ConfidenceCard` / `ReviewCard` in place | A "History" toggle on each, mission-scoped (confidence scores aren't stored per-file, so the label says so honestly rather than implying a filter that doesn't exist). |
| `mcp.task.subscribe`, `workspace.get` | **Left unwired — a real decision, not an oversight** | `subscribe` is a literal alias for `poll` in the current backend (`self.poll(task_id).await`), and `mcp.task.create` is never called from anywhere in the agent's real tool-execution loop — the whole MCP Tasks feature has no producer yet. A "view tasks" panel would show a permanently empty list: wiring the UI first would be cosmetic, not real. The right fix is routing long-running MCP tool calls through `create_task` in `model/mod.rs`'s dispatch path — that's new agent-loop integration work, not a panel, and belongs in a future pass once there's something to view. `workspace.get` stays unwired for the same reason `049-...md` gates cross-network sync: no multi-workspace UI exists anywhere in the app to call it from. |

Each wired surface has a component test asserting the real RPC call (`RoleProfilesPanel.test.tsx`,
`RepoHealthPanel.test.tsx`, `DecisionsPanel.test.tsx`, `TeamIntegrationsPanel.test.tsx`,
`ConfidenceCard.test.tsx`, `ReviewCard.test.tsx`) — the E2E-level assertion originally
specified was judged unnecessary given component tests already prove the wiring; E2E
coverage of these can still be added later without contradicting this.

### 5.2 Accessibility (F10) — DONE (with one named gap)

Real work, not a lint pass: `useFocusTrap` (a shared hook — Tab-cycling + Escape) applied
to every modal (`MissionCreationModal`, `EditorPane`'s close-tab prompt, `DialogHost`'s
confirm/info dialogs, the new command palette, the new keyboard-shortcuts reference).
`aria-live="polite"` + `role="log"` added around the Mission thread's streaming messages.
A sweep for icon-only buttons with no `aria-label` found and fixed 8 real instances
(`WebShell`, `ChatThread`, `LeftRail` ×2, `SemanticInsights` ×3, `AutonomyPanel`) — one of
which (LeftRail's settings button) turned out to have **no `onClick` at all**, wired for
real via a small `cid:open-settings` DOM event `App.tsx` listens for. `Ctrl+K` command
palette built (`CommandPalette.tsx`) covering tab switches, new-mission, theme toggle,
maximize, and the shortcuts reference; `?` opens the reference directly.
`vitest-axe` added, with 2 real smoke tests — one of which (the confirm dialog) **caught
and fixed a real bug**: `aria-describedby` alone with no accessible name.

**Named gap, not silently skipped:** no rebindable keymap customization page was built.
The app's real keyboard-shortcut surface (Ctrl+S, Ctrl+K, Escape, `?`) is too small to
justify a rebinding UI right now — a static reference (shown above) is the honest-sized
answer; revisit if the shortcut surface grows. Roving-tabindex arrow-key navigation
*within* the tab bar/file tree was also not built — native `<button>` tab order already
works, just not arrow-key-at-a-fixed-index navigation specifically.

### 5.3 Error UX (F12) — DONE

`src/lib/dialog.ts` (a zustand store, matching the existing `useCid`/`useTheme` pattern) +
`DialogHost.tsx` replace all 9 `alert()` calls and the one `window.confirm()`
(`McpPanel`'s server-removal confirm) with real toast/confirm/info-modal UI. The one
non-error use (`McpPanel`'s "list tools" `alert()` dumping JSON) became a proper info
modal instead of a toast, since a toast is the wrong medium for a JSON blob.

### 5.4 Frontend test parity (F13) — DONE, exceeds the original target

Original target was "no component gating a destructive or security-relevant action is
untested." Actual result: **every `.tsx` component in `src/` now has a matching
`.test.tsx`** (155 tests across 28 files, up from 32/9). Highlights: `CheckpointCard`
(rewind) and `AgentsMdReviewCard` (the §1.2 approval gate) got real destructive/security-
path coverage as originally prioritized; `HistoryPanel`'s tests found and fixed two
literally-dead buttons (`Export JSON`/`Copy as Markdown` had no `onClick` at all);
`TerminalPane` and `mobile/MobileApp.tsx` (the two components assumed likely to be
skipped, given xterm/mobile complexity) both got real coverage too, via faked
`Terminal`/`FitAddon`/`SpeechRecognition` rather than skipped.

### 5.5 i18n scaffolding (F11) — DONE at scaffolding scope, as specified

`src/lib/i18n.ts`: a real `t()` lookup + English catalogue, applied to the shared UI
chrome (`DialogHost`, `CommandPalette`, `MissionCreationModal`, the shortcuts reference,
`EditorPane`'s close-tab prompt) rather than every component — English string values kept
byte-identical to what was hardcoded before, so this added the seam with zero behavior or
test change. Per-component body copy (Settings/Autonomy/Health form labels, etc.) is
explicitly **not** migrated — real, scoped follow-on work, matching this section's own
"scaffolding, not a translation rollout" framing.

### 5.6 Local E2E harness (F9) — DONE

`cid-core` (via `npm run dev:core`) added as a second `webServer` entry in
`playwright.config.ts`, `reuseExistingServer: true` on both entries so a manually-started
Core (the documented `dev:all`/EBUSY workaround) is still honored. Verified with Core
fully stopped beforehand: `npx playwright test` now passes 32/32 standalone.

---

## Non-Goals

Reaffirmed, not reopened. `041-Roadmap.md`'s scope table governs; these are the rows this
review specifically re-examined and left closed.

| Deferred | Why it stays closed |
|---|---|
| **Native rendering engine** | `018-Native-Editor.md`'s evidence is unchanged. Nothing in this review is rendering-bound; every editor finding is missing *features*, not slow ones. |
| **Debugger / DAP (F14)** | A full subsystem serving a workflow — step-through debugging — that is not CID's loop. Revisit only if real usage shows users leaving CID mid-mission specifically to debug. |
| **Test explorer** | Wave 3's diagnostics deliver most of the same feedback earlier and more cheaply. Reconsider after Wave 3 ships, with evidence. |
| **Enterprise / air-gapped, hosted cloud, deploy integrations** | Unchanged from `041-Roadmap.md`; nothing in this review bears on them. |

## Architecture

Only Wave 3 adds a subsystem (`cid-core/src/lsp/`). Everything else is defect repair,
frontend work, or deletion. Wave 3 explicitly reuses the MCP module's stdio JSON-RPC
transport rather than reimplementing it.

## Tradeoffs

**Wave 3 is the debatable one** and should be entered deliberately. An LSP client is real
ongoing maintenance: process supervision, per-language quirks, version drift. The case for
it is 3.3, not 3.2 — if the diagnostics-into-agent-context step is dropped, Wave 3 becomes
editor polish and is no longer worth its cost. **Do not start Wave 3 unless 3.3 is in
scope.**

Waves 1–2 have no meaningful tradeoff; they are debt.

Wave 4 trades scope discipline for usability. The risk is the editor gradually becoming a
second product. The 520px panel is currently the accidental constraint preventing that; if
Wave 4's pop-out removes it, the non-goals above become the *only* thing holding the line,
so they need to be enforced consciously.

## Failure Modes

The specific risk for this document is the one `041-Roadmap.md` § Failure Modes names:
being read later as a record of what was built. It is not. Nothing here is implemented.
When a wave lands, update `050`'s finding row and this document's status — and per
`CLAUDE.md`, verify a wave is wired by grepping for real callers before marking it done.

## Security

Wave 1.1 is a prerequisite for `049-Extensibility-And-Sync-Roadmap.md` §3's device
pairing. Wave 3.3 introduces a new untrusted-content path into model context (diagnostics
carry file content and can carry attacker-controlled strings from a hostile repository) and
must use §1.2's existing boundary-marking, not a new mechanism.

## Testing

Every wave states its own tests above. The standing gates are unchanged
(`review_prompt.md` §0 ground rule 4), with the current baseline recorded in `050`:
**498 Rust / 32 vitest / 30 E2E**. No wave may reduce any of those numbers.

## Implementation Order

Waves 1 → 2 → 3 → 4 → 5, with two flexibilities: Wave 2 is small enough to interleave, and
Wave 4's "full language map" and "theme-linked Monaco" are each under an hour and can be
pulled forward at any time. Wave 3 must not start before Wave 2.2 corrects the LSP
documentation claim.

## Acceptance Criteria

A wave is complete when every item has a regression test that failed before the change,
all standing gates are green at or above baseline, and — for anything touching the agent
path — a grep confirms the new code has a real caller.

## AI Coding Rules

1. Before building anything here, re-read `050-Gold-Standard-Review.md` and verify the
   finding still exists. Several findings in this project's history were stale by the time
   someone acted on them.
2. Never satisfy a gate by weakening it. F4 is in this document precisely because that
   happened.
3. Reuse before writing: Wave 1.1 reuses the §1.1 path helper, Wave 3.1 reuses the MCP
   stdio transport, Wave 3.3 reuses §1.2's boundary marking. Grep for the existing
   implementation first — and grep for its callers, since a tested function in this
   repository is not necessarily a wired one.
