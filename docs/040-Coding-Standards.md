# 040 — Coding Standards

## Vision

Consistent, low-ceremony conventions actually observed across this codebase, described
from the real code rather than an idealized style guide.

## Goals

**Rust**:
- `anyhow::Result` for fallible functions across managers; `anyhow::bail!`/`anyhow!` for
  error construction with a clear message naming what went wrong (`required_str`'s
  pattern: "X is required", not a generic serde error).
- Comments explain *why*, not *what* — the WHY-only comment discipline is visible
  throughout `sandbox/mod.rs`, `confidence/mod.rs`, and every module written or corrected
  in this project's later phases; earlier Phase 0/1 code is less consistent about this,
  a real gap worth closing incrementally rather than in one pass.
- Tests live in `#[cfg(test)] mod tests` inside the module they test, named descriptively
  as full sentences (`an_unrelated_agents_md_rule_does_not_flag_an_unrelated_patch`) —
  the test name states the claim being verified, so a failure is legible without reading
  the test body.
- `cargo fmt` and `cargo clippy -D warnings` enforced in CI for `cid-core` (not yet for
  `cid-tui` — a real gap, see `036-CI-CD.md`).

**TypeScript**:
- Functional React components, hooks for state (`useCid`, `useState`), no class
  components.
- Tailwind utility classes inline, no separate CSS-in-JS layer.
- `src/lib/api.ts`'s `CidApiClient` is the single point of RPC access — components never
  construct a `fetch`/WebSocket call directly.

## Non-Goals

A formal linter-enforced style guide beyond `cargo fmt`/`clippy` and `eslint` — no custom
lint rules were written for this project.

## Architecture

N/A.

## Tradeoffs

Comment discipline is inconsistent across phases (see Goals) — a real, honest
observation rather than a claim of uniform quality throughout.

## Failure Modes

N/A.

## Security

Never construct a shell command via string interpolation of untrusted input — see
`sandbox/mod.rs`'s command-path-policy design and `redact/mod.rs`'s pattern-based secret
scrubbing for the two places this matters most.

## Testing

`cargo clippy -p cid-core --all-targets -- -D warnings` and `npm run lint` are the
mechanically-enforced parts of this document; the rest (comment discipline, naming) is
convention, not tooling.

## Implementation Order

N/A — describes current, not phased, practice.

## Acceptance Criteria

New code passes `cargo clippy -D warnings` and `npm run lint` without new suppressions
added to work around a real issue.

## AI Coding Rules

Match the surrounding file's existing style rather than introducing a new pattern for
the same problem — this codebase already has an established idiom for RPC handlers,
error messages, and test naming; use it.
