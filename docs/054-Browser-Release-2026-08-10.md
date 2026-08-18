# 054 — Browser Release Checklist (2026-08-10)

Answers one question: **can this go live in a browser today, and what do I personally
have to do?** Everything below was run for real on this machine this session — not
"should pass." It closes out the model-catalog work left uncommitted at the end of the
last session (`docs/053-Production-Readiness-Review.md` §3a/§5) and re-verifies the
whole stack on top of it.

## 1. What was pending from last session, and its status now

The only uncommitted work sitting in the tree was the live model-catalog feature
(`docs/053` §3a): `cid-core/src/model/catalog.rs`, `catalog_bundled.rs`,
`scripts/generate-model-catalog.mjs`, plus the router/persistence/frontend changes that
depend on it (folder picker, optional task description, model picker — `docs/053`'s
whole §0 scope list). Verified genuinely wired, not orphaned:

- `catalog::ANTHROPIC/OPENAI/GOOGLE` are read from `model/mod.rs`'s pricing and
  context-window lookups, and `main.rs` calls `catalog::refresh_in_background()` on
  startup — not just present in the tree unused.
- No leftover TODO/FIXME/`unimplemented!`/placeholder/simulated markers anywhere in
  `cid-core/src`, `src`, `src-tauri/src`, or `cid-tui/src` outside test files (checked
  this session by grep, then read the two new catalog files in full).

**Nothing was committed.** Per this repo's standing rule, commits happen only on
explicit request — the working tree still has the same uncommitted changes `git status`
showed at the start of this session. Say the word and I'll commit/push; until then it's
staged in your working copy only.

## 2. Gates — run fresh this session, all clean

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --exclude cid --all-features` | **550 passed**, 1 ignored (network-dependent real-embeddings test, expected) |
| `npx tsc --noEmit` | clean |
| `npm run lint` (`--max-warnings 0`) | clean |
| `npx vitest run` | **181 passed**, 29 files |
| `npm run build` (`tsc && vite build`) | clean, `dist/` produced |
| `npx playwright test` (32 E2E specs) | **32 passed**, 0 failed, real Core + real worktree + real Chromium |

The E2E run used `dev:core:e2e` (disposable `.cid-e2e/` DB per `playwright.config.ts`) —
your real `%APPDATA%/cid/cid.db` was never touched. Extra check beyond the suite: served
the actual `dist/` output with `vite preview` and confirmed the HTML and a built JS
bundle both return HTTP 200 with no missing-asset 404s.

## 3. How to actually put it in a browser today

Two options depending on what "release" means for you right now — both use the same
Core and the same `dist/` build.

### Option A — quick local run (five minutes, good for today)

```powershell
# Terminal 1 — Core, release build, loopback only (no token needed)
cargo build --release -p cid-core
.\target\release\cid-core.exe --port 5919

# Terminal 2 — production web bundle, served statically
npm run build
npm run preview        # serves dist/ at http://localhost:4173 by default
```

Open `http://localhost:4173` in your browser. This is exactly what the E2E suite and
the `vite preview` check above just proved works. First things to do in the UI:

1. Settings → Providers: add at least one API key (Anthropic/OpenAI/Google, or your
   OpenAI-compatible endpoint). Without one, Core now shows an honest "not configured"
   system notice instead of a fake simulated response (`docs/053` §1 defect #6) — you
   won't get a working Mission until a key is set.
2. Connect a repo via the folder picker (not a typed path — `docs/053` #2's dedupe fix
   only fires through `repo.connect`'s real storage path).
3. Create a Mission — task description is now optional (`docs/053` §0 item 1).

### Option B — reachable beyond your own machine (share it with a team today)

Same build, but Core needs to bind non-loopback and gets an auth token (Core refuses to
start non-loopback without one — `SECURITY.md` §2):

```powershell
$env:TOKEN = (.\target\release\cid-core.exe --generate-token)
.\target\release\cid-core.exe --host 0.0.0.0 --port 5919 --auth-token $env:TOKEN
```

Serve `dist/` from any static host (`npm run preview -- --host`, nginx, Caddy, S3 +
CloudFront, whatever you already use) and point it at that Core's `ws://<host>:5919/ws`.
Put a TLS-terminating reverse proxy in front if this leaves your local network — Core
itself still speaks plain HTTP/WS (`docs/052-Production-Deployment.md` §4 has ready-to-
use Caddy and nginx configs, including the WebSocket-upgrade headers nginx needs
explicitly). **`docs/052` is the full runbook** (persistent service, backups, monitoring,
upgrades) — read it before doing this for anyone other than yourself; the steps above
are the minimum to get a browser pointed at a real Core today, not the whole ops story.

### What "release" does *not* mean here

CID has no multi-tenant hosted backend — there's nothing to deploy to a public URL you
don't control. "Release on browser" is: build once, run your own Core, open the browser
tab. That's the actual shape of the product (`CLAUDE.md`'s Website section, `docs/052`
§0).

## 4. Known, honestly-stated gaps (unchanged by this pass, not release blockers)

- **`Dockerfile` still isn't build-verified** — no Docker daemon available on this
  machine (`HypervisorPresent: False`, no WSL). Not needed for Option A/B above; only
  matters if you specifically want the container path. See `docs/052` §1 and §9.
- **Windows has no kernel-level filesystem confinement for Autonomous mode** — command
  allow-list and path policy are real, but not a hard sandbox boundary on Windows
  specifically (`SECURITY.md`, `docs/RELEASE-REPORT-v1.0.0.md` #16). Relevant if you turn
  on Autonomous mode; not relevant to Manual/Co-Pilot use.
- **No auth-token rotation without a restart; no config file** — flags/env only. Fine at
  today's single-instance scope (`docs/052` §9).

Nothing above blocks using CID in a browser today — they're operational maturity items
for running it for a wider team over time, stated so you're not surprised by them later.
