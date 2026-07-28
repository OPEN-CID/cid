# How to use this folder

This is **not** the original build-prompt series (`CID-Phase0-Build-Prompt.md` through
`CID-Release-Prompt.md` — the one-time bootstrap prompts used to originally build CID;
removed from the repo once superseded by the real `docs/000`-`049` series, still visible
in git history). Those described what was *asked for*, written before the code existed. These
files describe **what actually exists in this repository right now**, written by
re-reading the real code, tests, and checkpoints — specifically so a different AI model
(or a human) can use them as a falsifiable checklist to find gaps between claim and
implementation, the same way this project's own later phases found real gaps in its
earlier phases' "done" claims (see `docs/041-Roadmap.md`'s Failure Modes section for that
history — it happened at least four separate times before this series of files existed).

## Why this exists

Across this project's build history, several "already built" claims turned out to be
false on re-verification: an ACP host with zero RPC methods, a sandbox-boundary test that
was a tautology, a Confidence Engine that was never wired into `lib.rs` and therefore
never compiled into the running binary, a Settings panel that could never have correctly
saved a real API key. Each was found by someone actually running the code against real
data, not by reading a summary and trusting it. These files are written to make that
easier to repeat: each claim below names a specific file, RPC method, or test — something
checkable, not a vague assertion.

## How to review

For each phase file:

1. **Read the "Claims" section.** Each claim names a specific file/function/RPC
   method/test it's about.
2. **Check the claim directly** — read the named file, run the named test, call the
   named RPC method against a running Core. Do not accept a claim because it *sounds*
   plausible; the whole point of this exercise is that plausible-sounding claims have
   been wrong before in this exact codebase.
3. **Report findings the same way this project's own checkpoints do**: for each claim,
   say whether it holds, partially holds (and how), or doesn't hold — with the specific
   evidence (a line number, a test failure, a missing file), not just "seems fine."
4. Cross-reference `docs/CHECKPOINT-Phase0.md` through `docs/CHECKPOINT-Phase6.md` and
   `docs/RELEASE-REPORT-v1.0.0.md` — they document what was already found and fixed
   during this project's own verification passes. A finding that duplicates something
   already fixed and documented there is a false positive; a finding that contradicts one
   of them (i.e., the checkpoint says X was fixed, but the code still shows the bug) is
   the single most valuable kind of result this exercise can produce.

## Running the actual verification commands

```powershell
cargo test --workspace --exclude cid --all-features   # Rust: unit + integration + fuzz + property + perf
npm run test                                            # frontend unit tests
npx playwright install && npm run test:e2e              # E2E — needs cid-core running (see below) + npm run dev
npx tsc --noEmit
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings

cargo run -p cid-core -- --port 5919      # start Core for manual RPC/UI checks
npm run dev                                # start the web shell, http://localhost:1420
```

## Files in this folder

- `phase0-review-prompt.md` through `phase6-review-prompt.md` — one per build phase.
- `release-review-prompt.md` — the v1.0.0 release as a whole: packaging, the
  cross-checkpoint known-issues disposition, and whether the public-facing docs
  (README, CHANGELOG, roadmap) actually match the code.
