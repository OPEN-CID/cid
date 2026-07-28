<!--
Mirrors CONTRIBUTING.md's PR checklist. Fill in each section honestly — "I ran the
tests and they passed" should be true when you write it, not aspirational.
-->

## What was built

<!-- Concrete description. Include exact commands to try it if that's not obvious. -->

## What was deferred or stubbed

<!-- Name what's left out and why. "Nothing" is a fine answer if it's true. -->

## Known issues

<!-- Real ones. "None found" is fine if you actually looked. -->

## Test status

- [ ] `cargo test --workspace` passes locally
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -p cid-core -p cid-tui --all-targets -- -D warnings` passes
- [ ] `npm run test` passes
- [ ] `npx tsc --noEmit` passes
- [ ] `npm run lint` passes
- [ ] `npm run test:e2e` passes (if this PR touches the frontend or RPC surface)

## Security-critical changes

<!-- Delete this section if it doesn't apply. -->
<!-- If this PR touches sandbox/, access/, auth/, or governance/: does it have a real
     integration test exercising the actual enforcement point, not just a unit test of
     the isolated function? See docs/031-Security.md's Failure Modes for why this
     matters. -->

## Docs

<!-- Did this change make any docs/0XX-*.md file inaccurate? Updated it in this PR, or
     explain why not. -->
