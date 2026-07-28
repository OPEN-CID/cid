# 045 — Dependency Audit (Phase 5)

A real, researched audit of Appendix A Part 18's stack table against its current state as
of July 26, 2026 — not a repeat of the table researched at project inception. Per the
Phase 5 build prompt: only change a dependency where this audit finds a concrete problem
or a clearly better, checkable alternative — not for novelty.

## Method

`cargo update --dry-run` confirms every pinned dependency is already at the latest
version *within its current semver constraint* — "Locking 0 packages to latest
compatible versions." Anything genuinely newer requires a manual `Cargo.toml` version
bump, which is what this audit evaluates case by case below. Findings are sourced from
live web search (dated July 26, 2026), not recalled from training data.

## Findings — already-decided choices, re-verified

**`git2-rs` over `gitoxide` (`gix`) for writes — still correct.** Live-searched against
`gitoxide`'s own `crate-status.md` and recent maintainer discussion: push support,
complete merge workflows, rebase, reset, and commit hooks remain listed as planned but
not fully implemented as of the current search results. Some merge-tree/merge-commit
capability has landed, but full push/rebase parity has not. ADR 0002's decision stands
unchanged.

**MCP spec target (2026-07-28) — on schedule, still the right target.** Confirmed via
live search: the release-candidate-locked spec (May 21, 2026) is finalizing July 28,
2026 — two days after this audit — as "the largest revision since launch": stateless
core, MCP Apps and Tasks as the first two official extensions, OAuth/OIDC-aligned
authorization, and a formal Active/Deprecated/Removed feature lifecycle policy (minimum
12 months deprecation notice). `cid-core/src/mcp/mod.rs` and `mcp_tasks/mod.rs` already
target this shape. **Action for a near-future pass**: do one validation pass against the
actual final spec text once it locks on the 28th, per the Phase 5 prompt's own
instruction — a release candidate can still carry last-minute changes.

**Tauri v2 mobile — maturity assessment updated, not the underlying decision.** ADR 0010
characterized Tauri v2 mobile as "not yet first-class" per the Tauri team's own mid-2025
framing. Live search shows this has genuinely moved: Tauri v2 is now described as
shipping "first-class iOS and Android support," the API is stable for both platforms as
of 2.0.0, and the project is on the active 2.11.x line (2.11.5, released July 1, 2026)
with frequent patch releases. The caveat that "not all desktop plugins are available on
mobile yet" still holds. **This does not change ADR 0010's decision** (Tauri v2 Mobile
was already selected) — it strengthens the confidence behind it, and is relevant context
for `027-Mobile.md`'s open item (no physical-device verification yet): the underlying
framework is more mature than it was characterized to be at bake-off time.

**`ratatui`** (Phase 4's CLI/TUI shell dependency) — confirmed mature and actively
maintained; no reason to look further, consistent with the Phase 5 prompt's own guidance
to spend less audit effort here.

## Findings — newer versions exist, evaluated and deferred

| Crate | Pinned | Latest found | Assessment |
|---|---|---|---|
| `rusqlite` | 0.32 | 0.40.1 (2026-06-06) | An 8-minor-version gap. No CVE or security advisory found for 0.32. Bumping this many versions at once risks undetected API drift across `persistence/mod.rs`'s ~600 lines of query code. **Deferred** — recommend a dedicated pass with its own full-suite regression run, not bundled into this audit. |
| `tantivy` | 0.22 | 0.26.1 | 4-minor-version gap. `semantic_engine/index.rs` uses specific Tantivy APIs (`TantivyDocument`, `QueryParser`, `TopDocs`) that may have changed shape across those versions. **Deferred** for the same reason as `rusqlite` — real, but not urgent, and worth its own verification budget given 22 tests depend on this module's exact behavior. |
| `axum` | 0.7 | 0.8.x | Real breaking changes exist (path-parameter syntax `/:id` → `/{id}`, `FromRequest`/`FromRequestParts` no longer `async_trait`, WebSocket `Message` now uses `Bytes`/`Utf8Bytes`). **Checked against CID's actual usage**: `create_router` (`api/router.rs`) defines exactly three routes — `/ws`, `/health`, `/api/rpc` — none use path parameters, so the highest-profile breaking change doesn't apply here. Still deferred: the extractor and WebSocket-message-type changes could affect `handle_ws`'s message handling and would need real verification, not an assumption that "no path params" means "no risk." |

None of the three above are security vulnerabilities — they are version-currency gaps.
Per the Phase 5 prompt's explicit instruction, they are not bumped in this pass; bumping
without a dedicated regression budget for each would be exactly the "churn for novelty"
this audit is told to avoid.

## Findings — real security advisory found during Release validation (added post-Phase 5)

`npm audit` surfaces a moderate-severity `dompurify` advisory (`GHSA-c2j3-45gr-mqc4`,
`GHSA-cmwh-pvxp-8882`, `GHSA-vxr8-fq34-vvx9` — `CUSTOM_ELEMENT_HANDLING`/`ALLOWED_ATTR`/
Trusted-Types sanitizer bypasses) pulled in transitively by `monaco-editor@0.56.0`
(pinning `dompurify@3.4.8`). Verified this is not fixable from CID's side right now:
`monaco-editor@0.56.0` is already the latest published version, and it is the one
pinning the vulnerable range internally — not a version CID chose. **Tracked, not
silently dropped**: `npm audit fix`/`--force` were both tried; neither resolves it
without an unreleased upstream `monaco-editor` fix. Re-check on every `monaco-editor`
bump. A `dependency-audit` CI job (`continue-on-error`, so a new advisory is visible
without blocking every PR) now runs `npm audit --audit-level=moderate` and `cargo audit`
on every PR — this specific gap was found by actually running the tool the CI job now
runs automatically, not by inspection.

## Findings — lighter pass, no red flags

React, TypeScript, Vite, Tailwind, shadcn/ui, Tree-sitter grammars, SQLite (via
`rusqlite`'s bundled feature), Tokio: no maintenance or security red flags surfaced.
`keyring` 3.5, `portable-pty` 0.8, `argon2` 0.5, `proptest` 1.x, `tokio-tungstenite` 0.26:
all current within their pinned ranges and actively maintained upstream.

## Non-Goals

Re-researching every dependency from scratch, as if the original Part 18 table didn't
exist — this audit verifies and extends prior research, per the Phase 5 prompt's own
framing, rather than repeating it.

## Recommendation

1. Do the promised MCP final-spec validation pass on or after July 28, 2026.
2. Schedule a dedicated `rusqlite`/`tantivy`/`axum` major-version-bump pass, each with its
   own full `cargo test --workspace` regression run and manual review of the specific
   breaking-change surface identified above — not bundled with unrelated feature work.
3. Update `027-Mobile.md` and ADR 0010 to reflect Tauri v2 Mobile's improved maturity
   characterization found here, without re-opening the already-settled framework choice.

## AI Coding Rules

Do not bump `rusqlite`, `tantivy`, or `axum` to the versions found in this audit without
running the full test suite immediately after and manually checking the specific
breaking-change areas named above — this document exists precisely so that check isn't
skipped.
