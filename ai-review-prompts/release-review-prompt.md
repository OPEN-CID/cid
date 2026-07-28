# Release review — v1.0.0 as a whole

Source of truth: `docs/RELEASE-REPORT-v1.0.0.md`, `CHANGELOG.md`, `README.md`,
`docs/041-Roadmap.md`.

## Claims to verify

1. **The consolidated known-issues list has a real disposition for every item**, not a
   silent drop. Cross-reference every "Known issues" section across
   `docs/CHECKPOINT-Phase0.md` through `docs/CHECKPOINT-Phase6.md` against
   `docs/RELEASE-REPORT-v1.0.0.md`'s disposition table. An item that appears in a phase
   checkpoint but has no corresponding row (fixed / tracked / accepted) in the Release
   Report is exactly the failure mode the Release prompt was written to prevent.
2. **"Fixed" dispositions are actually fixed** — re-run the specific test or reproduction
   named for each one, don't take the label at face value (this whole review series
   exists because that exact kind of unverified claim has been wrong before in this
   codebase).
3. **README.md describes the real, current feature set**, not Phase 0's original scope.
   Check for stale version/phase labels anywhere user-facing — `grep -rn "Phase 0"
   src/` should turn up only legitimate historical design-footnote comments (e.g. in
   `DiffViewer.tsx` explaining *when* a specific limitation originated), not a live status
   indicator like the one found and fixed in `LeftRail.tsx` during this Release pass
   (it read "Phase 0 • Co-Pilot" regardless of actual state).
4. **The full regression suite actually passes on a clean checkout**, run fresh, not
   copy-pasted from an earlier run:
   ```powershell
   cargo test --workspace --exclude cid --all-features
   npm run test
   npx tsc --noEmit
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   npx playwright install && npm run test:e2e   # needs cid-core + npm run dev running
   ```
   If `repo.connect` (or any DB-writing RPC) fails with `FOREIGN KEY constraint failed`
   against a *reused* local dev database, don't assume it's the WAL-mode fix regressing —
   first delete the local `cid.db` (path printed at Core startup, or `%APPDATA%/cid/cid.db`
   on Windows / the platform-equivalent `dirs::data_dir()` elsewhere) and retry against a
   fresh one. Only treat it as a real regression if it reproduces from a **fresh** database.
5. **Packaging claims match what was actually done.** This release was prepared without
   code-signing certificates or the ability to push a GitHub Release — check
   `docs/RELEASE-REPORT-v1.0.0.md`'s Packaging section states this plainly (CI config
   ready, signing/publishing pending real credentials and explicit human go-ahead) rather
   than claiming signed binaries exist when they don't.
6. **Nothing was committed or pushed without explicit instruction.** Check `git log` and
   `git status` — per this project's own operating constraint for this build session (see
   the session's own instructions, not a file in the repo), no commits should exist
   beyond what a human explicitly requested. If you find commits or pushes you can't
   account for, that's a process violation worth flagging loudly, not code-reviewing
   quietly.

## What "done" should mean here

Per the Release prompt's own framing: the goal isn't zero known issues, it's that every
known issue has an honest, checkable disposition. A reviewing AI's most valuable output
from this exercise is not "everything looks fine" — it's a specific claim above that
doesn't hold, with the file/test/command that proves it.
