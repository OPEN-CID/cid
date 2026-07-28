# 036 — CI/CD

## Vision

Every PR builds and tests all components on Windows, macOS, and Linux before merge — the
real `.github/workflows/ci.yml`, not an aspirational description of one.

## Goals

Current jobs (`.github/workflows/ci.yml`, triggered on push/PR to `main`):

- `lint-rust` — `cargo fmt --check`, `cargo clippy -p cid-core --all-targets -D warnings`
- `test-rust-linux` / `-windows` / `-macos` — `cargo test -p cid-core --lib --all-features`
- `lint-frontend` — `npm run lint`
- `test-frontend` — `npm run test`
- `typecheck-frontend` — (continues past the shown excerpt)

## Non-Goals

Automated release/publish pipelines — not built in Phases 0–5; releases are tagged
manually per phase (`v0.1.0` through the current tag), not pushed by CI.

## Architecture

Six-plus parallel jobs, one workflow file, GitHub Actions.

## Tradeoffs

**Fixed during Phase 5's contributor-experience pass** — this section originally
documented a real, found gap: CI's Rust test jobs ran `cargo test -p cid-core --lib
--all-features` only, excluding `cid-core/tests/*.rs` (81 integration/fuzz/property/
performance tests) and the `cid-tui` crate entirely. `lint-rust`'s clippy step also
checked `cid-core` only. Both are now `cargo test --workspace --exclude cid
--all-features` and `cargo clippy -p cid-core -p cid-tui --all-targets -- -D warnings`
respectively — `cid` (the Tauri package) is excluded from the test/clippy run since it
needs system webview dependencies already covered by the separate `build-tauri-*` jobs,
not because it's untested.

**Also found during this same pass**: `cargo fmt --check` and `cargo clippy -p cid-core
--all-targets -- -D warnings` — both already claimed as CI-enforced — were actually
failing against the repository as it stood (32 files needed reformatting; ~30 real
clippy findings across dead code, style, and Windows-FFI-naming lints). Both were fixed:
formatting applied workspace-wide, and every clippy finding was either corrected (dead
code removed, `unwrap`-after-`is_some` replaced with pattern matching,
`field_reassign_with_default` sites rewritten as struct literals, a manual prefix-strip
replaced with `strip_prefix`) or given a narrow, justified `#[allow]` where the lint
doesn't apply (Windows FFI type names that must match the real Win32 API spelling,
functions with many arguments that are genuinely that many independent pieces of data,
struct fields kept for API-response shape-completeness). See the corresponding commit for
the full list; `cargo fmt --check` and `cargo clippy --workspace --all-targets -D
warnings` both pass cleanly as of this fix.

## Failure Modes

Both gaps above are now closed and verified locally with the exact commands CI runs. The
general lesson stands as a caution for future changes: a claim in a doc or a CONTRIBUTING
guide that a check "must pass" is only true if someone has actually run it recently — this
project's own history now includes two real instances (this one, and the sandbox/access
findings in `031-Security.md`) of a stated guarantee turning out to be false until
re-verified.

## Security

CI runs on GitHub-hosted runners with no elevated credentials beyond `checkout`/build
tooling — no deployment secrets or production access from CI as currently configured
(consistent with no automated release pipeline existing).

## Testing

See Tradeoffs — the CI-vs-local test-command gap is the one honest finding in this
document.

## Implementation Order

Established in Phase 2 (cross-platform matrix added), unchanged in scope since — the gap
named above predates and survives through Phase 4's additions, since new test files were
added to `cid-core/tests/` without updating CI's test command to include them.

## Acceptance Criteria

Not currently met: "every PR builds and tests all components." The real acceptance
criterion as of this document is narrower — every PR builds and unit-tests `cid-core`'s
lib target plus the frontend, on three platforms.

## AI Coding Rules

If you extend CI to close the gap above (`cargo test --workspace` instead of
`-p cid-core --lib`), update this document's Goals and remove the Tradeoffs/Failure Modes
entries describing the gap — don't leave a stale "known gap" note next to a CI config
that no longer has it.
