# 054 — Browser Release Checklist (2026-08-10)

Answers one question: **can this go live in a browser today, and what do I personally
have to do?** Everything below was run for real on this machine this session — not
"should pass." It closes out the model-catalog work left uncommitted at the end of the
last session (`docs/053-Production-Readiness-Review.md` §3a/§5) and re-verifies the
whole stack on top of it.

**Update 2026-08-18 — release day, still PR #4.** Clearing the advisories before merge
turned up a defect that would have broken Option C's very first step. Both fixed in this
pass, before merge:

- **`Cargo.lock` was gitignored and absent from the repo.** The `Dockerfile`'s third
  instruction is `COPY Cargo.toml Cargo.lock ./`, and Coolify builds from a fresh clone —
  so §3 Option C's resource 1 could never have built, failing on a missing file. `docs/053`
  §4's "every `COPY` source exists" check was run against a *working tree*, which has the
  file; the repository does not. Now committed, with `.gitignore` corrected. **This is the
  same class of bug this repo keeps finding: verify against the artifact the consumer
  actually gets, not the one on your disk.**
- **`h2` 0.4.15 → 0.4.16** (RUSTSEC-2026-0258, published 2026-08-17). Worth noting *why*
  CI was green on this while a local `cargo audit` failed: with no lockfile committed,
  every build resolved its own versions, so CI happened to pick the patched release and
  this machine did not. The two only agree now that the lock is pinned.
- **npm: 4 advisories → 0** (`js-yaml` 4.3.0→4.3.1 and `nanoid` 3.3.16→3.3.18, both high;
  `dompurify` 3.4.12→3.4.13 via `monaco-editor`, moderate). Lockfile-only patch bumps.
- **Still open, unchanged:** 25 `cargo audit` warnings — `unmaintained`/`unsound` notices,
  not vulnerabilities, and not suppressed. 11 are the GTK3 bindings Tauri v2 requires on
  Linux (no upgrade exists upstream); the rest are `git2` 0.19, `lru`, `paste`, `instant`,
  and the `unic-*` family. Clearing `git2` means a major-version bump with real API churn —
  deliberately not done on release day.
- **Option C could not have worked at all, for a reason no test covered.** Core requires
  `Authorization: Bearer <token>` on `/api/rpc` *and* on the `/ws` upgrade whenever a
  token is set (`router.rs`), a container always binds `0.0.0.0` so a token is always
  mandatory there — and the browser client sent no credentials on either transport, with
  no way to. `new WebSocket(...)` cannot set request headers at all. So every hosted
  deployment would have failed with a 401 and an opaque closed socket, and Option B's
  `--host 0.0.0.0` shape was broken the same way. Invisible until now because every
  environment ever tested (local dev, Tauri, E2E) is loopback with no token.
  **Fixed this session**: Core also accepts the token as a `cid.bearer.<base64url>`
  WebSocket subprotocol — the only channel a browser controls — and the web client stores
  a pasted token in `localStorage` and sends it on both transports. `SECURITY.md` §2 has
  the table. Verified in a real Chromium against a real token-protected Core: 6/6 checks,
  including that a *wrong* token still fails.
- **A footgun in Option C's Build Variables**, for whoever hits it: `VITE_CID_CORE_PORT`
  must be *empty*, and `api.ts` reads it with `??`, so an empty string works but an
  *absent* variable falls back to `5919` and produces `wss://cid-core.opencid.dev:5919/ws`
  — which Traefik does not serve. If Coolify drops the empty value, set it to `443`
  instead; `wss://host:443/ws` is equivalent and unambiguous. The console check in
  §3 Option C's Verify step catches this.

**Update 2026-08-10, opened as PR #4**: committing this work and pushing it through real
CI (rather than only this machine) caught two things neither local testing nor the
sections below originally covered, both fixed before merge:

- `test-rust-windows`/`test-rust-macos` failed in CI on two pre-existing governance/PR
  tests — a real regression the model-catalog path-normalization work introduced at its
  *read* side (`get_repo_channel_by_path`, `governance::paths_match` compared a
  caller-supplied raw path against the now-normalized stored column and silently lost
  a session/governance check when they didn't match). Invisible on this machine because
  both sides happened to agree here; exposed by GitHub's Windows runner resolving the
  same temp directory to a different-looking path (8.3 short name) than this machine
  does. Fixed at both read boundaries with the same normalization function already used
  at the write boundary — see the commit for detail.
- Deciding the actual deployment target for Option C below (a real `https://` origin)
  surfaced that `src/lib/api.ts` hardcoded `ws://`/`http://` unconditionally, which a
  hosted TLS deployment would hit as a browser mixed-content block. Fixed with an
  explicit `VITE_CID_CORE_SECURE` build flag — see §3 Option C.

Both are folded into PR #4 as follow-up commits rather than a fresh PR, and CI was
re-verified green after each. §2's gate numbers below are this session's *first* local
run, before either fix — still accurate for what they measured (nothing they check
regressed), but the CI-only bug they couldn't have caught is the reason this update
exists.

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
§0). Option C below is *your own* private deployment on infrastructure you already run
for other projects — not a public multi-tenant CID, same distinction `docs/052` §0 draws.

### Option C — same Coolify/Oracle/Cloudflare flow as `houses`, on a subdomain

You already run Cloudflare DNS + an Oracle Cloud Always Free ARM box + Coolify for the
`houses` project (`nilaami.opencid.dev`, see that repo's `docs/08-operations.md` §A). CID
reuses the same box and the same Coolify instance rather than standing up new
infrastructure — two more Coolify resources, two more DNS records.

**Domain decision** (resolved this session — `WEBSITE-BUILD-PROMPT.md`'s original plan
put CID at the `opencid.dev` root, which conflicts with `houses` already using that root
for its own subdomain): CID gets **`cid.opencid.dev`** for the web client, consistent with
`nilaami.opencid.dev`'s pattern. `opencid.dev` root stays free for a future umbrella page.
`WEBSITE-BUILD-PROMPT.md` §0 still says "root" — update it to `cid.opencid.dev` next time
that file is touched, so it doesn't drift from the decision actually made here.

**A real fix this required, made in this session**: `src/lib/api.ts` hardcoded `ws://`
and `http://` unconditionally. A browser page served over `https://cid.opencid.dev`
cannot open a plain `ws://` connection to anything — browsers block it as mixed content,
silently, with nothing more diagnostic than a closed socket. This was invisible in every
environment tested so far (local dev, Tauri, the E2E suite) because both sides were
always plain HTTP there. Fixed with an explicit opt-in build-time flag,
`VITE_CID_CORE_SECURE=true`, rather than inferring it from `window.location.protocol` —
Core and the page can legitimately sit on different origins with different schemes, so
guessing from the page's own scheme would be wrong exactly when it matters. `.env`/build
vars for a hosted deploy: `VITE_CID_CORE_HOST=cid-core.opencid.dev`,
`VITE_CID_CORE_PORT=` (empty — Traefik terminates on 443, not 5919),
`VITE_CID_CORE_SECURE=true`.

**DNS** (Cloudflare, same zone `houses` already manages):

| Type | Name | Target | Purpose |
|---|---|---|---|
| A | `cid` | `<Oracle VM public IP>` | Web client |
| A | `cid-core` | `<Oracle VM public IP>` | cid-core JSON-RPC/WS |

**Coolify resource 1 — cid-core**:

1. Projects → New Resource → Public Repository → `https://github.com/OPEN-CID/cid`.
2. Build Pack: **Dockerfile** (the repo-root `Dockerfile`, headless-core-only per its own
   header comment). Base directory `/`.
3. Port: `5919`.
4. **Start Command override** (Coolify → resource → General): the image's default `CMD`
   has no `--allow-origin`, and that flag has no env-var form (`cid-core --help`) —
   ```
   cid-core --host 0.0.0.0 --port 5919 --db /home/cid/data/cid.db --allow-origin https://cid.opencid.dev
   ```
5. Environment Variables (runtime, not build): `CID_AUTH_TOKEN` — generate with
   `docker run --rm ghcr.io/open-cid/cid --generate-token` (or any built image) once, then
   paste the value in; Core refuses to start non-loopback without it (`SECURITY.md` §2).
6. Storage: persistent volume mounted at `/home/cid/data` (Coolify's volume UI, matching
   the image's own `VOLUME` declaration — already fixed this pass, see `docs/053` §4, to
   actually be owned by the unprivileged `cid` user instead of failing on first write).
7. Domain: `https://cid-core.opencid.dev`. Traefik issues the Let's Encrypt cert
   automatically, same as `houses`.
8. **This Dockerfile has still never been build-verified anywhere** (§4 below) — the
   first real build happens on Coolify itself. Watch the build log on first deploy;
   if it fails, that's the first real signal on this image, not a config mistake.

**Coolify resource 2 — the web client**:

1. Projects → New Resource → Public Repository → same repo.
2. Build Pack: **Nixpacks** (static site), Build Command `npm run build`, Publish
   Directory `dist`. No new Dockerfile needed — Vite's output is static assets, and this
   avoids inventing a second container image this session couldn't build-verify either.
3. **Build Variables** (tick "Build Variable", not runtime — `vite build` inlines these
   into the client bundle the same way `houses`' `NEXT_PUBLIC_*` are inlined into its
   Next.js bundle, §A4 step 4 of that project's runbook, same reasoning here):
   ```
   VITE_CID_CORE_HOST=cid-core.opencid.dev
   VITE_CID_CORE_PORT=
   VITE_CID_CORE_SECURE=true
   ```
4. Domain: `https://cid.opencid.dev`.
5. Deploy resource 1 first, resource 2 second — the frontend's build only matters once
   there's a Core at the host it's being told to bake in.

**Verify** (same shape as `houses` §C4 — check the server-rendered-equivalent claim and
the actual client bundle separately, since a build-variable mistake shows up in the
bundle, not the server):

1. `https://cid-core.opencid.dev/health` returns `200`, with `"auth_required": true`.
2. Open `https://cid.opencid.dev`. The banner asks for an access token — paste the same
   `CID_AUTH_TOKEN` value from resource 1 and press **Save and reconnect**. It is stored
   in that browser's `localStorage`, so each person does this once per device; it is
   never baked into the bundle.
3. Browser console: `[CID] Connected to core at wss://cid-core.opencid.dev/ws` — if it
   instead says `ws://` (not `wss://`) or shows a host of `127.0.0.1`, the Build Variables
   weren't set as *build* variables and the bundle needs rebuilding, not just redeploying.
   If the banner keeps asking for a token, the token is wrong, not the deployment.
4. Connect a repo, create a Mission, confirm a message round-trips — the real golden
   path, not just a reachable socket.

This Option C was **not verified end-to-end this session** — it depends on Coolify/Oracle
infrastructure this environment doesn't have access to, and on the Dockerfile's
first-ever real build (§4). Options A/B above were. Do §4's Dockerfile build-verification
before trusting resource 1 to come up clean on the first try.

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
