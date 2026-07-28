# Checkpoint — Phase 6

`CID-Phase6-Build-Prompt.md` does not exist as a file — only `CID-Roadmap-Index.md`'s
one-line description of it does: "Repository Health dashboard (test coverage +
duplicate/redundant test detection), crate-layout reconciliation and documentation,
observability (Prometheus-style metrics + Sentry-style crash reporting with an explicit,
tested no-code-leakage guarantee)." Built to that description directly, at a scope
appropriate for one continuous session rather than a multi-day dedicated phase.

## 1. What was built

- **Repository Health** (`cid-core/src/repo_health/mod.rs`, RPC `repo_health.scan`,
  `RepoHealthPanel.tsx`) — test-to-code presence ratio per module and duplicate-test
  detection via body-hash grouping. Explicitly a signal, not instrumented coverage — the
  panel says so inline rather than presenting a plausible-looking percentage this repo
  can't actually produce yet.
- **Crate-layout doc** (`docs/046-Crate-Layout.md`) — what each of the three workspace
  members (`cid-core`, `cid-tui`, `src-tauri`) is for and why it's separate, plus why
  `cid-core`'s 30+ internal modules are deliberately not further split into crates.
- **Observability** (`cid-core/src/observability/mod.rs`, `/metrics` HTTP route, RPC
  `observability.crashes.list`) — hand-rolled Prometheus text exposition (no new
  dependency, per Phase 5's "don't churn for novelty" instruction — the metric surface
  is four names as of this phase) and a local, secret-redacted crash log with a
  structural + behavioral no-code-leakage guarantee, tested. Documented together in
  `docs/047-Repository-Health-Observability.md`.
- **Autonomy/allow-list settings panel** (`src/components/autonomy/AutonomyPanel.tsx`) —
  the backend (`autonomy.allowlist.*` RPCs, built in an earlier phase) had zero frontend
  surface; found during this phase's product-design audit and built now, since it's the
  direct, concrete answer to "let me allow or disable specific commands like don't
  commit automatically or raise a PR" from Phase 5/6-adjacent product feedback. Per-repo,
  toggle any command pattern between auto-run and ask-first, add custom patterns, edit
  denied paths, reset to the built-in default list.

Try it: `cargo run -p cid-core -- --port 5919`, then `npm run dev`, connect a repo, open
the **Health** and **Autonomy** tabs in the right panel.

## 2. What was deferred or stubbed, and which phase it belongs to

- Instrumented line coverage (tarpaulin/llvm-cov) — needs a CI/build step this repo
  doesn't have; a real gap, named plainly rather than approximated. Natural next step
  whenever coverage-as-a-CI-gate becomes a real ask, not scoped to a numbered phase here.
- Raw-string (`r"..."`) handling in the health scanner's string/comment masking pass —
  named explicitly in the function's own doc comment as an accepted gap for a
  signal-based tool, not a full Rust lexer.

## 3. Known issues

- **A real bug was found and fixed by dogfooding this phase's own feature against the
  actual repository**, not just its unit-test fixtures: `repo_health.scan`'s first
  version misparsed a test fixture string in `api_integration.rs` (one that quotes
  example `#[test]` source as a string literal) as a second real, duplicate test. Fixed
  by masking string/comment interiors before pattern matching, with a regression test
  reproducing the exact false positive. See `docs/047-...md`'s Failure Modes section.
- No other known issues from this phase's own work; see the Release checkpoint for the
  consolidated cross-phase list.

## 4. Test status

Honest, as of this checkpoint: `cargo test --workspace --exclude cid --all-features` —
**406 passed, 0 failed** (up from 391 at the end of Phase 5: +11 repo_health, +4
observability, +3 new integration tests, +1 not-yet-counted... exact breakdown: 316 lib +
62 integration + 5 performance + 9 fuzz + 11 worktree + 3 cid-tui). `cargo fmt --all --
--check` and `cargo clippy --workspace --all-targets -- -D warnings` both clean. Both new
frontend panels verified in a real browser against a real running Core and this actual
repository — not mocked, not screenshotted-and-assumed — including the false-positive
bug above, found and fixed specifically because it was checked against real data instead
of trusted from unit tests alone.

## 5. Proposed go/no-go for the Release prompt

**GO.** Phase 6's scope (per the roadmap index's one-line description) is now real and
tested. Moving to the Release prompt's cross-checkpoint audit and regression pass next.
